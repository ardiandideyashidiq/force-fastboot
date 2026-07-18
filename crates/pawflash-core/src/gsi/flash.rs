use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use android_sparse_image::{FileHeader, FILE_HEADER_BYTES_LEN};
use crate::gsi::error::{GsiError, Result};
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::time::Duration;
use tracing::{debug, info};

use crate::flash::executor::{BootTarget, FlashExecutor};
use crate::flash::transport::FlashTransport;
use crate::format::generator;

use super::types::{FastbootMode, GsiEvent, GsiFlashOutcome, GsiFlashSummary, GsiStep};

const PRODUCT_GSI_BLOCK_SIZE: u64 = 4_096;
const PRODUCT_GSI_UUID: &str = "cdd462dd-8dd0-4006-8a5a-94e5a70c2bc3";
const MINIMAL_PRODUCT_GSI_SIZE: u64 = 64 * 1024 * 1024;

fn detect_fastboot_mode(vars: &HashMap<String, String>) -> FastbootMode {
    match vars.get("is-userspace").map(String::as_str) {
        Some("yes" | "true" | "1" | "on") => FastbootMode::Fastbootd,
        _ => FastbootMode::Bootloader,
    }
}

/// Compute the size (in bytes) needed for a `product_gsi` partition when the
/// GSI image exceeds the system partition.  Returns 0 if no overflow.
/// The overflow is rounded up to the next megabyte to give mke2fs headroom.
const fn product_gsi_overflow_size(system_partition_size: u64, gsi_expanded_size: u64) -> u64 {
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
async fn read_gsi_expanded_size(image: &Path) -> Result<u64> {
    let is_sparse = crate::flash::sparse::is_sparse_image(image)
        .await
        .map_err(|e| GsiError::ImageCheck(format!("failed to check if {} is a sparse image: {e}", image.display())))?;
    if is_sparse {
        let mut file = tokio::fs::File::open(image).await?;
        let mut header_bytes = [0u8; FILE_HEADER_BYTES_LEN];
        file.read_exact(&mut header_bytes).await?;
        let header = FileHeader::from_bytes(&header_bytes)
            .map_err(|_| GsiError::SparseHeader("failed to parse sparse file header".into()))?;
        Ok(u64::from(header.blocks) * u64::from(header.block_size))
    } else {
        Ok(tokio::fs::metadata(image).await?.len())
    }
}

/// Query partition size directly from the device via `getvar`.
async fn query_partition_size<T: FlashTransport>(executor: &mut FlashExecutor<T>, name: &str) -> Option<u64> {
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
async fn resolve_system_partition<T: FlashTransport>(executor: &mut FlashExecutor<T>) -> Result<(String, u64)> {
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
    Err(GsiError::PartitionResolution(format!("neither system_{slot} nor system found in device partitions")))
}

fn generate_product_gsi_image(tools_dir: &Path) -> Result<(TempDir, std::path::PathBuf)> {
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
        return Err(GsiError::FormatTools(format!("mke2fs for product_gsi failed with status {status}")));
    }

    Ok((dir, output))
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
            return Err(GsiError::PartitionResolution(format!("device did not switch to {} after retry", target.as_str())));
        }
    }

    unreachable!()
}

// ── GSI stage machine ────────────────────────────────────────────────

struct GsiCounters {
    flash_count: AtomicU64,
    wipe_count: AtomicU64,
    skipped_count: AtomicU64,
    total_bytes: AtomicU64,
}

impl GsiCounters {
    const fn new() -> Self {
        Self {
            flash_count: AtomicU64::new(0),
            wipe_count: AtomicU64::new(0),
            skipped_count: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
        }
    }
}

