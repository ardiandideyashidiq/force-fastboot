use std::collections::HashMap;
use std::time::Duration;

use miette::{bail, Context, Result};
use tracing::debug;

use pawflash_core::flash::executor::FlashExecutor;
use pawflash_core::flash::simulate::SimulatedTransport;
use pawflash_core::output;

/// Flash the vendored empty vbmeta image to both slots with AVB flags=3.
/// This disables dm-verity and AVB verification.
///
/// When `simulate` is true, uses [`SimulatedTransport`] with
/// `is-userspace: no` (bootloader mode).
///
/// # Errors
///
/// Returns an error if the device is not reachable or flashing fails.
pub async fn run(simulate: bool) -> Result<()> {
    debug!("disable-vbmeta started");

    if simulate {
        output::status::heading("⚠ SIMULATED MODE — no device will be touched");
        let vars = HashMap::from([
            ("max-download-size".into(), "0x10000000".into()),
            ("product".into(), "SIM_DEVICE".into()),
            ("serialno".into(), "SIM000001".into()),
            ("version".into(), "0.5".into()),
            ("current-slot".into(), "a".into()),
            ("is-userspace".into(), "no".into()),
        ]);
        let transport = SimulatedTransport::new(vars);
        let mut executor = FlashExecutor::new(transport, HashMap::new());
        let resp = executor.flash_empty_vbmeta().await
            .context("simulated disable-vbmeta failed")?;
        output::status::ok("OKAY", resp);
        debug!("simulated disable-vbmeta completed");
        return Ok(());
    }

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device (60s timeout)...",
        FlashExecutor::wait_for_device(Duration::from_secs(60)),
    )
    .await?;

    // vbmeta partitions are only accessible in bootloader mode.
    let is_userspace = executor.get_var("is-userspace").await.unwrap_or_default();
    if is_userspace == "yes" || is_userspace == "true" {
        bail!(
            "device is in fastbootd mode; vbmeta can only be flashed in bootloader mode.\n\
             Run 'pawflash device reboot bootloader' first."
        );
    }

    output::spinner::run_with_spinner(
        "Disabling vbmeta verification...",
        async {
            executor.flash_empty_vbmeta().await
        },
    )
    .await
    .context("failed to flash empty vbmeta")?;

    debug!("disable-vbmeta completed");
    Ok(())
}
