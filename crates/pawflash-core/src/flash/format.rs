use std::path::Path;

use tracing::{debug, error, info, warn};

use crate::flash::error::FlashError;
use crate::flash::executor::FlashExecutor;
use crate::flash::results::{FormatDataResult, FormatOutcome, FormatStatus};
use crate::flash::sparse::CRYPT_FOOTER_OFFSET;
use crate::flash::transport::FlashTransport;
use crate::format::generator;
use crate::format::generator::FsType;

struct WiperConfig<'a> {
    fs_options: u32,
    max_download: u32,
    erase_blk: u32,
    logical_blk: u32,
    tools_dir: &'a Path,
    clean_test: bool,
}

impl<T: FlashTransport> FlashExecutor<T> {
    /// Erase and format userdata, metadata, and cache.
    ///
    /// 1. Checks that we are in **fastbootd** mode (warns if not).
    /// 2. Cancels pending OTA snapshots via `snapshot-update:cancel`.
    /// 3. Erases + creates a fresh filesystem on each partition.
    /// 4. For `userdata` on legacy FDE (ext4 + footer) devices the filesystem
    ///    is shrunk by [`CRYPT_FOOTER_OFFSET`] (16 KiB) and the footer zeroed.
    ///    When the filesystem type is `f2fs` no footer offset is applied
    ///    (modern Android uses inline encryption, not an on-disk footer).
    ///
    /// `fs_type_override` — when `Some(FsType)`, applies that type to
    /// **userdata** even if the bootloader reports a different type
    /// (e.g. MTK's fastbootd HAL reports `raw` for both f2fs and ext4).
    /// When `None`, the type is auto-detected from `partition-type:` and
    /// falls back to **f2fs** for userdata, **ext4** for metadata/cache.
    ///
    /// When `clean_test` is true, only erases — skips filesystem generation.
    pub async fn format_data(
        &mut self,
        fs_options: u32,
        clean_test: bool,
        fs_type_override: Option<FsType>,
    ) -> FormatDataResult {
        let partitions = ["userdata", "metadata", "cache"];

        // Warn if not in fastbootd — caller should have transitioned.
        let is_fastbootd = self
            .fb
            .get_var("is-userspace")
            .await
            .is_ok_and(|v| v == "yes");
        if !is_fastbootd {
            warn!("format-data: device is in bootloader mode — logical partitions may not be accessible");
        }

        // Cancel any pending OTA snapshot state so the bootloader doesn't
        // try to merge stale COW data on next boot.
        info!("cancelling pending OTA snapshots");
        match self.fb.snapshot_update("cancel").await {
            Ok(resp) => debug!(response = %resp, "snapshot-update:cancel succeeded"),
            Err(ref e) => {
                debug!(error = %e, "snapshot-update:cancel failed (may be normal if no OTA was pending)");
            }
        }

        // Clear the bootloader control block (misc partition) so the
        // bootloader doesn't find a stale "boot-recovery" command and enter
        // recovery on next boot — which would fail and bootloop.
        self.clear_bootloader_bcb().await;

        info!(partitions = ?partitions, clean_test, "starting format-data");

        let (_tools, tools_dir) = match generator::extract_format_tools() {
            Ok(pair) => pair,
            Err(e) => {
                let reason = format!("failed to extract format tools: {e}");
                error!(%reason);
                let mut outcomes = Vec::with_capacity(partitions.len());
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

        let mut outcomes = Vec::with_capacity(partitions.len());
        for partition in &partitions {
            // Crypto footer offset only applies to ext4 userdata (legacy FDE).
            // f2fs + inlinecrypt has no on-disk footer.
            let footer_size = match (*partition, fs_type_override.unwrap_or(FsType::F2fs)) {
                ("userdata", FsType::Ext4) => CRYPT_FOOTER_OFFSET,
                _ => 0,
            };

            let wc = WiperConfig {
                fs_options,
                max_download,
                erase_blk,
                logical_blk,
                tools_dir: &tools_dir,
                clean_test,
            };

            let outcome = self
                .wipe_partition(
                    &wc,
                    partition,
                    footer_size,
                    fs_type_override,
                )
                .await;
            match &outcome.status {
                FormatStatus::Wiped => info!(%partition, "wiped"),
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
        let erased_only =
            outcomes.iter().filter(|o| matches!(o.status, FormatStatus::ErasedOnly(_))).count();
        let skipped = outcomes.iter().filter(|o| matches!(o.status, FormatStatus::Skipped(_))).count();
        info!(wiped, erased_only, skipped, failed, "format-data complete");

        FormatDataResult { outcomes }
    }

    /// Zero the bootloader control block (BCB) in the misc partition so the
    /// bootloader doesn't boot into recovery on next start.  Recovery's
    /// `FinishRecovery()` calls `clear_bootloader_message()` which does the
    /// same — without it a stale "boot-recovery" command left by a failed or
    /// incomplete recovery attempt can cause an endless bootloop.
    ///
    /// We write 2048 zero bytes (the size of a standard BCB struct) to the
    /// misc partition, or to `para` as a fallback (used by some MTK devices).
    /// This preserves calibration data stored beyond the BCB area.
    async fn clear_bootloader_bcb(&mut self) {
        let partitions = ["misc", "para"];
        let bcb_size = 2048u32;
        let bcb_buf = vec![0u8; bcb_size as usize];

        for part in &partitions {
            let exists = match self.fb.get_var(&format!("partition-type:{part}")).await {
                Ok(t) => !t.is_empty(),
                Err(_) => false,
            };
            if !exists {
                debug!(partition = *part, "BCB partition not found");
                continue;
            }
            let Ok(mut sender) = self.fb.download(bcb_size).await else {
                warn!(partition = *part, "BCB download failed");
                continue;
            };
            if sender.extend_from_slice(&bcb_buf).await.is_err() {
                warn!(partition = *part, "BCB data transfer failed");
                continue;
            }
            if sender.finish().await.is_err() {
                warn!(partition = *part, "BCB download finalise failed");
                continue;
            }
            match self.fb.flash(part).await {
                Ok(resp) =>             info!(partition = *part, response = %resp, "BCB cleared"),
                Err(e) => warn!(partition = *part, error = %e, "BCB flash failed"),
            }
            return; // success on first writable partition
        }

        debug!("no BCB-capable partition found (misc/para)");
    }

    async fn partition_type(&mut self, partition: &str) -> Result<String, FormatOutcome> {
        match self.fb.get_var(&format!("partition-type:{partition}")).await {
            Ok(t) if !t.is_empty() => Ok(t),
            Ok(_) => Err(FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Skipped("empty partition type".into()),
            }),
            Err(_) => Err(FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Skipped("partition not found".into()),
            }),
        }
    }

    fn determine_fs_type(partition: &str, partition_type: &str, fs_type_override: Option<FsType>) -> Result<FsType, FormatOutcome> {
        match (partition, fs_type_override) {
            ("userdata", Some(t)) => Ok(t),
            (_, _) => match FsType::from_partition_type(partition_type) {
                Some(t) => Ok(t),
                None if partition == "userdata" => {
                    info!(%partition, reported = %partition_type, "defaulting to f2fs");
                    Ok(FsType::F2fs)
                }
                None if ["metadata", "cache"].contains(&partition) => {
                    info!(%partition, reported = %partition_type, "defaulting to ext4");
                    Ok(FsType::Ext4)
                }
                None => Err(FormatOutcome {
                    partition: partition.into(),
                    status: FormatStatus::ErasedOnly(partition_type.to_string()),
                }),
            },
        }
    }

    async fn query_partition_size(&mut self, partition: &str) -> Result<u64, FormatOutcome> {
        match self.fb.get_var(&format!("partition-size:{partition}")).await {
            Ok(s) => {
                let size = parse_getvar_hex_u64(&s).unwrap_or(0);
                if size == 0 {
                    Err(FormatOutcome {
                        partition: partition.into(),
                        status: FormatStatus::Failed(FlashError::ActionFailed {
                            partition: partition.into(),
                            reason: "partition size is 0".into(),
                        }),
                    })
                } else {
                    Ok(size)
                }
            }
            Err(e) => Err(FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(e),
            }),
        }
    }

    /// Erase, generate empty filesystem, download, and flash a single partition.
    async fn wipe_partition(
        &mut self,
        wc: &WiperConfig<'_>,
        partition: &str,
        footer_size: u64,
        fs_type_override: Option<FsType>,
    ) -> FormatOutcome {
        debug!(%partition, "wipe_partition: querying partition type");
        let partition_type = match self.partition_type(partition).await {
            Ok(t) => t,
            Err(outcome) => return outcome,
        };

        info!(%partition, "erasing");
        if let Err(e) = self.fb.erase(partition).await {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(e),
            };
        }

        let fs_type = match Self::determine_fs_type(partition, &partition_type, fs_type_override) {
            Ok(t) => t,
            Err(outcome) => return outcome,
        };

        if wc.clean_test {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::ErasedOnly(partition_type),
            };
        }

