use anyhow::Result;
use tracing::debug;

use crate::flash::FlashExecutor;
use crate::output;

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

    output::spinner::run_with_spinner(
        "Disabling vbmeta verification...",
        async {
            executor.flash_empty_vbmeta().await
        },
    )
    .await?;

    debug!("disable-vbmeta completed");
    Ok(())
}
