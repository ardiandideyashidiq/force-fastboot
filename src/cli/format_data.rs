use anyhow::Result;
use tracing::{debug, info};

use crate::flash::FlashExecutor;
use crate::format::generator;

/// Erase and format userdata, cache, and metadata.
///
/// # Errors
///
/// Returns an error if the device is not reachable or formatting fails.
pub async fn run(fs_options: Vec<String>) -> Result<()> {
    let fs_options = generator::parse_fs_options(&fs_options);

    debug!(?fs_options, "format-data started");
    info!(?fs_options, "connecting to fastboot device");
    let mut executor = FlashExecutor::connect().await?;

    let result = executor.format_data(fs_options).await;
    info!(outcomes = result.outcomes.len(), "format-data done");

    Ok(())
}
