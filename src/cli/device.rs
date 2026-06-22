use anyhow::Result;
use tracing::info;

use crate::cli::args::DeviceAction;
use crate::cli::init_stderr_logging;
use crate::flash::FlashExecutor;

pub async fn run(action: DeviceAction) -> Result<()> {
    init_stderr_logging("info");

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
            match target.as_str() {
                "system" => executor.reboot().await?,
                "bootloader" => executor.reboot_to("bootloader").await?,
                "fastbootd" | "fastboot" => executor.reboot_to("fastboot").await?,
                "recovery" => executor.reboot_to("recovery").await?,
                _ => anyhow::bail!("unknown reboot target '{target}': expected system, bootloader, fastbootd, or recovery"),
            }
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
