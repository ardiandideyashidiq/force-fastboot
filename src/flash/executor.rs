use std::collections::HashMap;
use std::path::Path;
use tokio::io::AsyncReadExt;

use fastboot_protocol::nusb::NusbFastBoot;
use tracing::{debug, info, trace, warn};

use crate::flash::error::FlashError;
use crate::flash::error::Result;
use crate::scatter_parser::types::FlashPlan;

/// Outcome of a single flash action.
#[derive(Debug)]
pub struct FlashOutcome {
    pub partition: String,
    pub success: bool,
    pub error: Option<FlashError>,
}

/// Overall result of executing a flash plan.
#[derive(Debug)]
pub struct FlashResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub outcomes: Vec<FlashOutcome>,
}

/// Fastboot flash executor.
pub struct FlashExecutor {
    fb: NusbFastBoot,
    device_vars: HashMap<String, String>,
}

impl FlashExecutor {
    /// Connect to the first available fastboot device and query its variables.
    pub async fn connect() -> Result<Self> {
        let mut devices = fastboot_protocol::nusb::devices()
            .await
            .map_err(|_| FlashError::NoDevice)?;

        let info = devices
            .next()
            .ok_or(FlashError::NoDevice)?;

        debug!(
            vidpid = format_args!("{:04x}:{:04x}", info.vendor_id(), info.product_id()),
            serial = info.serial_number().unwrap_or("?"),
            "connecting to fastboot device"
        );

        let mut fb = NusbFastBoot::from_info(&info).await?;
        let device_vars = fb.get_all_vars().await?;

        info!(
            product = device_vars.get("product").map_or("?", |s| s.as_str()),
            serial = device_vars.get("serialno").map_or("?", |s| s.as_str()),
            version = device_vars.get("version").map_or("?", |s| s.as_str()),
            "connected to fastboot device"
        );

        Ok(Self { fb, device_vars })
    }

    /// Return the cached device variables.
    pub fn device_vars(&self) -> &HashMap<String, String> {
        &self.device_vars
    }

    /// Get a fastboot variable from the device.
    pub async fn get_var(&mut self, var: &str) -> Result<String> {
        self.fb.get_var(var).await.map_err(FlashError::from)
    }

    /// Get all fastboot variables from the device.
    pub async fn get_all_vars(&mut self) -> Result<HashMap<String, String>> {
        self.fb.get_all_vars().await.map_err(FlashError::from)
    }

    /// Reboot the device (to system by default).
    pub async fn reboot(&mut self) -> Result<()> {
        self.fb.reboot().await.map_err(FlashError::from)
    }

    /// Reboot to a specific mode (bootloader, fastboot, recovery).
    pub async fn reboot_to(&mut self, mode: &str) -> Result<()> {
        self.fb.reboot_to(mode).await.map_err(FlashError::from)
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
    pub async fn execute_plan(&mut self, plan: &FlashPlan, dry_run: bool) -> FlashResult {
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
                .flash_partition(action, dry_run, max_download)
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

    async fn flash_partition(
        &mut self,
        action: &crate::scatter_parser::types::FlashAction,
        dry_run: bool,
        max_download: u32,
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

        let file_len = tokio::fs::metadata(path).await?.len();
        let size = u32::try_from(file_len).unwrap_or(u32::MAX);

        debug!(
            %partition,
            %image_path,
            file_size = file_len,
            max_download = max_download,
            "checking image"
        );

        if dry_run {
            info!(%partition, %image_path, size = file_len, "dry run: would flash this image");
            return Ok(());
        }

        self.fb.erase(partition).await?;

        if size > max_download {
            // Large file: split into chunks
            info!(%partition, size = file_len, %max_download, "image exceeds max download, splitting into chunks");
            self.flash_large_partition(partition, path, file_len, max_download).await
        } else {
            self.flash_raw_partition(partition, path, size).await
        }
    }

    /// Flash a partition that fits in a single download.
    async fn flash_raw_partition(
        &mut self,
        partition: &str,
        path: &Path,
        size: u32,
    ) -> Result<()> {
        let mut file = tokio::fs::File::open(path).await?;
        let mut sender = self.fb.download(size).await?;

        loop {
            let left = sender.left();
            if left == 0 {
                break;
            }
            let buf = sender.get_mut_data(left as usize).await?;
            let total = left as usize;
            let mut offset = 0;
            while offset < total {
                let n = file.read(&mut buf[offset..total]).await?;
                if n == 0 {
                    break;
                }
                offset += n;
            }
        }

        sender.finish().await?;
        self.fb.flash(partition).await?;
        Ok(())
    }

    /// Flash a partition by splitting into chunks that fit max_download_size.
    async fn flash_large_partition(
        &mut self,
        partition: &str,
        path: &Path,
        file_len: u64,
        max_download: u32,
    ) -> Result<()> {
        let chunk_size = max_download as usize;
        let mut file = tokio::fs::File::open(path).await?;
        let mut remaining = file_len;
        let mut chunk_index = 0u32;

        while remaining > 0 {
            let this_chunk = remaining.min(chunk_size as u64) as u32;
            info!(%partition, chunk = chunk_index, size = this_chunk, "sending chunk");

            let mut sender = self.fb.download(this_chunk).await?;

            loop {
                let left = sender.left();
                if left == 0 {
                    break;
                }
                let buf = sender.get_mut_data(left as usize).await?;
                let total = left as usize;
                let mut offset = 0;
                while offset < total {
                    let n = file.read(&mut buf[offset..total]).await?;
                    if n == 0 {
                        break;
                    }
                    offset += n;
                }
            }

            sender.finish().await?;
            self.fb.flash(partition).await?;

            remaining = remaining.saturating_sub(this_chunk as u64);
            chunk_index += 1;
        }

        Ok(())
    }
}
