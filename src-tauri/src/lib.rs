use std::collections::HashMap;
use std::path::Path;

use pawflash_core::flash::executor::BootTarget;
use pawflash_core::flash::FlashExecutor;
use pawflash_core::format::generator::{self, FsType};
use pawflash_core::gsi::GsiEvent;
use pawflash_core::scatter_parser as sp;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::Emitter;

// ── Event types ───────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum ProgressEvent {
  Phase { phase: String, message: String },
  FlashProgress { partition: String, percent: f64 },
  FlashComplete { partition: String, success: bool, response: Option<String> },
  FormatProgress { partition: String, status: String },
  DeviceAction { action: String, detail: String },
  Overall { current: usize, total: usize },
  Warning { message: String },
  Error { message: String },
  Done { ok: bool, detail: String },
}

#[derive(Clone, Serialize)]
pub struct DeviceInfo {
  pub connected: bool,
  pub serial: Option<String>,
  pub vars: HashMap<String, String>,
}

// ── Helper ────────────────────────────────────────────────────────────

fn send<T: Serialize + Clone>(ch: &Channel<T>, msg: T) {
  let _ = ch.send(msg);
}

fn map_flash_result(result: &pawflash_core::flash::results::FormatDataResult) -> Vec<ProgressEvent> {
  result
    .outcomes
    .iter()
    .map(|o| {
      let status = match &o.status {
        pawflash_core::flash::results::FormatStatus::Wiped => "wiped".into(),
        pawflash_core::flash::results::FormatStatus::ErasedOnly(_) => "erased-only".into(),
        pawflash_core::flash::results::FormatStatus::Skipped(r) => format!("skipped ({r})"),
        pawflash_core::flash::results::FormatStatus::Failed(e) => format!("failed ({e})"),
      };
      ProgressEvent::FormatProgress { partition: o.partition.clone(), status }
    })
    .collect()
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
  send(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Checking fastboot mode...".into() });

  if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
    send(&on_event, ProgressEvent::Done { ok: true, detail: "Already in fastboot mode".into() });
    let _ = app.emit("fastboot-devices", ());
    return Ok(());
  }

  send(&on_event, ProgressEvent::Phase { phase: "waiting".into(), message: "Waiting for preloader...".into() });
  let port = pawflash_core::force_fastboot::serial::wait_for_preloader(false)
    .await
    .map_err(|e| e.to_string())?
    .ok_or("No preloader device found")?;

  let mut dev = pawflash_core::force_fastboot::serial::open_with_permission_recovery(&port)
    .map_err(|e| e.to_string())?;

  send(&on_event, ProgressEvent::Phase { phase: "sending".into(), message: format!("Found preloader on {port}, sending FASTBOOT...") });

  loop {
    use tokio::io::AsyncWriteExt;
    match dev.write_all(b"FASTBOOT").await {
      Ok(()) => { let _ = dev.flush().await; }
      Err(_) => {
        drop(dev);
        if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
          break;
        }
        let Some(new_port) =
          pawflash_core::force_fastboot::serial::wait_for_preloader(true).await.map_err(|e| e.to_string())?
        else {
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

  send(&on_event, ProgressEvent::Done { ok: true, detail: "Device now in fastboot mode".into() });
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
  send(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to device...".into() });
  let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;

  send(&on_event, ProgressEvent::Phase { phase: "flashing".into(), message: "Flashing empty vbmeta...".into() });
  executor.flash_empty_vbmeta().await.map_err(|e| e.to_string())?;

  send(&on_event, ProgressEvent::Done { ok: true, detail: "vbmeta verification disabled".into() });
  Ok(())
}

#[tauri::command]
async fn format_data(
  fs_type: String,
  fs_options: Vec<String>,
  clean_test: bool,
  on_event: Channel<ProgressEvent>,
) -> Result<(), String> {
  send(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to device...".into() });
  let executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;
  let mut executor = executor.ensure_fastbootd().await.map_err(|e| e.to_string())?;

  send(&on_event, ProgressEvent::Phase { phase: "formatting".into(), message: "Formatting data partitions...".into() });
  let fs_override = match fs_type.to_lowercase().as_str() {
    "ext4" => Some(FsType::Ext4),
    "f2fs" => Some(FsType::F2fs),
    _ => None,
  };
  let parsed_options = generator::parse_fs_options(&fs_options);
  let result = executor.format_data(parsed_options, clean_test, fs_override).await;

  let events = map_flash_result(&result);
  for e in events {
    send(&on_event, e);
  }

  let failed: usize = result
    .outcomes
    .iter()
    .filter(|o| matches!(o.status, pawflash_core::flash::results::FormatStatus::Failed(_)))
    .count();
  if failed > 0 {
    return Err(format!("format-data completed with {failed} failure(s)"));
  }

  send(&on_event, ProgressEvent::Done { ok: true, detail: "Data partitions formatted".into() });
  Ok(())
}

// ── New scatter commands ──────────────────────────────────────────────

#[tauri::command]
async fn parse_scatter(path: String) -> Result<sp::ScatterFile, String> {
  let parsed = sp::parse_scatter(Path::new(&path)).map_err(|e| e.to_string())?;
  Ok(parsed)
}

#[tauri::command]
async fn build_plan(path: String, options: sp::FlashPlanOptions) -> Result<sp::FlashPlan, String> {
  let parsed = sp::parse_scatter(Path::new(&path)).map_err(|e| e.to_string())?;
  let plan = sp::build_flash_plan(&parsed, &options);
  Ok(plan)
}

#[tauri::command]
async fn execute_plan(
  app: tauri::AppHandle,
  path: String,
  options: sp::FlashPlanOptions,
  on_event: Channel<ProgressEvent>,
) -> Result<pawflash_core::flash::results::FlashResult, String> {
  // Parse
  send(&on_event, ProgressEvent::Phase { phase: "parsing".into(), message: "Parsing scatter file...".into() });
  let parsed = sp::parse_scatter(Path::new(&path)).map_err(|e| e.to_string())?;

  // Build plan
  send(&on_event, ProgressEvent::Phase { phase: "planning".into(), message: "Building flash plan...".into() });
  let plan = sp::build_flash_plan(&parsed, &options);

  if !plan.errors.is_empty() {
    for err in &plan.errors {
      send(&on_event, ProgressEvent::Error { message: err.clone() });
    }
    return Err(format!("flash plan has {} error(s)", plan.errors.len()));
  }

  if plan.actions.is_empty() {
    return Err("flash plan has no actions to execute".into());
  }

  // Connect
  send(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to fastboot device...".into() });
  let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;

  // Execute
  let total = plan.actions.len();
  send(&on_event, ProgressEvent::Overall { current: 0, total });
  send(&on_event, ProgressEvent::Phase { phase: "flashing".into(), message: format!("Flashing {total} partitions...") });

  let result = executor.execute_plan(&plan, false, None).await;

  // Report outcomes
  for (i, outcome) in result.outcomes.iter().enumerate() {
    send(&on_event, ProgressEvent::FlashProgress { partition: outcome.partition.clone(), percent: ((i + 1) as f64 / total as f64) * 100.0 });
    send(&on_event, ProgressEvent::FlashComplete {
      partition: outcome.partition.clone(),
      success: outcome.success,
      response: outcome.response.clone(),
    });
    if !outcome.success {
      if let Some(ref err) = outcome.error {
        send(&on_event, ProgressEvent::Error { message: format!("{}: {err}", outcome.partition) });
      }
    }
  }

  let _ = app.emit("flash-complete", ());
  send(&on_event, ProgressEvent::Done {
    ok: result.failed == 0,
    detail: format!("{}/{} partitions flashed successfully", result.succeeded, result.total),
  });

  Ok(result)
}

#[tauri::command]
async fn flash_raw_image(
  app: tauri::AppHandle,
  partition: String,
  image_path: String,
  on_event: Channel<ProgressEvent>,
) -> Result<String, String> {
  send(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to device...".into() });
  let mut executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;

  let path = Path::new(&image_path);
  if !path.exists() {
    return Err(format!("image not found: {image_path}"));
  }

  send(&on_event, ProgressEvent::Phase { phase: "flashing".into(), message: format!("Flashing {partition}...") });
  let resp = executor.flash_raw_image(&partition, path).await.map_err(|e| e.to_string())?;

  let _ = app.emit("flash-complete", ());
  send(&on_event, ProgressEvent::FlashComplete { partition, success: true, response: Some(resp.clone()) });
  send(&on_event, ProgressEvent::Done { ok: true, detail: "Raw flash complete".into() });

  Ok(resp)
}

#[tauri::command]
async fn flash_gsi(
  app: tauri::AppHandle,
  image_path: String,
  clean_test: bool,
  on_event: Channel<ProgressEvent>,
) -> Result<String, String> {
  let path = Path::new(&image_path);
  if !path.exists() {
    return Err(format!("GSI image not found: {image_path}"));
    }
    let image = path.canonicalize().map_err(|e| format!("failed to resolve path: {e}"))?;

  send(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to device...".into() });
  let executor = FlashExecutor::connect().await.map_err(|e| e.to_string())?;

  let report = |event: GsiEvent| {
    let msg = match &event {
      GsiEvent::Step(s) => ProgressEvent::Phase { phase: "gsi-step".into(), message: s.as_str().into() },
      GsiEvent::ModeDetected(m) => ProgressEvent::Phase { phase: "mode".into(), message: format!("Detected mode: {}", m.as_str()) },
      GsiEvent::ModeReady(m) => ProgressEvent::Phase { phase: "mode".into(), message: format!("Ready in mode: {}", m.as_str()) },
      GsiEvent::ResolvedPartition { base, partition, size_bytes } => {
        ProgressEvent::Phase { phase: "resolved".into(), message: format!("{base} → {partition} ({size_bytes} bytes)") }
      }
      GsiEvent::Flashing { partition, size_bytes: _ } => {
        ProgressEvent::FlashProgress { partition: partition.clone(), percent: 0.0 }
      }
      GsiEvent::Wiping { partition } => {
        ProgressEvent::FormatProgress { partition: partition.clone(), status: "wiping".into() }
      }
      GsiEvent::PartitionSkipped { partition, reason } => {
        ProgressEvent::Warning { message: format!("Skipped {partition}: {reason}") }
      }
    };
    let _ = on_event.send(msg);
  };

  let outcome = pawflash_core::gsi::execute_gsi_flash(executor, &image, clean_test, None, report)
    .await
    .map_err(|e| e.to_string())?;

  let _ = app.emit("flash-complete", ());
  send(&on_event, ProgressEvent::Done {
    ok: true,
    detail: format!("GSI flash complete ({} partitions flashed, {} bytes)", outcome.summary.flash_count, outcome.summary.total_bytes),
  });

  Ok(format!("{}", outcome.summary.flash_count))
}

// ── App entry ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_fs::init())
    .plugin(tauri_plugin_notification::init())
    .plugin(tauri_plugin_shell::init())
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
      parse_scatter,
      build_plan,
      execute_plan,
      flash_raw_image,
      flash_gsi,
    ])
    .run(tauri::generate_context!())
    .expect("error while running pawflash");
}
