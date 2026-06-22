use std::path::Path;

use anyhow::Result;
use tracing::info;

use crate::flash::FlashExecutor;
use crate::gsi::GsiEvent;
use crate::output;

/// Handler for `pawflash flash gsi <image>`.
///
/// # Errors
///
/// Returns an error if the image is missing, the device is unreachable,
/// or any stage of the GSI flash workflow fails.
pub async fn run(image: &Path) -> Result<()> {
    if !image.exists() {
        anyhow::bail!("GSI image not found: {}", image.display());
    }
    let image = image.canonicalize()?;

    let executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device...",
        FlashExecutor::connect(),
    )
    .await?;

    info!(image = %image.display(), "starting GSI flash");

    let report = |event: GsiEvent| log_gsi_event(&event);

    let outcome = crate::gsi::execute_gsi_flash(executor, &image, report).await?;

    info!(
        flash_count = outcome.summary.flash_count,
        wipe_count = outcome.summary.wipe_count,
        "GSI flash complete",
    );

    Ok(())
}

fn log_gsi_event(event: &GsiEvent) {
    match event {
        GsiEvent::Step(step) => info!("[gsi] {}", step.as_str()),
        GsiEvent::ModeDetected(mode) => info!("[gsi] detected mode: {}", mode.as_str()),
        GsiEvent::ModeReady(mode) => info!("[gsi] ready in mode: {}", mode.as_str()),
        GsiEvent::ResolvedPartition { base, partition, size_bytes } => {
            info!("[gsi] resolved {base} -> {partition} ({size_bytes} bytes)");
        }
        GsiEvent::Flashing { partition, size_bytes } => {
            info!("[gsi] flashing {partition} ({size_bytes} bytes)");
        }
        GsiEvent::Wiping { partition } => {
            info!("[gsi] wiping {partition}");
        }
        GsiEvent::PartitionSkipped { partition, reason } => {
            info!("[gsi] skipped {partition}: {reason}");
        }
    }
}
