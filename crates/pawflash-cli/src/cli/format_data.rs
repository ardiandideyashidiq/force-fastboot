use anyhow::{bail, Result};
use tracing::debug;

use pawflash_core::flash::executor::FlashExecutor;
use pawflash_core::format::generator::{self, FsType};
use pawflash_core::output;

/// Erase and format userdata, metadata, and cache.
///
/// # Errors
///
/// Returns an error if the device is not reachable or formatting fails.
pub async fn run(
    fs_options: Vec<String>,
    fs_type_raw: String,
    clean_test: bool,
) -> Result<()> {
    let fs_options = generator::parse_fs_options(&fs_options);

    let fs_type_override = match fs_type_raw.to_lowercase().as_str() {
        "ext4" => Some(FsType::Ext4),
        "f2fs" => Some(FsType::F2fs),
        other => bail!("invalid --fs-type '{other}': expected ext4 or f2fs"),
    };

    debug!(?fs_options, fs_type = ?fs_type_override, clean_test, "format-data started");

    let executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device for format-data...",
        FlashExecutor::connect(),
    )
    .await?;

    // Transition to fastbootd so we can cancel OTA snapshots and access
    // logical partitions (metadata/userdata may be inside super).
    let mut executor = output::spinner::run_with_spinner(
        "Rebooting to fastbootd for format-data...",
        executor.ensure_fastbootd(),
    )
    .await?;

    let result = executor.format_data(fs_options, clean_test, fs_type_override).await;

    let failed = pawflash_core::flash::results::print_format_results(&result);
    if failed > 0 {
        bail!("format-data completed with {failed} failure(s)");
    }

    debug!("format-data done");

    Ok(())
}
