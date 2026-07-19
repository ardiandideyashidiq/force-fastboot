use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tracing::{debug, info, warn};

use pawflash_core::flash::executor::FlashExecutor;
use pawflash_core::flash::simulate::SimulatedTransport;
use pawflash_core::output;

pub(super) async fn run_raw_image(
    partition: &str,
    image: &Path,
    slot: Option<String>,
    both: bool,
    simulate: bool,
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

    if simulate {
        output::status::heading("⚠ SIMULATED MODE — no device will be touched");
        let vars = HashMap::from([
            ("max-download-size".into(), "0x10000000".into()),
            ("current-slot".into(), "a".into()),
            ("product".into(), "SIM_DEVICE".into()),
            ("serialno".into(), "SIM000001".into()),
            ("version".into(), "0.5".into()),
            ("is-userspace".into(), "yes".into()),
        ]);
        let transport = SimulatedTransport::new(vars.clone());
        let mut executor = FlashExecutor::new(transport, vars);
        return do_raw_flash(&mut executor, partition, &image, slot, both).await;
    }

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device (60s timeout)...",
        FlashExecutor::wait_for_device(Duration::from_secs(60)),
    )
    .await?;

    do_raw_flash(&mut executor, partition, &image, slot, both).await
}

/// Shared raw flash logic used by both real and simulated paths.
async fn do_raw_flash<T: pawflash_core::flash::transport::FlashTransport>(
    executor: &mut FlashExecutor<T>,
    partition: &str,
    image: &Path,
    slot: Option<String>,
    both: bool,
) -> Result<()> {
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
        match executor.flash_raw_image(target, image).await {
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
