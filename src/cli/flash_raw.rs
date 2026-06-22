use std::path::PathBuf;
use anyhow::{bail, Context, Result};
use tracing::{error, info, warn};

use crate::cli::init_stderr_logging;
use crate::flash::FlashExecutor;

pub async fn run(
    partition: &str,
    image: &PathBuf,
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
        let current = executor.device_vars().get("current-slot").map(|s| s.as_str());
        match current {
            Some("a") | Some("b") => {
                vec![format!("{partition}_{}", current.unwrap())]
            }
            _ => {
                warn!("device has no current-slot variable; flashing to bare partition name");
                vec![partition.to_string()]
            }
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
