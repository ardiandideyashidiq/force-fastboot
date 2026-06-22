use std::collections::HashMap;
use std::path::Path;

use fastboot_protocol::nusb::NusbFastBoot;
use indicatif::ProgressBar;
use tokio::io::AsyncReadExt;
use tracing::{debug, info, trace, warn};

use crate::flash::error::FlashError;
use crate::flash::error::Result;
use crate::flash::results::{FlashOutcome, FlashResult};
use crate::scatter_parser::types::FlashPlan;

/// Reboot target modes understood by fastboot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BootTarget {
    System,
    Bootloader,
    Fastboot,
    Recovery,
}

impl BootTarget {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Bootloader => "bootloader",
            Self::Fastboot => "fastboot",
            Self::Recovery => "recovery",
        }
    }
}

/// Fastboot flash executor.
pub struct FlashExecutor {
    pub(crate) fb: NusbFastBoot,
    device_vars: HashMap<String, String>,
}

#[allow(clippy::missing_errors_doc)]
impl FlashExecutor {
    /// Connect to the first available fastboot device and query its variables.
    pub async fn connect() -> Result<Self> {
        let mut devices = fastboot_protocol::nusb::devices()
            .await
            .map_err(|_| FlashError::NoDevice)?;

        let info = devices.next().ok_or_else(|| {
            #[cfg(target_os = "linux")]
            crate::flash::diagnostics::diagnose_fastboot_sysfs();
            FlashError::NoDevice
        })?;

        debug!(
            vidpid = format_args!("{:04x}:{:04x}", info.vendor_id(), info.product_id()),
            serial = info.serial_number().unwrap_or("?"),
            "connecting to fastboot device"
        );

        let mut fb = NusbFastBoot::from_info(&info).await?;

        // Some bootloaders hang on getvar:all (slow response), so use a
        // generous timeout and fall back to individual queries if it fails.
        let device_vars = match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            fb.get_all_vars(),
        )
            .await
        {
            Ok(Ok(vars)) => vars,
            Ok(Err(e)) => {
                debug!(error = %e, "getvar:all failed, falling back to individual queries");
                HashMap::new()
            }
            Err(_) => {
                debug!("getvar:all timed out, falling back to individual queries");
                HashMap::new()
            }
        };

