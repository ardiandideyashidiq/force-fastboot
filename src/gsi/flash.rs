use std::collections::HashMap;
use std::path::Path;

use android_sparse_image::{FileHeader, FILE_HEADER_BYTES_LEN};
use anyhow::{bail, Context, Result};
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::time::{sleep, Duration};
use tracing::debug;

use crate::flash::executor::{BootTarget, FlashExecutor};
use crate::format::generator;

use super::types::{FastbootMode, GsiEvent, GsiFlashOutcome, GsiFlashSummary, GsiStep};

const PRODUCT_GSI_SIZE: u64 = 335_872;
const PRODUCT_GSI_BLOCKS: u64 = 82;
const PRODUCT_GSI_BLOCK_SIZE: u64 = 4_096;
const PRODUCT_GSI_UUID: &str = "cdd462dd-8dd0-4006-8a5a-94e5a70c2bc3";

fn detect_fastboot_mode(vars: &HashMap<String, String>) -> FastbootMode {
    match vars.get("is-userspace").map(String::as_str) {
        Some("yes" | "true" | "1" | "on") => FastbootMode::Fastbootd,
        _ => FastbootMode::Bootloader,
    }
}

const fn should_flash_product_gsi(system_partition_size: u64, gsi_expanded_size: u64) -> bool {
    gsi_expanded_size > system_partition_size
}

/// Read the expanded (unsparsed) size of a GSI image.
///
/// For raw images this is the file size.  For Android sparse images it reads
/// the header and returns `total_blocks × block_size` — the actual size the
/// image occupies on the partition after fastbootd expands it.
async fn read_gsi_expanded_size(image: &Path) -> Result<u64> {
    if crate::flash::sparse::is_sparse_image(image).await.unwrap_or(false) {
        let mut file = tokio::fs::File::open(image).await?;
        let mut header_bytes = [0u8; FILE_HEADER_BYTES_LEN];
        file.read_exact(&mut header_bytes).await?;
        let header = FileHeader::from_bytes(&header_bytes)
            .map_err(|_| anyhow::anyhow!("failed to parse sparse file header"))?;
        Ok(u64::from(header.blocks) * u64::from(header.block_size))
    } else {
        Ok(tokio::fs::metadata(image).await?.len())
    }
}

/// Query partition size directly from the device via `getvar`.
async fn query_partition_size(executor: &mut FlashExecutor, name: &str) -> Option<u64> {
    let resp = executor
        .get_var(&format!("partition-size:{name}"))
        .await
        .ok()?;
    let s = resp.trim().strip_prefix("0x").unwrap_or(resp.trim());
    u64::from_str_radix(s, 16).ok()
}

/// Resolve system partition name and size by probing the device directly.
/// Tries `system_{slot}` first, then bare `system`.
async fn resolve_system_partition(executor: &mut FlashExecutor) -> Result<(String, u64)> {
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
    bail!("neither system_{slot} nor system found in device partitions");
}

fn generate_product_gsi_image(tools_dir: &Path) -> Result<(TempDir, std::path::PathBuf)> {
    let dir = TempDir::new()?;
    let output = dir.path().join("product_gsi.img");

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
        .arg(PRODUCT_GSI_BLOCKS.to_string())
        .status()
        .with_context(|| "failed to spawn mke2fs for product_gsi")?;

    if !status.success() {
        bail!("mke2fs for product_gsi failed with status {status}");
    }

    Ok((dir, output))
}

async fn flash_gsi_system(
    executor: &mut FlashExecutor,
    image: &Path,
    system_partition: &str,
    gsi_size: u64,
) -> Result<()> {
    debug!(partition = %system_partition, gsi_size, "flashing GSI system image");

    let is_logical = executor.is_logical(system_partition).await.unwrap_or(false);
    if is_logical {
        debug!(partition = %system_partition, "resizing logical partition");
        executor
            .resize_logical_partition(system_partition, gsi_size)
            .await?;
    }

    executor.flash_raw_image(system_partition, image).await?;

    Ok(())
}

