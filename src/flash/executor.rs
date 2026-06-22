use std::collections::HashMap;
use std::path::Path;

use fastboot_protocol::nusb::NusbFastBoot;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info, trace, warn};

use crate::flash::error::FlashError;
use crate::flash::error::Result;
use crate::format::generator::{self, FsType};
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

/// Outcome of a format-data operation on a single partition.
#[derive(Debug)]
pub struct FormatOutcome {
    pub partition: String,
    pub status: FormatStatus,
}

/// Per-partition format status.
#[derive(Debug)]
pub enum FormatStatus {
    /// Fully wiped and formatted with an empty filesystem.
    Wiped,
    /// Erased but not formatted (filesystem type not recognised).
    ErasedOnly(String),
    /// Skipped (partition does not exist or empty type).
    Skipped(String),
    /// Operation failed with the given error.
    Failed(FlashError),
}

/// Result of a full format-data run.
#[derive(Debug)]
pub struct FormatDataResult {
    pub outcomes: Vec<FormatOutcome>,
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

    /// Erase userdata, cache, and metadata, then format with an empty filesystem.
    /// Equivalent to `fastboot -w`.
    pub async fn format_data(&mut self, fs_options: u32) -> FormatDataResult {
        let partitions = ["userdata", "cache", "metadata"];
        let mut outcomes = Vec::with_capacity(partitions.len());

        info!(partitions = ?partitions, "starting format-data");

        let (_tools, tools_dir) = match generator::extract_format_tools() {
            Ok(pair) => pair,
            Err(e) => {
                let reason = format!("failed to extract format tools: {e}");
                error!(%reason);
                for partition in &partitions {
                    outcomes.push(FormatOutcome {
                        partition: partition.to_string(),
                        status: FormatStatus::Failed(FlashError::GeneratorFailed {
                            reason: reason.clone(),
                        }),
                    });
                }
                return FormatDataResult { outcomes };
            }
        };

        let max_download = self
            .fb
            .get_var("max-download-size")
            .await
            .ok()
            .and_then(|s| fastboot_protocol::protocol::parse_u32(&s).ok())
            .unwrap_or(256 * 1024 * 1024);

        for partition in &partitions {
            let outcome = self
                .wipe_partition(partition, fs_options, max_download, &tools_dir)
                .await;
            match &outcome.status {
                FormatStatus::Wiped => info!(%partition, "wiped ✓"),
                FormatStatus::ErasedOnly(fs) => {
                    info!(%partition, fs_type = %fs, "erased only (unrecognized filesystem)")
                }
                FormatStatus::Skipped(reason) => info!(%partition, %reason, "skipped"),
                FormatStatus::Failed(e) => warn!(%partition, error = %e, "failed"),
            }
            outcomes.push(outcome);
        }

        let wiped = outcomes.iter().filter(|o| matches!(o.status, FormatStatus::Wiped)).count();
        let failed = outcomes.iter().filter(|o| matches!(o.status, FormatStatus::Failed(_))).count();
        let erased_only = outcomes.iter().filter(|o| matches!(o.status, FormatStatus::ErasedOnly(_))).count();
        let skipped = outcomes.iter().filter(|o| matches!(o.status, FormatStatus::Skipped(_))).count();
        info!(wiped, erased_only, skipped, failed, "format-data complete");
        FormatDataResult { outcomes }
    }

    /// Erase, generate empty filesystem, download, and flash a single partition.
    async fn wipe_partition(
        &mut self,
        partition: &str,
        fs_options: u32,
        max_download: u32,
        tools_dir: &Path,
    ) -> FormatOutcome {
        // 1. query partition-type — skip if nonexistent
        let partition_type = match self.fb.get_var(&format!("partition-type:{partition}")).await {
            Ok(t) if !t.is_empty() => t,
            Ok(_) => {
                return FormatOutcome {
                    partition: partition.into(),
                    status: FormatStatus::Skipped("empty partition type".into()),
                };
            }
            Err(_) => {
                return FormatOutcome {
                    partition: partition.into(),
                    status: FormatStatus::Skipped("partition not found".into()),
                };
            }
        };

        // 2. erase
        info!(%partition, "erasing");
        if let Err(e) = self.fb.erase(partition).await {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(FlashError::from(e)),
            };
        }

