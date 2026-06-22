use std::path::Path;
use anyhow::{bail, Context, Result};
use tracing::{error, info, warn};

use crate::cli::init_stderr_logging;
use crate::flash::FlashExecutor;

/// Flash a raw image to a partition (with A/B slot support).
///
/// # Errors
///
/// Returns an error if the image does not exist, the partition table
/// cannot be read, or the device is not reachable.
///
/// # Panics
///
/// Panics if the device variable `current-slot` contains an unexpected
/// value when neither `--slot` nor `--both` is provided.
pub async fn run(
    partition: &str,
    image: &Path,
    slot: Option<String>,
    both: bool,
    verbose: bool,
) -> Result<()> {
    let level = if verbose { "trace" } else { "info" };
    init_stderr_logging(level);

    // Validate mutually exclusive flags
    if both && slot.is_some() {
        bail!("--both and --slot are mutually exclusive");
    }
    if let Some(ref s) = slot {
        if s != "a" && s != "b" {
            bail!("--slot must be 'a' or 'b', got '{s}'");
        }
    }

    // Fail fast if image does not exist
    if !image.exists() {
        bail!("image not found: {}", image.display());
    }
    let image = image.canonicalize().context("failed to resolve image path")?;

    info!(
        partition,
        image = %image.display(),
        ?slot,
        both,
        "connecting to fastboot device",
    );

    let mut executor = FlashExecutor::connect().await?;

    // Resolve target partition names
    let targets: Vec<String> = if both {
        vec![format!("{partition}_a"), format!("{partition}_b")]
    } else if let Some(s) = slot {
        vec![format!("{partition}_{s}")]
    } else {
        let current = executor.device_vars().get("current-slot").map(String::as_str);
        if let Some(slot) = current {
            vec![format!("{partition}_{slot}")]
        } else {
            warn!("device has no current-slot variable; flashing to bare partition name");
            vec![partition.to_string()]
        }
    };

    info!(?targets, partition, "flashing");

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for target in &targets {
        match executor.flash_raw_image(target, &image).await {
            Ok(()) => {
                info!(partition = %target, "flash-raw successful");
                succeeded += 1;
            }
            Err(e) => {
                error!(partition = %target, error = %e, "flash-raw failed");
                failed += 1;
            }
        }
    }

    info!(succeeded, failed, "flash-raw complete");

    if failed > 0 && succeeded == 0 {
        bail!("flash-raw failed for all targets");
    }

    Ok(())
}
