use anyhow::Result;
use tracing::{debug, warn};

use crate::flash::executor::FlashExecutor;
use crate::flash::results::FormatStatus;
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

    let result = executor.format_data(fs_options).await;

    for outcome in &result.outcomes {
        match &outcome.status {
            FormatStatus::Wiped => {
                println!("  {} {}", output::theme::ok("OKAY"), outcome.partition);
            }
            FormatStatus::ErasedOnly(fs) => {
                println!("  {} {} (erased, unrecognised fs: {fs})", output::theme::warn("WARN"), outcome.partition);
            }
            FormatStatus::Skipped(reason) => {
                println!("  {} {} ({reason})", output::theme::dim("SKIP"), outcome.partition);
            }
            FormatStatus::Failed(e) => {
                warn!(partition = %outcome.partition, error = %e, "format failed");
                eprintln!("  {} {} ({e})", output::theme::error("FAIL"), outcome.partition);
            }
        }
    }

    let failed = result.outcomes.iter().filter(|o| matches!(o.status, FormatStatus::Failed(_))).count();
    if failed > 0 {
        anyhow::bail!("format-data completed with {failed} failure(s)");
    }

    debug!("format-data done");

    Ok(())
}
