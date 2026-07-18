use std::collections::HashMap;
use std::time::Duration;

use android_sparse_image::{FileHeader, FILE_HEADER_BYTES_LEN};
use tokio::io::AsyncReadExt;
use crate::flash::executor::{BootTarget, FlashExecutor};
use crate::flash::transport::FlashTransport;

use super::types::FastbootMode;
use super::types::GsiEvent;

pub(super) fn detect_fastboot_mode(vars: &HashMap<String, String>) -> FastbootMode {
    match vars.get("is-userspace").map(String::as_str) {
        Some("yes" | "true" | "1" | "on") => FastbootMode::Fastbootd,
        _ => FastbootMode::Bootloader,
    }
}

/// Compute the size (in bytes) needed for a `product_gsi` partition when the
/// GSI image exceeds the system partition.  Returns 0 if no overflow.
/// The overflow is rounded up to the next megabyte to give mke2fs headroom.
pub(super) const fn product_gsi_overflow_size(system_partition_size: u64, gsi_expanded_size: u64) -> u64 {
    if gsi_expanded_size > system_partition_size {
        let overflow = gsi_expanded_size - system_partition_size;
        overflow.next_multiple_of(1024 * 1024)
    } else {
        0
    }
}

/// Read the expanded (unsparsed) size of a GSI image.
///
/// For raw images this is the file size.  For Android sparse images it reads
/// the header and returns `total_blocks × block_size` — the actual size the
/// image occupies on the partition after fastbootd expands it.
pub(super) async fn read_gsi_expanded_size(image: &std::path::Path) -> crate::gsi::error::Result<u64> {
    let is_sparse = crate::flash::sparse::is_sparse_image(image)
        .await
        .map_err(|e| crate::gsi::error::GsiError::ImageCheck(format!("failed to check if {} is a sparse image: {e}", image.display())))?;
    if is_sparse {
        let mut file = tokio::fs::File::open(image).await?;
        let mut header_bytes = [0u8; FILE_HEADER_BYTES_LEN];
        file.read_exact(&mut header_bytes).await?;
        let header = FileHeader::from_bytes(&header_bytes)
            .map_err(|_| crate::gsi::error::GsiError::SparseHeader("failed to parse sparse file header".into()))?;
        Ok(u64::from(header.blocks) * u64::from(header.block_size))
    } else {
        Ok(tokio::fs::metadata(image).await?.len())
    }
}

/// Query partition size directly from the device via `getvar`.
pub(super) async fn query_partition_size<T: FlashTransport>(executor: &mut FlashExecutor<T>, name: &str) -> Option<u64> {
    let resp = executor
        .get_var(&format!("partition-size:{name}"))
        .await
        .ok()?;
    let s = resp.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u64::from_str_radix(s, 16).ok()
}

/// Resolve system partition name and size by probing the device directly.
/// Tries `system_{slot}` first, then bare `system`.
pub(super) async fn resolve_system_partition<T: FlashTransport>(executor: &mut FlashExecutor<T>) -> crate::gsi::error::Result<(String, u64)> {
    let slot = executor
        .get_var("current-slot")
        .await
        .unwrap_or_else(|_| "a".into());
    let candidates = [format!("system_{slot}"), "system".to_string()];
    for name in &candidates {
        if let Some(size) = query_partition_size(executor, name).await {
            if size > 0 {
                return Ok((name.clone(), size));
            }
        }
    }
    Err(crate::gsi::error::GsiError::PartitionResolution(format!("neither system_{slot} nor system found in device partitions")))
}

/// Reboot the device to a target fastboot mode, wait for it to re-appear,
/// re-fetch device variables, and verify the mode. Retries once on failure.
///
/// Short-circuits if already in the target mode.
///
/// # Errors
///
/// Returns an error if the reboot, wait, or mode verification fails.
pub(super) async fn transition_mode(
    mut executor: FlashExecutor,
    target: FastbootMode,
    report: &mut impl FnMut(GsiEvent),
) -> crate::gsi::error::Result<FlashExecutor> {
    if detect_fastboot_mode(executor.device_vars()) == target {
        report(GsiEvent::ModeReady(target));
        return Ok(executor);
    }

    let boot_target = match target {
        FastbootMode::Fastbootd => BootTarget::Fastboot,
        FastbootMode::Bootloader => BootTarget::Bootloader,
    };

    for attempt in 0..=1 {

        let new_exec = executor
            .reboot_and_wait(boot_target)
            .await?;

        if detect_fastboot_mode(new_exec.device_vars()) == target {
            report(GsiEvent::ModeReady(target));
            return Ok(new_exec);
        }

        if attempt == 0 {
            executor = FlashExecutor::wait_for_device(Duration::from_secs(30)).await?;
        } else {
            return Err(crate::gsi::error::GsiError::PartitionResolution(format!("device did not switch to {} after retry", target.as_str())));
        }
    }

    unreachable!()
}
