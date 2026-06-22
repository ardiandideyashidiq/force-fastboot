use anyhow::Result;
use tracing::{debug, info};

use crate::flash::FlashExecutor;

/// Flash the vendored empty vbmeta image to both slots with AVB flags=3.
/// This disables dm-verity and AVB verification.
///
/// # Errors
///
/// Returns an error if the device is not reachable or flashing fails.
pub async fn run() -> Result<()> {
    debug!("disable-vbmeta started");

    info!("connecting to fastboot device");
    let mut executor = FlashExecutor::connect().await?;

    executor.flash_empty_vbmeta().await?;

    info!("vbmeta disabled — dm-verity and AVB verification are now off");
    debug!("disable-vbmeta completed");
    Ok(())
}
