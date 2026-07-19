use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Result};
use tracing::debug;

use pawflash_core::flash::executor::FlashExecutor;
use pawflash_core::flash::simulate::SimulatedTransport;
use pawflash_core::format::generator::{self, FsType};
use pawflash_core::output;

/// Erase and format userdata, metadata, and cache.
///
/// When `simulate` is true, uses [`SimulatedTransport`] with
/// `is-userspace: yes` — no real device is touched.
///
/// # Errors
///
/// Returns an error if the device is not reachable or formatting fails.
pub async fn run(
    fs_options: Vec<String>,
    fs_type_raw: String,
    clean_test: bool,
    simulate: bool,
) -> Result<()> {
    let fs_options = generator::parse_fs_options(&fs_options);

    let fs_type_override = match fs_type_raw.to_lowercase().as_str() {
        "ext4" => Some(FsType::Ext4),
        "f2fs" => Some(FsType::F2fs),
        other => bail!("invalid --fs-type '{other}': expected ext4 or f2fs"),
    };

    debug!(?fs_options, fs_type = ?fs_type_override, clean_test, "format-data started");

    if simulate {
        output::status::heading("⚠ SIMULATED MODE — no device will be touched");
        let vars = HashMap::from([
            ("max-download-size".into(), "0x10000000".into()),
            ("product".into(), "SIM_DEVICE".into()),
            ("serialno".into(), "SIM000001".into()),
            ("version".into(), "0.5".into()),
            ("current-slot".into(), "a".into()),
            ("is-userspace".into(), "yes".into()),
        ]);
        let transport = SimulatedTransport::new(vars);
        let mut executor = FlashExecutor::new(transport, HashMap::new());
        let result = executor.format_data(fs_options, clean_test, fs_type_override).await?;
        let failed = pawflash_core::flash::results::print_format_results(&result);
        if failed > 0 {
            bail!("simulated format-data completed with {failed} failure(s)");
        }
        debug!("simulated format-data done");
        return Ok(());
    }

    let executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device for format-data (60s timeout)...",
        FlashExecutor::wait_for_device(Duration::from_secs(60)),
    )
    .await?;

    // Transition to fastbootd so we can cancel OTA snapshots and access
    // logical partitions (metadata/userdata may be inside super).
    let mut executor = output::spinner::run_with_spinner(
        "Rebooting to fastbootd for format-data...",
        executor.ensure_fastbootd(),
    )
    .await?;

    let result = executor.format_data(fs_options, clean_test, fs_type_override).await?;

    let failed = pawflash_core::flash::results::print_format_results(&result);
    if failed > 0 {
        bail!("format-data completed with {failed} failure(s)");
    }

    debug!("format-data done");

    Ok(())
}
