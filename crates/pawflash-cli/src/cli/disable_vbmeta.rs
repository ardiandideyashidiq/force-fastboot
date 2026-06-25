use anyhow::{Context, Result};
use tracing::debug;

use pawflash_core::flash::executor::FlashExecutor;
use pawflash_core::output;

/// Flash the vendored empty vbmeta image to both slots with AVB flags=3.
/// This disables dm-verity and AVB verification.
///
/// # Errors
///
/// Returns an error if the device is not reachable or flashing fails.
pub async fn run() -> Result<()> {
    debug!("disable-vbmeta started");

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device...",
        FlashExecutor::connect(),
    )
    .await?;

    // vbmeta partitions are only accessible in bootloader mode.
    let is_userspace = executor.get_var("is-userspace").await.unwrap_or_default();
    if is_userspace == "yes" || is_userspace == "true" {
        anyhow::bail!(
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
