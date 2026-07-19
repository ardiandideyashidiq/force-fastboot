use std::collections::HashMap;
use std::time::Duration;

use miette::{bail, miette, Result};
use tracing::{debug, info};

use crate::cli::args::DeviceAction;
use pawflash_core::flash::executor::BootTarget;
use pawflash_core::flash::executor::FlashExecutor;
use pawflash_core::flash::simulate::SimulatedTransport;
use pawflash_core::output;

/// Run a fastboot device operation.
///
/// When `simulate` is true, uses [`SimulatedTransport`] — no real
/// USB device is needed.
///
/// # Errors
///
/// Returns an error if the device is not reachable or the operation fails.
pub async fn run(action: DeviceAction, simulate: bool) -> Result<()> {
    debug!("device command: {action:?}");

    if simulate {
        output::status::heading("⚠ SIMULATED MODE — no device will be touched");
        let vars = HashMap::from([
            ("version".into(), "0.5".into()),
            ("product".into(), "SIM_DEVICE".into()),
            ("serialno".into(), "SIM000001".into()),
            ("current-slot".into(), "a".into()),
            ("max-download-size".into(), "0x10000000".into()),
            ("is-userspace".into(), "no".into()),
            ("battery-voltage".into(), "4300mV".into()),
            ("battery-soc-ok".into(), "yes".into()),
            ("slot-count".into(), "2".into()),
            ("slot-successful:_a".into(), "yes".into()),
            ("slot-successful:_b".into(), "no".into()),
            ("slot-unbootable:_a".into(), "no".into()),
            ("slot-unbootable:_b".into(), "no".into()),
            ("partition-type:boot".into(), "raw".into()),
            ("partition-size:boot".into(), "0x4000000".into()),
            ("has-slot:boot".into(), "yes".into()),
        ]);
        let transport = SimulatedTransport::new(vars);
        let mut executor = FlashExecutor::new(transport, HashMap::new());
        return dispatch_device_action(&mut executor, action).await;
    }

    let mut executor = output::spinner::run_with_spinner(
        "Connecting to fastboot device (60s timeout)...",
        FlashExecutor::wait_for_device(Duration::from_secs(60)),
    )
    .await?;

    dispatch_device_action(&mut executor, action).await
}

async fn dispatch_device_action<T: pawflash_core::flash::transport::FlashTransport>(
    executor: &mut FlashExecutor<T>,
    action: DeviceAction,
) -> Result<()> {
    match action {
        DeviceAction::Info => {
            let vars = executor.get_all_vars().await?;
            output::status::data(output::tables::device_info(&vars));
        }
        DeviceAction::Reboot { target } => {
            info!(%target, "rebooting device");
            let boot_target: BootTarget = target.parse().map_err(|e: String| miette!(e))?;
            executor.reboot_to(boot_target).await?;
        }
        DeviceAction::Lock => {
            let resp = executor.flashing_lock().await?;
            output::status::ok("OKAY", resp);
        }
        DeviceAction::Unlock => {
            let resp = executor.flashing_unlock().await?;
            output::status::ok("OKAY", resp);
        }
        DeviceAction::SetActive { slot } => {
            if slot != "a" && slot != "b" {
                bail!("invalid slot '{slot}': expected 'a' or 'b'");
            }
            let resp = executor.set_active_slot(&slot).await?;
            output::status::ok(format!("{slot} OKAY"), resp);
        }
        DeviceAction::GetVar { var } => match executor.get_var(&var).await {
            Ok(value) => output::status::data(format!("{var}: {value}")),
            Err(e) => bail!("failed to get '{var}': {e}"),
        },
    }

    Ok(())
}
