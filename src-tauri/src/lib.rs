use std::collections::HashMap;

use pawflash_core::flash::executor::BootTarget;
use pawflash_core::flash::FlashExecutor;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::Emitter;

// ── Event types ───────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum ProgressEvent {
    Message { text: String },
    Done { ok: bool, detail: String },
}

#[derive(Clone, Serialize)]
pub struct DeviceInfo {
    pub connected: bool,
    pub serial: Option<String>,
    pub vars: HashMap<String, String>,
}

// ── Commands ──────────────────────────────────────────────────────────

#[tauri::command]
async fn get_device_info() -> Result<DeviceInfo, String> {
    let Ok(mut executor) = FlashExecutor::connect().await else {
        return Ok(DeviceInfo { connected: false, serial: None, vars: HashMap::new() });
    };
    let vars = executor.get_all_vars().await.map_err(|e| e.to_string())?;
    let serial = vars.get("serial").cloned();
    Ok(DeviceInfo { connected: true, serial, vars })
}

#[tauri::command]
async fn force_fastboot(app: tauri::AppHandle, on_event: Channel<ProgressEvent>) -> Result<(), String> {
    if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
        let _ = on_event.send(ProgressEvent::Message { text: "Already in fastboot mode".into() });
        let _ = app.emit("fastboot-devices", ());
        return Ok(());
    }

    let port = pawflash_core::force_fastboot::serial::wait_for_preloader(false)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No preloader device found")?;

    let mut dev = pawflash_core::force_fastboot::serial::open_with_permission_recovery(&port)
        .map_err(|e| e.to_string())?;

    let _ = on_event.send(ProgressEvent::Message { text: format!("Found preloader on {port}, sending FASTBOOT...") });

    loop {
        use tokio::io::AsyncWriteExt;
        match dev.write_all(b"FASTBOOT").await {
            Ok(()) => { let _ = dev.flush().await; }
            Err(_) => {
                drop(dev);
                if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
                    break;
                }
                let Some(new_port) = pawflash_core::force_fastboot::serial::wait_for_preloader(true).await.map_err(|e| e.to_string())? else {
                    break;
                };
                dev = pawflash_core::force_fastboot::serial::open_with_permission_recovery(&new_port)
                    .map_err(|e| e.to_string())?;
                continue;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
            break;
        }
    }

    let _ = on_event.send(ProgressEvent::Done { ok: true, detail: "Device now in fastboot mode".into() });
    let _ = app.emit("fastboot-devices", ());
    Ok(())
}

#[tauri::command]
async fn reboot_device(target: String) -> Result<(), String> {
    let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;
    let boot_target = match target.as_str() {
        "system" => BootTarget::System,
        "bootloader" => BootTarget::Bootloader,
        "fastbootd" | "fastboot" => BootTarget::Fastboot,
        "recovery" => BootTarget::Recovery,
        _ => return Err(format!("unknown target '{target}'")),
    };
    executor.reboot_to(boot_target).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn lock_bootloader() -> Result<String, String> {
    let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;
    executor.flashing_lock().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn unlock_bootloader() -> Result<String, String> {
    let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;
    executor.flashing_unlock().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_active_slot(slot: String) -> Result<String, String> {
    if slot != "a" && slot != "b" {
        return Err("slot must be 'a' or 'b'".into());
    }
    let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;
    executor.set_active_slot(&slot).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_var(name: String) -> Result<String, String> {
    let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;
    executor.get_var(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn disable_vbmeta(on_event: Channel<ProgressEvent>) -> Result<(), String> {
    let _ = on_event.send(ProgressEvent::Message { text: "Connecting to device...".into() });
    let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;

    let _ = on_event.send(ProgressEvent::Message { text: "Flashing empty vbmeta...".into() });
    executor.flash_empty_vbmeta().await.map_err(|e| e.to_string())?;

    let _ = on_event.send(ProgressEvent::Done { ok: true, detail: "vbmeta verification disabled".into() });
    Ok(())
}

#[tauri::command]
async fn format_data(
    fs_type: String,
    on_event: Channel<ProgressEvent>,
) -> Result<(), String> {
    let _ = on_event.send(ProgressEvent::Message { text: "Connecting to device...".into() });
    let executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;
    let mut executor = executor.ensure_fastbootd().await.map_err(|e| e.to_string())?;

    let _ = on_event.send(ProgressEvent::Message { text: "Formatting data partitions...".into() });
    let fs_override = match fs_type.to_lowercase().as_str() {
        "ext4" => Some(pawflash_core::format::generator::FsType::Ext4),
        "f2fs" => Some(pawflash_core::format::generator::FsType::F2fs),
        _ => None,
    };
    let result = executor.format_data(Default::default(), false, fs_override).await;
    let failed: usize = result.outcomes.iter().filter(|o| matches!(o.status, pawflash_core::flash::results::FormatStatus::Failed(_))).count();
    if failed > 0 {
        return Err(format!("format-data completed with {failed} failure(s)"));
    }

    let _ = on_event.send(ProgressEvent::Done { ok: true, detail: "Data partitions formatted".into() });
    Ok(())
}

// ── App entry ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_device_info,
            force_fastboot,
            reboot_device,
            lock_bootloader,
            unlock_bootloader,
            set_active_slot,
            get_var,
            disable_vbmeta,
            format_data,
        ])
        .run(tauri::generate_context!())
        .expect("error while running pawflash");
}
