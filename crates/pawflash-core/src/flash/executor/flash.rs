use std::path::Path;
use std::time::Instant;

use indicatif::ProgressBar;
use tokio::io::AsyncReadExt;
use tracing::{debug, info, warn};

use crate::flash::error::{FlashError, Result};
use crate::flash::results::{FlashOutcome, FlashResult};
use crate::flash::transport::FlashTransport;
use crate::scatter_parser::types::FlashPlan;
use super::{parse_max_download, FlashExecutor};
use super::EMPTY_VBMETA;

impl<T: FlashTransport> FlashExecutor<T> {
    /// # Errors
    /// Returns an error if the download or flash command fails.
    ///
    /// # Panics
    /// Panics if `EMPTY_VBMETA` exceeds 4 GiB (impossible for a 512-byte image).
    pub async fn flash_empty_vbmeta(&mut self) -> Result<String> {
        let data = EMPTY_VBMETA;
        debug!("flashing empty vbmeta to both slots");
        let mut last_resp = String::new();
        for slot in &["a", "b"] {
            let partition = format!("vbmeta_{slot}");
            info!(%partition, "flashing empty vbmeta");
            let mut sender = self.fb.download(
                u32::try_from(data.len())
                    .expect("EMPTY_VBMETA is 512 bytes, always fits in u32"),
            ).await?;
            sender.extend_from_slice(data).await?;
            sender.finish().await?;
            last_resp = self.fb.flash(&partition).await?;
        }
        Ok(last_resp)
    }

    /// Flash a raw image to a partition. Public entry point for `flash-raw`.
    /// Returns the device response message.
    ///
    /// # Errors
    ///
    /// Returns an error if the image file cannot be read, the device cannot
    /// accept the data, or the flash command fails.
    pub async fn flash_raw_image(
        &mut self,
        partition: &str,
        image_path: &Path,
    ) -> Result<String> {
        debug!(%partition, image_path = %image_path.display(), "flash_raw_image entry");
        let max_download = parse_max_download(&mut self.fb).await?;

        self.flash_image_to_partition(partition, image_path, max_download, None).await
    }

    /// Shared helper: erase partition, then download+flash (single or chunked).
    /// Detects Android sparse images and routes to the sparse-aware handler.
    /// Returns the device response message.
    async fn flash_image_to_partition(
        &mut self,
        partition: &str,
        path: &Path,
        max_download: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<String> {
        // Shared transfer buffer reused across all sparse operations.
        let mut xbuf = crate::flash::sparse::XferBuf::new();

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
                &mut xbuf,
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

        if size > max_download {
            // Route through sparse wrapping to avoid each flash overwriting from
            // offset 0 (the fastbootd flash handler writes downloaded data at the
            // start of the partition; raw chunked flash would only leave the last
            // chunk intact).  Sparse wrapping encodes offset metadata so the device
            // writes each split to the correct position, matching AOSP behaviour.
            info!(%partition, file_len, %max_download, "image exceeds max download, wrapping in sparse format");
            crate::flash::sparse::flash_sparse_wrapped(
                &mut self.fb,
                partition,
                path,
                file_len,
                max_download,
                &mut xbuf,
            )
            .await
        } else {
            self.flash_raw_partition(partition, path, size, progress_bar, &mut xbuf).await
        }
    }

    /// # Errors
    /// Returns an error if the fastboot query fails.
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
        let max_download = self.fb.get_var("max-download-size").await.ok()
            .and_then(|s| fastboot_protocol::protocol::parse_u32(&s).ok())
            .unwrap_or(256 * 1024 * 1024);
        for action in &all_actions {
            let partition = &action.partition;
            info!(%partition, "Writing partition");
            let start = Instant::now();
            let result = self
                .flash_partition(action, dry_run, max_download, progress_bar)
                .await;
            let duration = start.elapsed();
            match result {
                Ok(response) => {
                    info!(%partition, duration = ?duration, response, "flash successful");
                    outcomes.push(FlashOutcome {
                        partition: partition.clone(),
                        success: true,
                        response: Some(response),
                        duration,
                        error: None,
                    });
                }
                Err(e) => {
                    warn!(%partition, duration = ?duration, error = %e, "flash failed, skipping");
                    if let Some(pb) = progress_bar {
                        pb.abandon_with_message(format!("{partition} failed"));
                    }
                    outcomes.push(FlashOutcome {
                        partition: partition.clone(),
                        success: false,
                        response: None,
                        duration,
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
        progress_bar: Option<&ProgressBar>,
    ) -> Result<String> {
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
            return Ok(String::new());
        }

        self.flash_image_to_partition(partition, path, max_download, progress_bar).await
    }

    /// Flash a partition that fits in a single download.
    /// Returns the device response message.
    pub(crate) async fn flash_raw_partition(
        &mut self,
        partition: &str,
        path: &Path,
        size: u32,
        progress_bar: Option<&ProgressBar>,
        xbuf: &mut crate::flash::sparse::XferBuf,
    ) -> Result<String> {
        debug!(%partition, file_size = size, "flashing raw partition");
        let mut file = tokio::fs::File::open(path).await?;
        let mut sender = self.fb.download(size).await?;

        let buf = xbuf.get(1024 * 1024);
        let mut written = 0u64;
        loop {
            let n = file.read(buf).await?;
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
        let resp = self.fb.flash(partition).await?;
        if let Some(pb) = progress_bar {
            pb.set_position(u64::from(size));
        }
        debug!(%partition, response = resp, "raw partition flash complete");
        Ok(resp)
    }
}
