use anyhow::Result;
use tracing::debug;

use crate::flash::FlashExecutor;
use crate::format::generator;
use crate::output;

/// Erase and format userdata, cache, and metadata.
///
/// # Errors
///
/// Returns an error if the device is not reachable or formatting fails.
pub async fn run(fs_options: Vec<String>) -> Result<()> {
    let fs_options = generator::parse_fs_options(&fs_options);

    debug!(?fs_options, "format-data started");

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device for format-data...",
        FlashExecutor::connect(),
    )
    .await?;

    output::spinner::run_with_spinner(
        "Formatting userdata, cache, metadata...",
        async {
            executor.format_data(fs_options).await;
        },
    )
    .await;

    debug!("format-data done");

    Ok(())
}
