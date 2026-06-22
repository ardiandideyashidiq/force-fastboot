use anyhow::Result;
use tracing::info;

use crate::cli::args::DeviceAction;
use crate::cli::init_stderr_logging;
use crate::flash::executor::BootTarget;
use crate::flash::FlashExecutor;

/// Run a fastboot device operation.
///
/// # Errors
///
/// Returns an error if the device is not reachable or the operation fails.
pub async fn run(verbose: bool, action: DeviceAction) -> Result<()> {
    let level = if verbose { "trace" } else { "info" };
    init_stderr_logging(level);

    info!("connecting to fastboot device");
    let mut executor = FlashExecutor::connect().await?;

    match action {
        DeviceAction::Info => {
            let vars = executor.get_all_vars().await?;
            println!("=== Fastboot Device Info ===");
            for (key, value) in &vars {
                println!("  {key}: {value}");
            }
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
            info!(%target, "reboot command sent");
        }
        DeviceAction::Lock => {
            info!("locking bootloader");
            executor.flashing_lock().await?;
            info!("bootloader locked");
        }
        DeviceAction::Unlock => {
            info!("unlocking bootloader");
            executor.flashing_unlock().await?;
            info!("bootloader unlocked");
        }
        DeviceAction::SetActive { slot } => {
            if slot != "a" && slot != "b" {
                anyhow::bail!("invalid slot '{slot}': expected 'a' or 'b'");
            }
            info!(%slot, "setting active slot");
            executor.set_active_slot(&slot).await?;
            info!(%slot, "active slot set");
        }
        DeviceAction::GetVar { var } => {
            match executor.get_var(&var).await {
                Ok(value) => println!("{var}: {value}"),
                Err(e) => anyhow::bail!("failed to get '{var}': {e}"),
            }
        }
    }

    Ok(())
}
