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

    let wiped = result.outcomes.iter().filter(|o| matches!(o.status, crate::flash::executor::FormatStatus::Wiped)).count();
    let failed = result.outcomes.iter().filter(|o| matches!(o.status, crate::flash::executor::FormatStatus::Failed(_))).count();
    let erased_only = result.outcomes.iter().filter(|o| matches!(o.status, crate::flash::executor::FormatStatus::ErasedOnly(_))).count();
    let skipped = result.outcomes.iter().filter(|o| matches!(o.status, crate::flash::executor::FormatStatus::Skipped(_))).count();
    info!(wiped, erased_only, skipped, failed, "format-data done");

    Ok(())
}