fn make_reporter<'a>(
    counters: &'a GsiCounters,
    inner: &'a mut impl FnMut(GsiEvent),
) -> impl FnMut(GsiEvent) + 'a {
    let GsiCounters { ref flash_count, ref wipe_count, ref skipped_count, ref total_bytes } = *counters;
    |event: GsiEvent| {
        match &event {
            GsiEvent::Flashing { size_bytes, .. } => {
                flash_count.fetch_add(1, Ordering::Relaxed);
                total_bytes.fetch_add(*size_bytes, Ordering::Relaxed);
            }
            GsiEvent::Wiping { .. } => {
                wipe_count.fetch_add(1, Ordering::Relaxed);
            }
            GsiEvent::PartitionSkipped { .. } => {
                skipped_count.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        inner(event);
    }
}

enum GsiStage {
    FlashVbmeta,
    WipeUserdata,
    FlashSystem,
}

fn plan_stage_groups(mode: FastbootMode) -> Vec<(FastbootMode, Vec<GsiStage>)> {
    match mode {
        FastbootMode::Bootloader => vec![
            (FastbootMode::Bootloader, vec![GsiStage::FlashVbmeta, GsiStage::WipeUserdata]),
            (FastbootMode::Fastbootd, vec![GsiStage::FlashSystem]),
        ],
        FastbootMode::Fastbootd => vec![
            (FastbootMode::Fastbootd, vec![GsiStage::FlashSystem]),
            (FastbootMode::Bootloader, vec![GsiStage::FlashVbmeta, GsiStage::WipeUserdata]),
        ],
    }
}

fn check_cancelled(cancel_token: Option<&Arc<AtomicBool>>) -> Result<()> {
    if let Some(token) = cancel_token {
        if token.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(GsiError::Cancelled);
        }
    }
    Ok(())
}

/// Execute the full GSI flash workflow.
///
/// Takes ownership of the executor, runs the state machine, and returns the
/// outcome. The workflow handles both bootloader-start and fastbootd-start
/// scenarios with optimal ordering depending on the entry mode:
///
/// - **Bootloader first**: vbmeta disable → wipe → fastbootd → flash
/// - **Fastbootd first**: flash → bootloader → vbmeta disable → wipe
///
/// # Errors
///
/// Returns an error if image validation, mode transitions, partition
/// resolution, vbmeta flash, userdata wipe, or GSI flash fails.
///
/// # Panics
///
/// Panics if the `usize::try_from` conversion of flash/wipe/skipped
/// counters exceeds the target platform's `usize` range.
pub async fn execute_gsi_flash(
    mut executor: FlashExecutor,
    image: &Path,
    clean_test: bool,
    cancel_token: Option<Arc<AtomicBool>>,
    mut user_report: impl FnMut(GsiEvent),
) -> Result<GsiFlashOutcome> {
    let vars = executor.device_vars().clone();
    let mode = detect_fastboot_mode(&vars);

    let gsi_expanded_size = read_gsi_expanded_size(image).await?;

    let counters = GsiCounters::new();
    let mut report = make_reporter(&counters, &mut user_report);

    report(GsiEvent::ModeDetected(mode));

    let (tools_dir, tools_root) = generator::extract_format_tools()
        .map_err(|e| GsiError::FormatTools(format!("{e}")))?;

    for (required_mode, stages) in plan_stage_groups(mode) {
        if detect_fastboot_mode(executor.device_vars()) != required_mode {
            check_cancelled(cancel_token.as_ref())?;
            executor = transition_mode(executor, required_mode, &mut report).await?;
        }

        for stage in &stages {
            check_cancelled(cancel_token.as_ref())?;
            match stage {
                GsiStage::FlashVbmeta => {
                    report(GsiEvent::Step(GsiStep::PreparingVbmetaFlash));
                    report(GsiEvent::Step(GsiStep::FlashingVbmeta));
                    executor.flash_empty_vbmeta().await?;
                }
                GsiStage::WipeUserdata => {
                    report(GsiEvent::Step(GsiStep::WipingUserdata));
                    executor.format_data(0, clean_test, None).await;
                }
                GsiStage::FlashSystem => {
                    let (system_partition, system_size) = resolve_system_partition(&mut executor).await?;
                    report(GsiEvent::ResolvedPartition {
                        base: "system",
                        partition: system_partition.clone(),
                        size_bytes: system_size,
                    });

                    let overflow = (
                        gsi_expanded_size,
                        product_gsi_overflow_size(system_size, gsi_expanded_size),
                    );
                    flash_system_and_product(
                        &mut executor,
                        image,
                        overflow,
                        &system_partition,
                        &tools_root,
                        cancel_token.as_ref(),
                        &mut report,
                    )
                    .await?;
                }
            }
        }
    }

    drop(tools_dir);
    report(GsiEvent::Step(GsiStep::GsiFlowComplete));

    info!("rebooting to system");
    executor.reboot().await?;

    Ok(GsiFlashOutcome {
        summary: GsiFlashSummary {
            flash_count: usize::try_from(counters.flash_count.load(Ordering::Relaxed)).expect("flash count fits usize"),
            wipe_count: usize::try_from(counters.wipe_count.load(Ordering::Relaxed)).expect("wipe count fits usize"),
            skipped_count: usize::try_from(counters.skipped_count.load(Ordering::Relaxed)).expect("skipped count fits usize"),
            total_bytes: counters.total_bytes.load(Ordering::Relaxed),
        },
    })
}