async fn flash_product_gsi(
    executor: &mut FlashExecutor,
    product_image: &Path,
    product_partition: &str,
) -> Result<()> {
    debug!(partition = %product_partition, "flashing product GSI");

    let is_logical = executor.is_logical(product_partition).await.unwrap_or(false);
    if is_logical {
        executor
            .resize_logical_partition(product_partition, PRODUCT_GSI_SIZE)
            .await?;
    }

    executor.flash_raw_image(product_partition, product_image).await?;

    Ok(())
}

/// Reboot the device to a target fastboot mode, wait for it to re-appear,
/// re-fetch device variables, and verify the mode. Retries once on failure.
///
/// Short-circuits if already in the target mode.
///
/// # Errors
///
/// Returns an error if the reboot, wait, or mode verification fails.
async fn transition_mode(
    mut executor: FlashExecutor,
    target: FastbootMode,
    report: &mut impl FnMut(GsiEvent),
) -> Result<FlashExecutor> {
    if detect_fastboot_mode(executor.device_vars()) == target {
        report(GsiEvent::ModeReady(target));
        return Ok(executor);
    }

    let step = match target {
        FastbootMode::Fastbootd => GsiStep::RebootingToFastbootd,
        FastbootMode::Bootloader => GsiStep::RebootingToBootloader,
    };
    let boot_target = match target {
        FastbootMode::Fastbootd => BootTarget::Fastboot,
        FastbootMode::Bootloader => BootTarget::Bootloader,
    };

    for attempt in 0..=1 {
        report(GsiEvent::Step(step));
        if target == FastbootMode::Fastbootd {
            report(GsiEvent::Step(GsiStep::WaitingForFastbootd));
        }

        let new_exec = executor
            .reboot_and_wait(boot_target)
            .await
            .with_context(|| format!("failed to transition to {}", target.as_str()))?;

        if detect_fastboot_mode(new_exec.device_vars()) == target {
            report(GsiEvent::ModeReady(target));
            return Ok(new_exec);
        }

        if attempt == 0 {
            sleep(Duration::from_secs(2)).await;
            executor = FlashExecutor::connect().await?;
        } else {
            bail!("device did not switch to {} after retry", target.as_str());
        }
    }

    unreachable!()
}

// ── Top-level entry point ────────────────────────────────────────────

/// Execute the full GSI flash workflow.
///
/// Takes ownership of the executor, runs the state machine, and returns the
/// outcome. The workflow handles both bootloader-start and fastbootd-start
/// scenarios, including vbmeta disable, userdata wipe, mode transitions,
/// and the `product_gsi` fallback.
///
/// # Errors
///
/// Returns an error if image validation, mode transitions, partition
/// resolution, vbmeta flash, userdata wipe, or GSI flash fails.
pub async fn execute_gsi_flash(
    executor: FlashExecutor,
    image: &Path,
    mut report: impl FnMut(GsiEvent),
) -> Result<GsiFlashOutcome> {
    let vars = executor.device_vars().clone();
    let mode = detect_fastboot_mode(&vars);

    let gsi_expanded_size = read_gsi_expanded_size(image)
        .await
        .with_context(|| format!("cannot determine expanded size of {}", image.display()))?;

    report(GsiEvent::ModeDetected(mode));

    let tools_dir = extract_tools()?;
    let tools_root = tools_dir.path().to_path_buf();

    let outcome = match mode {
        FastbootMode::Bootloader => {
            gsi_from_bootloader(executor, image, gsi_expanded_size, &tools_root, &mut report).await
        }
        FastbootMode::Fastbootd => {
            gsi_from_fastbootd(executor, image, gsi_expanded_size, &tools_root, &mut report).await
        }
    };
    drop(tools_dir);
    outcome
}

fn extract_tools() -> Result<TempDir> {
    let (dir, _root) = generator::extract_format_tools()
        .map_err(|e| anyhow::anyhow!("failed to extract format tools: {e}"))?;
    Ok(dir)
}

// ── Entry-point branches ─────────────────────────────────────────────