        let device_vars = if device_vars.is_empty() {
            let mut vars: HashMap<String, String> = HashMap::new();
            for var in ["version", "product", "serialno", "current-slot", "max-download-size"] {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    fb.get_var(var),
                )
                    .await
                {
                    Ok(Ok(v)) => { vars.insert(var.to_string(), v); }
                    Ok(Err(e)) => { debug!(%var, error = %e, "getvar failed"); }
                    Err(_) => { debug!(%var, "getvar timed out"); }
                }
            }
            vars
        } else {
            device_vars
        };

        info!(
            product = device_vars.get("product").map_or("?", |s| s.as_str()),
            serial = device_vars.get("serialno").map_or("?", |s| s.as_str()),
            version = device_vars.get("version").map_or("?", |s| s.as_str()),
            "connected to fastboot device"
        );

        Ok(Self { fb, device_vars })
    }

    /// Return the cached device variables.
    #[must_use]
    pub const fn device_vars(&self) -> &HashMap<String, String> {
        &self.device_vars
    }

    /// Get a fastboot variable from the device.
    pub async fn get_var(&mut self, var: &str) -> Result<String> {
        self.fb.get_var(var).await.map_err(FlashError::from)
    }

    /// Get all fastboot variables from the device (with 10s timeout).
    pub async fn get_all_vars(&mut self) -> Result<HashMap<String, String>> {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.fb.get_all_vars(),
        )
            .await
            .map_err(|_| FlashError::NoDevice)?
            .map_err(FlashError::from)
    }

    /// Reboot the device (to system).
    pub async fn reboot(&mut self) -> Result<()> {
        self.fb.reboot().await.map_err(FlashError::from)
    }

    /// Reboot to a specific mode (bootloader, fastboot, recovery).
    pub async fn reboot_to(&mut self, target: BootTarget) -> Result<()> {
        self.fb.reboot_to(target.as_str()).await.map_err(FlashError::from)
    }

    /// Lock the bootloader.
    pub async fn flashing_lock(&mut self) -> Result<()> {
        self.fb.flashing("lock").await.map_err(FlashError::from)
    }

    /// Unlock the bootloader.
    pub async fn flashing_unlock(&mut self) -> Result<()> {
        self.fb.flashing("unlock").await.map_err(FlashError::from)
    }

    /// Set the active boot slot.
    pub async fn set_active_slot(&mut self, slot: &str) -> Result<()> {
        self.fb.set_active(slot).await.map_err(FlashError::from)
    }

    /// Verify that the connected device matches the expected platform/project.
    pub async fn verify_device(&mut self, plan: &FlashPlan) -> Result<()> {
        let product = self.fb.get_var("product").await?;
        if let Some(ref platform) = plan.platform {
            if product.to_lowercase() != platform.to_lowercase() {
                return Err(FlashError::DeviceMismatch {
                    expected: platform.clone(),
                    actual: product,
                });
            }
        }
        debug!(%product, platform = ?plan.platform, "device identity verified");
        Ok(())
    }

    /// Execute a flash plan. Skips failed actions and continues.
    /// In dry-run mode, verifies device + images without writing.
    /// When `progress_bar` is `Some`, partition flash progress is shown on the bar.
    pub async fn execute_plan(
        &mut self,
        plan: &FlashPlan,
        dry_run: bool,
        progress_bar: Option<&ProgressBar>,
    ) -> FlashResult {
        let all_actions: Vec<_> = plan.actions.iter().filter(|a| a.action == "flash").collect();
        let total = all_actions.len();
        let mut outcomes = Vec::with_capacity(total);

        if dry_run {
            info!(total, "DRY RUN — no data will be written");
        } else {
            info!(total, "starting flash execution");
        }

        // Query max download size
        let max_download = self.fb.get_var("max-download-size").await.ok()
            .and_then(|s| fastboot_protocol::protocol::parse_u32(&s).ok())
            .unwrap_or(256 * 1024 * 1024);

        for action in &all_actions {
            let partition = &action.partition;
            trace!(%partition, "processing flash action");

            let result = self
                .flash_partition(action, dry_run, max_download, progress_bar)
                .await;

            match result {
                Ok(()) => {
                    info!(%partition, "flash successful");
                    outcomes.push(FlashOutcome {
                        partition: partition.clone(),
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    warn!(%partition, error = %e, "flash failed, skipping");
                    if let Some(pb) = progress_bar {
                        pb.abandon_with_message(format!("{partition} failed"));
                    }
                    outcomes.push(FlashOutcome {
                        partition: partition.clone(),
                        success: false,
                        error: Some(e),
                    });
                }
            }
        }

        let succeeded = outcomes.iter().filter(|o| o.success).count();
        let failed = outcomes.iter().filter(|o| !o.success).count();

        info!(succeeded, failed, total, "flash plan execution complete");

        FlashResult { total, succeeded, failed, outcomes }
    }

    /// Flash the vendored empty vbmeta image to both slots.
    /// This disables dm-verity and AVB verification (flags=3).
    pub async fn flash_empty_vbmeta(&mut self) -> Result<()> {
        let data = crate::flash::vbmeta::EMPTY_VBMETA;
        debug!("flashing empty vbmeta to both slots");
        for slot in &["a", "b"] {
            let partition = format!("vbmeta_{slot}");
            info!(%partition, "flashing empty vbmeta");
            let mut sender = self.fb.download(u32::try_from(data.len()).unwrap_or(u32::MAX)).await?;
            sender.extend_from_slice(data).await?;
            sender.finish().await?;
            self.fb.flash(&partition).await?;
        }
        Ok(())
    }

    /// Flash a raw image to a partition. Public entry point for `flash-raw`.
    pub async fn flash_raw_image(
        &mut self,
        partition: &str,
        image_path: &Path,
    ) -> Result<()> {
        debug!(%partition, image_path = %image_path.display(), "flash_raw_image entry");
        let max_download = self.fb.get_var("max-download-size").await.ok()
            .and_then(|s| fastboot_protocol::protocol::parse_u32(&s).ok())
            .unwrap_or(256 * 1024 * 1024);

        self.flash_image_to_partition(partition, image_path, max_download, None).await
    }

    /// Shared helper: erase partition, then download+flash (single or chunked).
    /// Detects Android sparse images and routes to the sparse-aware handler.
    async fn flash_image_to_partition(
        &mut self,
        partition: &str,
        path: &Path,
        max_download: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<()> {
        // Route Android sparse images through the sparse-aware handler.
        if crate::flash::sparse::is_sparse_image(path).await.unwrap_or(false) {
            let file_len = tokio::fs::metadata(path).await?.len();
            return crate::flash::sparse::flash_sparse_image(
                &mut self.fb,
                partition,
                path,
                file_len,
                max_download,
                progress_bar,
            )
            .await;
        }

        let file_len = tokio::fs::metadata(path).await?.len();
        let size = u32::try_from(file_len).unwrap_or(u32::MAX);

        if let Some(pb) = progress_bar {
            pb.set_length(file_len);
            pb.set_prefix(partition.to_string());
            pb.reset();
            pb.set_position(0);
        }

        debug!(%partition, file_size = file_len, max_download, "flashing image to partition");

        self.fb.erase(partition).await?;

        if size > max_download {
            info!(%partition, size = file_len, %max_download, "image exceeds max download, splitting into chunks");
            self.flash_large_partition(partition, path, file_len, max_download, progress_bar).await
        } else {
            self.flash_raw_partition(partition, path, size, progress_bar).await
        }
    }

    async fn flash_partition(
        &mut self,
        action: &crate::scatter_parser::types::FlashAction,
        dry_run: bool,
        max_download: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<()> {
        let partition = &action.partition;
        let Some(image_path) = action.image_resolved_path() else {
            return Err(FlashError::ActionFailed {
                partition: partition.clone(),
                reason: "no resolved image path".into(),
            });
        };

        let path = Path::new(image_path);
        if !path.exists() {
            return Err(FlashError::ImageNotFound(path.to_path_buf()));
        }

        debug!(
            %partition,
            %image_path,
            max_download,
            "checking image"
        );

        if dry_run {
            let file_len = tokio::fs::metadata(path).await?.len();
            info!(%partition, %image_path, size = file_len, "dry run: would flash this image");
            return Ok(());
        }

        self.flash_image_to_partition(partition, path, max_download, progress_bar).await
    }

    /// Flash a partition that fits in a single download.
    pub(crate) async fn flash_raw_partition(
        &mut self,
        partition: &str,
        path: &Path,
        size: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<()> {
        debug!(%partition, file_size = size, "flashing raw partition");
        let mut file = tokio::fs::File::open(path).await?;
        let mut sender = self.fb.download(size).await?;

        let mut buf = vec![0u8; 1024 * 1024];
        let mut written = 0u64;
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            sender.extend_from_slice(&buf[..n]).await?;
            written += n as u64;
            if let Some(pb) = progress_bar {
                pb.set_position(written);
            }
        }

        sender.finish().await?;
        self.fb.flash(partition).await?;
        if let Some(pb) = progress_bar {
            pb.set_position(u64::from(size));
        }
        debug!(%partition, "raw partition flash complete");
        Ok(())
    }

    /// Flash a partition by splitting into chunks that fit `max_download_size`.
    pub(crate) async fn flash_large_partition(
        &mut self,
        partition: &str,
        path: &Path,
        file_len: u64,
        max_download: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<()> {
        debug!(%partition, file_len, max_download, "starting chunked flash");
        let chunk_size = u64::from(max_download);
        let mut file = tokio::fs::File::open(path).await?;
        let mut remaining = file_len;
        let mut chunk_index = 0u32;
        let mut buf = vec![0u8; 1024 * 1024];

        while remaining > 0 {
            let this_chunk = u32::try_from(remaining.min(chunk_size)).unwrap_or(u32::MAX);
            info!(%partition, chunk = chunk_index, size = this_chunk, "sending chunk");

            let mut sender = self.fb.download(this_chunk).await?;
            let mut to_send = u64::from(this_chunk);
            while to_send > 0 {
                let limit = buf.len().min(usize::try_from(to_send).unwrap_or(usize::MAX));
                let n = file.read(&mut buf[..limit]).await?;
                if n == 0 {
                    return Err(FlashError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "unexpected EOF while streaming chunk",
                    )));
                }
                sender.extend_from_slice(&buf[..n]).await?;
                to_send = to_send.saturating_sub(n as u64);
            }

            sender.finish().await?;
            self.fb.flash(partition).await?;

            remaining = remaining.saturating_sub(u64::from(this_chunk));

            if let Some(pb) = progress_bar {
                pb.inc(u64::from(this_chunk));
            }

            chunk_index += 1;
        }
        if let Some(pb) = progress_bar {
            pb.set_position(file_len);
        }

        debug!(%partition, chunks = chunk_index, "chunked flash complete");
        Ok(())
    }
}


