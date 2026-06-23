use anyhow::Result;
use tracing::{debug, info};

use crate::cli::args::DeviceAction;
use crate::flash::executor::BootTarget;
use crate::flash::FlashExecutor;
use crate::output;

/// Run a fastboot device operation.
///
/// # Errors
///
/// Returns an error if the device is not reachable or the operation fails.
pub async fn run(action: DeviceAction) -> Result<()> {
    debug!("device command: {action:?}");

    let mut executor = output::spinner::run_with_spinner("Connecting to fastboot device...", FlashExecutor::connect()).await?;

    match action {
        DeviceAction::Info => {
            let vars = executor.get_all_vars().await?;
            output::status::data(output::tables::device_info(&vars));
        }
        DeviceAction::Reboot { target } => {
            info!(%target, "rebooting device");
            let boot_target = match target.as_str() {
                "system" => BootTarget::System,
                "bootloader" => BootTarget::Bootloader,
                "fastbootd" | "fastboot" => BootTarget::Fastboot,
                "recovery" => BootTarget::Recovery,
                _ => anyhow::bail!("unknown reboot target '{target}': expected system, bootloader, fastbootd, or recovery"),
            };
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
                anyhow::bail!("invalid slot '{slot}': expected 'a' or 'b'");
            }
            let resp = executor.set_active_slot(&slot).await?;
            output::status::ok(format!("{slot} OKAY"), resp);
        }
        DeviceAction::GetVar { var } => {
            match executor.get_var(&var).await {
                Ok(value) => output::status::data(format!("{var}: {value}")),
                Err(e) => anyhow::bail!("failed to get '{var}': {e}"),
            }
        }
    }

    Ok(())
}