        // 3. determine filesystem type
        let fs_type = match FsType::from_partition_type(&partition_type) {
            Some(t) => t,
            None => {
                return FormatOutcome {
                    partition: partition.into(),
                    status: FormatStatus::ErasedOnly(partition_type),
                };
            }
        };

        // 4. query partition size
        let part_size = match self.fb.get_var(&format!("partition-size:{partition}")).await {
            Ok(s) => parse_getvar_hex_u64(&s).unwrap_or(0),
            Err(e) => {
                return FormatOutcome {
                    partition: partition.into(),
                    status: FormatStatus::Failed(FlashError::from(e)),
                };
            }
        };
        if part_size == 0 {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(FlashError::ActionFailed {
                    partition: partition.into(),
                    reason: "partition size is 0".into(),
                }),
            };
        }

        // 5. optional block sizes for stride optimisation
        let erase_blk = self
            .fb
            .get_var("erase-block-size")
            .await
            .ok()
            .and_then(|s| {
                parse_getvar_hex_u64(&s)
                    .and_then(|v| u32::try_from(v).ok())
            })
            .unwrap_or(0);
        let logical_blk = self
            .fb
            .get_var("logical-block-size")
            .await
            .ok()
            .and_then(|s| {
                parse_getvar_hex_u64(&s)
                    .and_then(|v| u32::try_from(v).ok())
            })
            .unwrap_or(0);

        // 6. generate empty filesystem image
        let output_path = tools_dir.join("format.img");
        if let Err(e) = generator::generate_empty_fs(
            tools_dir,
            &output_path,
            fs_type,
            part_size,
            erase_blk,
            logical_blk,
            fs_options,
        )
        .await
        {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(e),
            };
        }

        // 7. download + flash
        let file_len = match tokio::fs::metadata(&output_path).await {
            Ok(m) => m.len(),
            Err(e) => {
                return FormatOutcome {
                    partition: partition.into(),
                    status: FormatStatus::Failed(FlashError::Io(e)),
                };
            }
        };

        let size = u32::try_from(file_len).unwrap_or(u32::MAX);

        info!(%partition, size = file_len, fs_type = %partition_type, "flashing empty filesystem");

        let result = if size > max_download {
            self.flash_large_partition(partition, &output_path, file_len, max_download)
                .await
        } else {
            self.flash_raw_partition(partition, &output_path, size).await
        };

        match result {
            Ok(()) => FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Wiped,
            },
            Err(e) => FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(e),
            },
        }
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

    /// Flash a raw image to a partition. Public entry point for `flash-raw`.
    pub async fn flash_raw_image(
        &mut self,
        partition: &str,
        image_path: &Path,
    ) -> Result<()> {
        let max_download = self.fb.get_var("max-download-size").await.ok()
            .and_then(|s| fastboot_protocol::protocol::parse_u32(&s).ok())
            .unwrap_or(256 * 1024 * 1024);

        self.flash_image_to_partition(partition, image_path, max_download).await
    }

    /// Shared helper: erase partition, then download+flash (single or chunked).
    async fn flash_image_to_partition(
        &mut self,
        partition: &str,
        path: &Path,
        max_download: u32,
    ) -> Result<()> {
        let file_len = tokio::fs::metadata(path).await?.len();
        let size = u32::try_from(file_len).unwrap_or(u32::MAX);

        debug!(%partition, file_size = file_len, max_download, "flashing image to partition");

        self.fb.erase(partition).await?;

        if size > max_download {
            info!(%partition, size = file_len, %max_download, "image exceeds max download, splitting into chunks");
            self.flash_large_partition(partition, path, file_len, max_download).await
        } else {
            self.flash_raw_partition(partition, path, size).await
        }
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

        self.flash_image_to_partition(partition, path, max_download).await
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

/// Parse a bootloader-reported numeric variable as hex.
/// Some bootloaders omit the `0x` prefix; AOSP always treats the value as hex.
fn parse_getvar_hex_u64(s: &str) -> Option<u64> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s.trim());
    if s.is_empty() {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}
