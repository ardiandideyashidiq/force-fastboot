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
pub async fn run(image: &Path, clean_test: bool) -> Result<()> {
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

    let mut gsi_progress = output::gsi_progress::GsiProgress::new();
    let report = |event: GsiEvent| gsi_progress.report(&event);

    let outcome = crate::gsi::execute_gsi_flash(executor, &image, clean_test, None, report).await?;

    gsi_progress.finish();

    info!(
        flash_count = outcome.summary.flash_count,
        wipe_count = outcome.summary.wipe_count,
        "GSI flash complete",
    );

    Ok(())
}
