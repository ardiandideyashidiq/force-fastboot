use std::path::Path;

use tracing::{debug, error, info, warn};

use crate::flash::error::FlashError;
use crate::flash::executor::FlashExecutor;
use crate::flash::results::{FormatDataResult, FormatOutcome, FormatStatus};
use crate::format::generator::{self, FsType};

impl FlashExecutor {
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

        let max_download = crate::flash::executor::parse_max_download(&mut self.fb)
            .await
            .unwrap_or(256 * 1024 * 1024);

        let erase_blk = self
            .fb
            .get_var("erase-block-size")
            .await
            .ok()
            .and_then(|s| parse_getvar_hex_u64(&s).and_then(|v| u32::try_from(v).ok()))
            .unwrap_or(0);
        let logical_blk = self
            .fb
            .get_var("logical-block-size")
            .await
            .ok()
            .and_then(|s| parse_getvar_hex_u64(&s).and_then(|v| u32::try_from(v).ok()))
            .unwrap_or(0);

        for partition in &partitions {
            let outcome = self
                .wipe_partition(partition, fs_options, max_download, erase_blk, logical_blk, &tools_dir)
                .await;
            match &outcome.status {
                FormatStatus::Wiped => info!(%partition, "wiped ✓"),
                FormatStatus::ErasedOnly(fs) => {
                    info!(%partition, fs_type = %fs, "erased only (unrecognized filesystem)");
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
    #[allow(clippy::too_many_lines)]
    async fn wipe_partition(
        &mut self,
        partition: &str,
        fs_options: u32,
        max_download: u32,
        erase_blk: u32,
        logical_blk: u32,
        tools_dir: &Path,
    ) -> FormatOutcome {
        debug!(%partition, "wipe_partition: querying partition type");
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
        debug!(%partition, "sending erase command");
        if let Err(e) = self.fb.erase(partition).await {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(FlashError::from(e)),
            };
        }

        // 3. determine filesystem type
        debug!(%partition, %partition_type, "determining filesystem type");
        let Some(fs_type) = FsType::from_partition_type(&partition_type) else {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::ErasedOnly(partition_type),
            };
        };

        // 4. query partition size
        debug!(%partition, "querying partition size");
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

        // 5. generate empty filesystem image (block sizes pre-fetched)
        debug!(%partition, %partition_type, part_size, "generating empty filesystem");
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

        info!(%partition, size = file_len, fs_type = %partition_type, "flashing empty filesystem via sparse wrap");

        let result = crate::flash::sparse::flash_sparse_wrapped(
            &mut self.fb,
            partition,
            &output_path,
            file_len,
            max_download,
        )
        .await;

        match result {
            Ok(_) => FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Wiped,
            },
            Err(e) => FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(e),
            },
        }
    }
}

/// Parse a bootloader-reported numeric variable as hex.
/// Some bootloaders omit the `0x` prefix; AOSP always treats the value as hex.
fn parse_getvar_hex_u64(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}
