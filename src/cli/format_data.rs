use anyhow::Result;
use tracing::info;

use crate::cli::init_stderr_logging;
use crate::flash::FlashExecutor;
use crate::format::generator;

pub async fn run(verbose: bool, fs_options: Vec<String>) -> Result<()> {
    if verbose {
        init_stderr_logging("trace");
    } else {
        init_stderr_logging("info");
    }

    let fs_options = generator::parse_fs_options(&fs_options);

    info!(?fs_options, "connecting to fastboot device");
    let mut executor = FlashExecutor::connect().await?;

    let result = executor.format_data(fs_options).await;
    info!(outcomes = result.outcomes.len(), "format-data done");

    Ok(())
}
