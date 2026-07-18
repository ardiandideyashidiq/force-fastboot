use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tempfile::TempDir;
use tracing::debug;

use crate::flash::executor::FlashExecutor;

use super::types::{GsiEvent, GsiStep};
use super::flash::check_cancelled;

const PRODUCT_GSI_BLOCK_SIZE: u64 = 4_096;
const PRODUCT_GSI_UUID: &str = "cdd462dd-8dd0-4006-8a5a-94e5a70c2bc3";
const MINIMAL_PRODUCT_GSI_SIZE: u64 = 64 * 1024 * 1024;

pub(super) fn generate_product_gsi_image(tools_dir: &Path) -> crate::gsi::error::Result<(TempDir, std::path::PathBuf)> {
    let dir = TempDir::new()?;
    let output = dir.path().join("product_gsi.img");
    let blocks = MINIMAL_PRODUCT_GSI_SIZE / PRODUCT_GSI_BLOCK_SIZE;

    let mke2fs = tools_dir.join("mke2fs");
    let conf = tools_dir.join("mke2fs.conf");

    let status = std::process::Command::new(&mke2fs)
        .env("MKE2FS_CONFIG", &conf)
        .arg("-F")
        .arg("-t")
        .arg("ext4")
        .arg("-b")
        .arg(PRODUCT_GSI_BLOCK_SIZE.to_string())
        .arg("-L")
        .arg("product")
        .arg("-U")
        .arg(PRODUCT_GSI_UUID)
        .arg(&output)
        .arg(blocks.to_string())
        .status()?;

    if !status.success() {
        return Err(crate::gsi::error::GsiError::FormatTools(format!("mke2fs for product_gsi failed with status {status}")));
    }

    Ok((dir, output))
}

/// Flash the system GSI and (if needed) product GSI, both in fastbootd
/// mode where logical partitions are accessible.
///
/// When the GSI overflows the system partition, the space is taken from
/// product: product is shrunk first to free super space, then system is
/// expanded to fit the GSI. Both resizes are explicit because sparse-split
/// flashing writes past the original partition boundary — the device
/// rejects writes beyond the current size.
pub(super) async fn flash_system_and_product(
    executor: &mut FlashExecutor,
    image: &Path,
    overflow: (u64, u64),
    system_partition: &str,
    tools_root: &Path,
    cancel_token: Option<&Arc<AtomicBool>>,
    report: &mut impl FnMut(GsiEvent),
) -> crate::gsi::error::Result<()> {
    check_cancelled(cancel_token)?;
    report(GsiEvent::Step(GsiStep::CheckingProductGsiFallback));

    let file_size = tokio::fs::metadata(image).await?.len();

    if overflow.1 > 0 {
        let product_partition = system_partition.replace("system", "product");

        // Phase 1: shrink product to free super space, then expand system
        let is_product_logical = executor.is_logical(&product_partition).await.unwrap_or(false);
        let is_system_logical = executor.is_logical(system_partition).await.unwrap_or(false);

        check_cancelled(cancel_token)?;
        if is_product_logical {
            debug!(partition = %product_partition, size = MINIMAL_PRODUCT_GSI_SIZE, "shrinking product to free super space");
            executor.resize_logical_partition(&product_partition, MINIMAL_PRODUCT_GSI_SIZE).await?;
        }
        if is_system_logical {
            debug!(partition = %system_partition, size = overflow.0, "expanding system to GSI size");
            executor.resize_logical_partition(system_partition, overflow.0).await?;
        }

        // Phase 2: generate minimal product_gsi and flash both partitions
        check_cancelled(cancel_token)?;
        report(GsiEvent::Step(GsiStep::GeneratingProductGsiImage));
        let (_tmpdir, product_image) = generate_product_gsi_image(tools_root)?;

        check_cancelled(cancel_token)?;
        report(GsiEvent::Step(GsiStep::FlashingProductGsi));
        report(GsiEvent::Flashing {
            partition: product_partition.clone(),
            size_bytes: MINIMAL_PRODUCT_GSI_SIZE,
        });
        executor.flash_raw_image(&product_partition, &product_image).await?;

        check_cancelled(cancel_token)?;
        report(GsiEvent::Step(GsiStep::FlashingSystemGsi));
        report(GsiEvent::Flashing {
            partition: system_partition.to_string(),
            size_bytes: file_size,
        });
        executor.flash_raw_image(system_partition, image).await?;
    } else {
        check_cancelled(cancel_token)?;
        report(GsiEvent::Step(GsiStep::ProductGsiFallbackNotNeeded));
        report(GsiEvent::Step(GsiStep::FlashingSystemGsi));
        report(GsiEvent::Flashing {
            partition: system_partition.to_string(),
            size_bytes: file_size,
        });
        debug!(partition = %system_partition, "flashing GSI system image");
        executor.flash_raw_image(system_partition, image).await?;
    }

    Ok(())
}
