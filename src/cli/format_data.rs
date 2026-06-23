use anyhow::Result;
use tracing::debug;

use crate::flash::executor::FlashExecutor;
use crate::format::generator;
use crate::output;

/// Erase and format userdata, cache, and metadata.
///
/// # Errors
///
/// Returns an error if the device is not reachable or formatting fails.
pub async fn run(fs_options: Vec<String>, clean_test: bool) -> Result<()> {
    let fs_options = generator::parse_fs_options(&fs_options);

    debug!(?fs_options, clean_test, "format-data started");

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device for format-data...",
        FlashExecutor::connect(),
    )
    .await?;

    let result = executor.format_data(fs_options, clean_test).await;

    let failed = output::format_display::print_format_results(&result);
    if failed > 0 {
        anyhow::bail!("format-data completed with {failed} failure(s)");
    }

    debug!("format-data done");

    Ok(())
}