async fn gsi_from_bootloader(
    mut executor: FlashExecutor,
    image: &Path,
    gsi_expanded_size: u64,
    tools_root: &Path,
    report: &mut impl FnMut(GsiEvent),
) -> Result<GsiFlashOutcome> {
    report(GsiEvent::Step(GsiStep::StartingBootloaderPhase));

    report(GsiEvent::Step(GsiStep::PreparingVbmetaFlash));
    report(GsiEvent::Step(GsiStep::FlashingVbmeta));
    executor.flash_empty_vbmeta().await?;

    report(GsiEvent::Step(GsiStep::WipingUserdata));
    executor.format_data(0).await;

    executor = transition_mode(executor, FastbootMode::Fastbootd, report).await?;

    // Resolve system partition from fastbootd vars.
    // Logical partitions (inside super) are only visible in fastbootd.
    let (system_partition, system_size) = resolve_system_partition(&mut executor).await?;
    report(GsiEvent::ResolvedPartition {
        base: "system",
        partition: system_partition.clone(),
        size_bytes: system_size,
    });

    report(GsiEvent::Step(GsiStep::CheckingProductGsiFallback));
    let needs_product_gsi = should_flash_product_gsi(system_size, gsi_expanded_size);

    flash_in_fastbootd(
        &mut executor,
        image,
        gsi_expanded_size,
        &system_partition,
        needs_product_gsi,
        tools_root,
        report,
    )
    .await?;

    report(GsiEvent::Step(GsiStep::GsiFlowComplete));
    Ok(GsiFlashOutcome {
        summary: GsiFlashSummary::default(),
    })
}

async fn gsi_from_fastbootd(
    mut executor: FlashExecutor,
    image: &Path,
    gsi_expanded_size: u64,
    tools_root: &Path,
    report: &mut impl FnMut(GsiEvent),
) -> Result<GsiFlashOutcome> {
    report(GsiEvent::Step(GsiStep::StartingFastbootdPhase));

    // Resolve system partition directly from the device.
    let (system_partition, system_size) = resolve_system_partition(&mut executor).await?;
    report(GsiEvent::ResolvedPartition {
        base: "system",
        partition: system_partition.clone(),
        size_bytes: system_size,
    });

    report(GsiEvent::Step(GsiStep::CheckingProductGsiFallback));
    let needs_product_gsi = should_flash_product_gsi(system_size, gsi_expanded_size);

    flash_in_fastbootd(
        &mut executor,
        image,
        gsi_expanded_size,
        &system_partition,
        needs_product_gsi,
        tools_root,
        report,
    )
    .await?;

    executor = transition_mode(executor, FastbootMode::Bootloader, report).await?;

    report(GsiEvent::Step(GsiStep::PreparingVbmetaFlash));
    report(GsiEvent::Step(GsiStep::FlashingVbmeta));
    executor.flash_empty_vbmeta().await?;

    report(GsiEvent::Step(GsiStep::WipingUserdata));
    executor.format_data(0).await;

    transition_mode(executor, FastbootMode::Fastbootd, report).await?;

    report(GsiEvent::Step(GsiStep::GsiFlowComplete));
    Ok(GsiFlashOutcome {
        summary: GsiFlashSummary::default(),
    })
}

// ── Shared helpers ───────────────────────────────────────────────────

async fn flash_in_fastbootd(
    executor: &mut FlashExecutor,
    image: &Path,
    gsi_expanded_size: u64,
    system_partition: &str,
    needs_product_gsi: bool,
    tools_root: &Path,
    report: &mut impl FnMut(GsiEvent),
) -> Result<()> {
    if needs_product_gsi {
        report(GsiEvent::Step(GsiStep::GeneratingProductGsiImage));
        let (_tmpdir, product_image) = generate_product_gsi_image(tools_root)?;

        let product_partition = system_partition.replace("system", "product");

        report(GsiEvent::Step(GsiStep::FlashingProductGsi));
        report(GsiEvent::Flashing {
            partition: product_partition.clone(),
            size_bytes: PRODUCT_GSI_SIZE,
        });
        flash_product_gsi(executor, &product_image, &product_partition).await?;
    } else {
        report(GsiEvent::Step(GsiStep::ProductGsiFallbackNotNeeded));
    }

    let file_size = tokio::fs::metadata(image).await?.len();
    report(GsiEvent::Step(GsiStep::FlashingSystemGsi));
    report(GsiEvent::Flashing {
        partition: system_partition.to_string(),
        size_bytes: file_size,
    });
    flash_gsi_system(executor, image, system_partition, gsi_expanded_size).await?;

    Ok(())
}
