use anyhow::Result;
use tracing::info;

use crate::cli::init_stderr_logging;
use crate::flash::executor::FormatStatus;
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

    let mut wiped = 0usize;
    let mut erased_only = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    println!();
    println!("=== Format-Data Results ===");
    for outcome in &result.outcomes {
        match &outcome.status {
            FormatStatus::Wiped => {
                println!("  {}: WIPED ✓", outcome.partition);
                wiped += 1;
            }
            FormatStatus::ErasedOnly(fs) => {
                println!(
                    "  {}: ERASED (filesystem '{}' not supported, no format)",
                    outcome.partition, fs
                );
                erased_only += 1;
            }
            FormatStatus::Skipped(reason) => {
                println!("  {}: SKIPPED ({})", outcome.partition, reason);
                skipped += 1;
            }
            FormatStatus::Failed(e) => {
                println!("  {}: FAILED — {e}", outcome.partition);
                failed += 1;
            }
        }
    }
    println!();
    println!("  Summary: {wiped} wiped, {erased_only} erased only, {skipped} skipped, {failed} failed");

    Ok(())
}
