use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tracing::info;

use crate::flash::executor::FlashExecutor;
use crate::format::generator;

use super::product::flash_system_and_product;
use super::transition::{
    detect_fastboot_mode, product_gsi_overflow_size, read_gsi_expanded_size,
    resolve_system_partition, transition_mode,
};
use super::types::{FastbootMode, GsiEvent, GsiFlashOutcome, GsiFlashSummary, GsiStep};

// ── GSI stage machine ────────────────────────────────────────────────

struct GsiCounters {
    flash_count: u64,
    wipe_count: u64,
    skipped_count: u64,
    total_bytes: u64,
}

impl GsiCounters {
    const fn new() -> Self {
        Self { flash_count: 0, wipe_count: 0, skipped_count: 0, total_bytes: 0 }
    }
}

fn make_reporter<'a>(
    counters: &'a mut GsiCounters,
    inner: &'a mut impl FnMut(GsiEvent),
) -> impl FnMut(GsiEvent) + 'a {
    |event: GsiEvent| {
        match &event {
            GsiEvent::Flashing { size_bytes, .. } => {
                counters.flash_count += 1;
                counters.total_bytes += size_bytes;
            }
            GsiEvent::Wiping { .. } => counters.wipe_count += 1,
            GsiEvent::PartitionSkipped { .. } => counters.skipped_count += 1,
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

pub(super) fn check_cancelled(cancel_token: Option<&Arc<AtomicBool>>) -> crate::gsi::error::Result<()> {
    if let Some(token) = cancel_token {
        if token.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(crate::gsi::error::GsiError::Cancelled);
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
) -> crate::gsi::error::Result<GsiFlashOutcome> {
    let vars = executor.device_vars().clone();
    let mode = detect_fastboot_mode(&vars);

    let gsi_expanded_size = read_gsi_expanded_size(image).await?;

    let mut counters = GsiCounters::new();
    let mut report = make_reporter(&mut counters, &mut user_report);

    report(GsiEvent::ModeDetected(mode));

    let tools_root = generator::extract_format_tools()
        .as_ref()
        .map_err(|e| crate::gsi::error::GsiError::FormatTools(format!("{e}")))?
        .clone();

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
                    executor.format_data(0, clean_test, None).await?;
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

    report(GsiEvent::Step(GsiStep::GsiFlowComplete));
    drop(report);

    info!("rebooting to system");
    executor.reboot().await?;

    Ok(GsiFlashOutcome {
        summary: GsiFlashSummary {
            flash_count: usize::try_from(counters.flash_count).expect("flash count fits usize"),
            wipe_count: usize::try_from(counters.wipe_count).expect("wipe count fits usize"),
            skipped_count: usize::try_from(counters.skipped_count).expect("skipped count fits usize"),
            total_bytes: counters.total_bytes,
        },
    })
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
