use std::path::Path;

use anyhow::{bail, Context, Result};
use tracing::{debug, info, warn};

use pawflash_core::flash::FlashExecutor;
use pawflash_core::output;

pub(super) async fn run_raw_image(
    partition: &str,
    image: &Path,
    slot: Option<String>,
    both: bool,
) -> Result<()> {
    if both && slot.is_some() {
        bail!("--both and --slot are mutually exclusive");
    }
    if let Some(ref s) = slot {
        if s != "a" && s != "b" {
            bail!("--slot must be 'a' or 'b', got '{s}'");
        }
    }

    if !image.exists() {
        bail!("image not found: {}", image.display());
    }
    let image = image.canonicalize().context("failed to resolve image path")?;

    debug!(%partition, image = %image.display(), ?slot, both, "raw image flash requested");

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device...",
        FlashExecutor::connect(),
    )
    .await?;

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
            Ok(resp) => {
                info!(partition = %target, response = resp, "flash successful");
                succeeded += 1;
            }
            Err(e) => {
                tracing::error!(partition = %target, error = %e, "flash failed");
                failed += 1;
            }
        }
    }

    info!(succeeded, failed, "flash complete");

    if failed > 0 && succeeded == 0 {
        bail!("flash-raw failed for all targets");
    }

    Ok(())
}