        let part_size = match self.query_partition_size(partition).await {
            Ok(s) => s,
            Err(outcome) => return outcome,
        };

        let fs_size = part_size.saturating_sub(footer_size);
        debug!(%partition, %partition_type, part_size, fs_size, footer_size, "generating empty filesystem");
        let output_path = wc.tools_dir.join("format.img");
        if let Err(e) = generator::generate_empty_fs(
            wc.tools_dir,
            &output_path,
            fs_type,
            fs_size,
            wc.erase_blk,
            wc.logical_blk,
            wc.fs_options,
        )
        .await
        {
            return FormatOutcome {
                partition: partition.into(),
                status: FormatStatus::Failed(e),
            };
        }

        info!(%partition, part_size, footer_size, "flashing empty filesystem via sparse wrap");

        let mut xbuf = crate::flash::sparse::XferBuf::new();
        let result = crate::flash::sparse::sparse_wrap_file(
            &mut self.fb,
            partition,
            &output_path,
            part_size,
            wc.max_download,
            footer_size,
            &mut xbuf,
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

fn parse_getvar_hex_u64(s: &str) -> Option<u64> {
    let s = s.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    u64::from_str_radix(s, 16).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::parse_getvar_hex_u64;
    use crate::flash::mock::MockTransport;
    use crate::flash::executor::FlashExecutor;
    use crate::flash::results::FormatStatus;

    #[test]
    fn parse_getvar_hex_u64_should_accept_0x_prefix() {
        assert_eq!(parse_getvar_hex_u64("0x100000"), Some(0x0010_0000));
    }

    #[test]
    fn parse_getvar_hex_u64_should_accept_upper_x_prefix() {
        assert_eq!(parse_getvar_hex_u64("0X200000"), Some(0x0020_0000));
    }

    #[test]
    fn parse_getvar_hex_u64_should_accept_no_prefix() {
        assert_eq!(parse_getvar_hex_u64("abcdef"), Some(0x00ab_cdef));
    }

    #[test]
    fn parse_getvar_hex_u64_should_trim_whitespace() {
        assert_eq!(parse_getvar_hex_u64("  0x100  "), Some(0x100));
    }

    #[test]
    fn parse_getvar_hex_u64_should_return_none_for_empty() {
        assert_eq!(parse_getvar_hex_u64(""), None);
        assert_eq!(parse_getvar_hex_u64("0x"), None);
    }

    #[test]
    fn parse_getvar_hex_u64_should_return_none_for_invalid() {
        assert_eq!(parse_getvar_hex_u64("not_hex"), None);
    }

    #[tokio::test]
    async fn format_data_handles_missing_partition() {
        let fb = MockTransport::new();
        let mut exec = FlashExecutor::new(fb, HashMap::new());
        let result = exec.format_data(0, false, None).await;
        // With no partition-type responses configured, all three partitions are skipped
        assert_eq!(result.outcomes.len(), 3);
        for outcome in &result.outcomes {
            assert!(matches!(outcome.status, FormatStatus::Skipped(_)), "expected skipped, got {:?}", outcome.status);
        }
    }
}