/// Flash the system GSI and (if needed) product GSI, both in fastbootd
/// mode where logical partitions are accessible.
///
/// When the GSI overflows the system partition, the space is taken from
/// product: product is shrunk first to free super space, then system is
/// expanded to fit the GSI. Both resizes are explicit because sparse-split
/// flashing writes past the original partition boundary — the device
/// rejects writes beyond the current size.
async fn flash_system_and_product(
    executor: &mut FlashExecutor,
    image: &Path,
    overflow: (u64, u64),
    system_partition: &str,
    tools_root: &Path,
    cancel_token: Option<&Arc<AtomicBool>>,
    report: &mut impl FnMut(GsiEvent),
) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn product_gsi_overflow_size_zero_when_gsi_fits() {
        assert_eq!(product_gsi_overflow_size(100, 50), 0);
    }

    #[test]
    fn product_gsi_overflow_size_rounded_to_mb() {
        let result = product_gsi_overflow_size(100, 100 + 500);
        assert_eq!(result, 1024 * 1024);
    }

    #[test]
    fn product_gsi_overflow_size_exact_mb_when_exact() {
        assert_eq!(
            product_gsi_overflow_size(100, 100 + 5 * 1024 * 1024),
            5 * 1024 * 1024
        );
    }

    #[test]
    fn detect_fastboot_mode_bootloader_when_no_userspace() {
        let vars = HashMap::new();
        assert_eq!(detect_fastboot_mode(&vars), FastbootMode::Bootloader);
    }

    #[test]
    fn detect_fastboot_mode_fastbootd_when_yes() {
        let mut vars = HashMap::new();
        vars.insert("is-userspace".to_string(), "yes".to_string());
        assert_eq!(detect_fastboot_mode(&vars), FastbootMode::Fastbootd);
    }

    #[test]
    fn detect_fastboot_mode_bootloader_when_no() {
        let mut vars = HashMap::new();
        vars.insert("is-userspace".to_string(), "no".to_string());
        assert_eq!(detect_fastboot_mode(&vars), FastbootMode::Bootloader);
    }

    #[test]
    fn detect_fastboot_mode_accepts_truthy_values() {
        for val in &["true", "1", "on"] {
            let mut vars = HashMap::new();
            vars.insert("is-userspace".to_string(), val.to_string());
            assert_eq!(
                detect_fastboot_mode(&vars),
                FastbootMode::Fastbootd,
                "should accept '{val}' as fastbootd"
            );
        }
    }

    #[tokio::test]
    async fn gsi_flash_rejects_missing_image() {
        let result = read_gsi_expanded_size(Path::new("/nonexistent/gsi.img")).await;
        assert!(result.is_err(), "expected error for missing image");
    }

    #[tokio::test]
    async fn resolve_system_partition_uses_slot() {
        use crate::flash::mock::MockTransport;
        let fb = MockTransport::new().with_get_var("current-slot", "b");
        let mut vars = HashMap::new();
        vars.insert("is-userspace".to_string(), "yes".to_string());
        let mut executor = FlashExecutor::new(fb, vars);
        // Mock does not have partition-size for system_b → should fail
        let result = resolve_system_partition(&mut executor).await;
        assert!(result.is_err(), "expected error when no partition-size configured");
        let err = result.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("system"), "error should mention system partition, got: {msg}");
    }
}
