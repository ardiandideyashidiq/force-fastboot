//! Tauri v2 desktop app for pawflash — exposes core flashing operations as
//! IPC commands with progress reporting via `Channel<ProgressEvent>`.

use std::collections::HashMap;
use std::path::Path;

use pawflash_core::flash::executor::BootTarget;
use pawflash_core::flash::FlashExecutor;
use pawflash_core::format::generator::{self, FsType};

use pawflash_core::scatter_parser as sp;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::Emitter;
use tracing::{debug, info, trace, warn};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, registry::Registry, EnvFilter};

// ── Logging init ──────────────────────────────────────────────────────

fn init_logging() {
  let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| EnvFilter::new("info"));
  let subscriber = Registry::default()
    .with(filter)
    .with(
      fmt::Layer::new()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(true)
        .with_level(true)
        .compact(),
    );
  let _ = tracing::subscriber::set_global_default(subscriber);
}

// ── Event types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
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

// ── Helpers ───────────────────────────────────────────────────────────

fn send_progress(ch: &Channel<ProgressEvent>, event: ProgressEvent) {
  trace!(?event, "progress");
  let _ = ch.send(event);
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

#[tracing::instrument(skip_all)]
#[tauri::command]
async fn get_device_info() -> Result<DeviceInfo, String> {
  let Ok(mut executor) = FlashExecutor::connect().await else {
    info!("no fastboot device found");
    return Ok(DeviceInfo { connected: false, serial: None, vars: HashMap::new() });
  };
  let vars = executor.get_all_vars().await.map_err(|e| {
    warn!(error = %e, "get_all_vars failed");
    e.to_string()
  })?;
  let serial = vars.get("serial").cloned();
  let connected = true;
  info!(connected, serial = serial.as_deref().unwrap_or("?"), "device info retrieved");
  Ok(DeviceInfo { connected, serial, vars })
}

#[tracing::instrument(skip(app, on_event))]
#[tauri::command]
async fn force_fastboot(app: tauri::AppHandle, on_event: Channel<ProgressEvent>) -> Result<(), String> {
  send_progress(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Checking fastboot mode...".into() });

  if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
    info!("already in fastboot mode");
    send_progress(&on_event, ProgressEvent::Done { ok: true, detail: "Already in fastboot mode".into() });
    let _ = app.emit("fastboot-devices", ());
    return Ok(());
  }

  send_progress(&on_event, ProgressEvent::Phase { phase: "waiting".into(), message: "Waiting for preloader...".into() });
  let port = pawflash_core::force_fastboot::serial::wait_for_preloader(false)
    .await
    .map_err(|e| { warn!(error = %e, "wait_for_preloader failed"); e.to_string() })?
    .ok_or_else(|| { warn!("no preloader device found"); "No preloader device found".to_string() })?;

  let mut dev = pawflash_core::force_fastboot::serial::open_with_permission_recovery(&port)
    .map_err(|e| { warn!(%port, error = %e, "open_with_permission_recovery failed"); e.to_string() })?;

  info!(%port, "preloader found, sending FASTBOOT");
  send_progress(&on_event, ProgressEvent::Phase { phase: "sending".into(), message: format!("Found preloader on {port}, sending FASTBOOT...") });

  loop {
    use tokio::io::AsyncWriteExt;
    match dev.write_all(b"FASTBOOT").await {
      Ok(()) => { let _ = dev.flush().await; }
      Err(_) => {
        debug!("FASTBOOT write failed, checking mode and reconnecting");
        drop(dev);
        if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
          debug!("device already in fastboot mode after reconnect");
          break;
        }
        let Some(new_port) =
          pawflash_core::force_fastboot::serial::wait_for_preloader(true).await.map_err(|e| e.to_string())?
        else {
          warn!("preloader disappeared during handshake");
          break;
        };
        debug!(port = %new_port, "reconnecting to preloader");
        dev = pawflash_core::force_fastboot::serial::open_with_permission_recovery(&new_port)
          .map_err(|e| e.to_string())?;
        continue;
      }
    }
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    if pawflash_core::force_fastboot::fastboot::in_fastboot_mode().await {
      debug!("fastboot mode confirmed");
      break;
    }
  }

  info!("device now in fastboot mode");
  send_progress(&on_event, ProgressEvent::Done { ok: true, detail: "Device now in fastboot mode".into() });
  let _ = app.emit("fastboot-devices", ());
  Ok(())
}

#[tracing::instrument(skip_all, fields(target))]
#[tauri::command]
async fn reboot_device(target: String) -> Result<(), String> {
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;
  let boot_target: BootTarget = target.parse().map_err(|e: String| e)?;
  info!(?boot_target, "rebooting");
  executor.reboot_to(boot_target).await.map_err(|e| {
    warn!(?boot_target, error = %e, "reboot failed");
    e.to_string()
  })
}

#[tracing::instrument(skip_all)]
#[tauri::command]
async fn lock_bootloader() -> Result<String, String> {
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;
  let resp = executor.flashing_lock().await.map_err(|e| {
    warn!(error = %e, "flashing lock failed");
    e.to_string()
  })?;
  info!(response = %resp, "bootloader locked");
  Ok(resp)
}

#[tracing::instrument(skip_all)]
#[tauri::command]
async fn unlock_bootloader() -> Result<String, String> {
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;
  let resp = executor.flashing_unlock().await.map_err(|e| {
    warn!(error = %e, "flashing unlock failed");
    e.to_string()
  })?;
  info!(response = %resp, "bootloader unlocked");
  Ok(resp)
}

#[tracing::instrument(skip_all, fields(slot))]
#[tauri::command]
async fn set_active_slot(slot: String) -> Result<String, String> {
  if slot != "a" && slot != "b" {
    warn!(%slot, "invalid slot");
    return Err("slot must be 'a' or 'b'".into());
  }
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;
  let resp = executor.set_active_slot(&slot).await.map_err(|e| {
    warn!(%slot, error = %e, "set_active_slot failed");
    e.to_string()
  })?;
  info!(%slot, response = %resp, "active slot set");
  Ok(resp)
}

#[tracing::instrument(skip_all, fields(name))]
#[tauri::command]
async fn get_var(name: String) -> Result<String, String> {
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;
  let value = executor.get_var(&name).await.map_err(|e| {
    warn!(%name, error = %e, "get_var failed");
    e.to_string()
  })?;
  info!(%name, %value, "variable retrieved");
  Ok(value)
}

#[tracing::instrument(skip(on_event))]
#[tauri::command]
async fn disable_vbmeta(on_event: Channel<ProgressEvent>) -> Result<(), String> {
  send_progress(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to device...".into() });
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;

  send_progress(&on_event, ProgressEvent::Phase { phase: "flashing".into(), message: "Flashing empty vbmeta...".into() });
  executor.flash_empty_vbmeta().await.map_err(|e| {
    warn!(error = %e, "flash_empty_vbmeta failed");
    e.to_string()
  })?;

  info!("vbmeta verification disabled");
  send_progress(&on_event, ProgressEvent::Done { ok: true, detail: "vbmeta verification disabled".into() });
  Ok(())
}

#[tracing::instrument(skip(on_event), fields(fs_type, clean_test, fs_options_count = fs_options.len()))]
#[tauri::command]
async fn format_data(
  fs_type: String,
  fs_options: Vec<String>,
  clean_test: bool,
  on_event: Channel<ProgressEvent>,
) -> Result<(), String> {
  send_progress(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to device...".into() });
  let executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;
  let mut executor = executor.ensure_fastbootd().await.map_err(|e| {
    warn!(error = %e, "ensure_fastbootd failed");
    e.to_string()
  })?;

  send_progress(&on_event, ProgressEvent::Phase { phase: "formatting".into(), message: "Formatting data partitions...".into() });
  let fs_override = match fs_type.to_lowercase().as_str() {
    "ext4" => Some(FsType::Ext4),
    "f2fs" => Some(FsType::F2fs),
    _ => None,
  };
  let parsed_options = generator::parse_fs_options(&fs_options);
  let result = executor.format_data(parsed_options, clean_test, fs_override).await.map_err(|e| e.to_string())?;

  for e in map_flash_result(&result) {
    send_progress(&on_event, e);
  }

  let failed: usize = result
    .outcomes
    .iter()
    .filter(|o| matches!(o.status, pawflash_core::flash::results::FormatStatus::Failed(_)))
    .count();
  let wiped: usize = result.outcomes.iter().filter(|o| matches!(o.status, pawflash_core::flash::results::FormatStatus::Wiped)).count();
  info!(%fs_type, clean_test, %wiped, %failed, "format-data complete");

  if failed > 0 {
    warn!(%failed, "format-data completed with failures");
    return Err(format!("format-data completed with {failed} failure(s)"));
  }

  send_progress(&on_event, ProgressEvent::Done { ok: true, detail: "Data partitions formatted".into() });
  Ok(())
}

// ── Scatter commands ──────────────────────────────────────────────────

#[tracing::instrument(skip_all, fields(path))]
#[tauri::command]
async fn parse_scatter(path: String) -> Result<sp::ScatterFile, String> {
  let parsed = sp::parse_scatter(Path::new(&path)).map_err(|e| {
    warn!(%path, error = %e, "parse_scatter failed");
    e.to_string()
  })?;
  let count: usize = parsed.layouts.values().map(Vec::len).sum();
  info!(%path, partition_count = %count, "scatter parsed");
  Ok(parsed)
}

#[tracing::instrument(skip_all, fields(path))]
#[tauri::command]
async fn build_plan(path: String, options: sp::FlashPlanOptions) -> Result<sp::FlashPlan, String> {
  let parsed = sp::parse_scatter(Path::new(&path)).map_err(|e| {
    warn!(%path, error = %e, "parse_scatter for plan failed");
    e.to_string()
  })?;
  let plan = sp::build_flash_plan(&parsed, &options);
  info!(
    actions = %plan.actions.len(),
    skipped = %plan.skipped.len(),
    errors = %plan.errors.len(),
    "flash plan built"
  );
  Ok(plan)
}

#[tracing::instrument(skip(app, on_event, options), fields(path))]
#[tauri::command]
async fn execute_plan(
  app: tauri::AppHandle,
  path: String,
  options: sp::FlashPlanOptions,
  on_event: Channel<ProgressEvent>,
) -> Result<pawflash_core::flash::results::FlashResult, String> {
  // Parse
  send_progress(&on_event, ProgressEvent::Phase { phase: "parsing".into(), message: "Parsing scatter file...".into() });
  let parsed = sp::parse_scatter(Path::new(&path)).map_err(|e| {
    warn!(%path, error = %e, "execute_plan: parse failed");
    e.to_string()
  })?;

  // Build plan
  send_progress(&on_event, ProgressEvent::Phase { phase: "planning".into(), message: "Building flash plan...".into() });
  let plan = sp::build_flash_plan(&parsed, &options);
  debug!(actions = %plan.actions.len(), skipped = %plan.skipped.len(), "plan built");

  if !plan.errors.is_empty() {
    for err in &plan.errors {
      warn!(%err, "plan error");
      send_progress(&on_event, ProgressEvent::Error { message: err.clone() });
    }
    return Err(format!("flash plan has {} error(s)", plan.errors.len()));
  }

  if plan.actions.is_empty() {
    warn!("flash plan has no actions");
    return Err("flash plan has no actions to execute".into());
  }

  // Connect
  send_progress(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to fastboot device...".into() });
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "execute_plan: connect failed");
    e.to_string()
  })?;

  // Execute
  let total = plan.actions.len();
  info!(%total, "starting flash execution");
  send_progress(&on_event, ProgressEvent::Overall { current: 0, total });
  send_progress(&on_event, ProgressEvent::Phase { phase: "flashing".into(), message: format!("Flashing {total} partitions...") });

  let result = executor.execute_plan(&plan, false, None).await;

  // Report outcomes
  for (i, outcome) in result.outcomes.iter().enumerate() {
    debug!(
      partition = %outcome.partition,
      success = %outcome.success,
      response = outcome.response.as_deref().unwrap_or(""),
      "flash outcome"
    );
    send_progress(&on_event, ProgressEvent::FlashProgress {
      partition: outcome.partition.clone(),
      percent: ((i + 1) as f64 / total as f64) * 100.0,
    });
    send_progress(&on_event, ProgressEvent::FlashComplete {
      partition: outcome.partition.clone(),
      success: outcome.success,
      response: outcome.response.clone(),
    });
    if let Some(ref err) = outcome.error.as_ref().filter(|_| !outcome.success) {
      warn!(partition = %outcome.partition, error = %err, "partition flash failed");
      send_progress(&on_event, ProgressEvent::Error { message: format!("{}: {err}", outcome.partition) });
    }
  }

  let _ = app.emit("flash-complete", ());
  info!(
    succeeded = %result.succeeded,
    failed = %result.failed,
    total = %result.total,
    "flash execution complete"
  );
  send_progress(&on_event, ProgressEvent::Done {
    ok: result.failed == 0,
    detail: format!("{}/{} partitions flashed successfully", result.succeeded, result.total),
  });

  Ok(result)
}

#[tracing::instrument(skip(app, on_event), fields(partition, image_path))]
#[tauri::command]
async fn flash_raw_image(
  app: tauri::AppHandle,
  partition: String,
  image_path: String,
  on_event: Channel<ProgressEvent>,
) -> Result<String, String> {
  send_progress(&on_event, ProgressEvent::Phase { phase: "connecting".into(), message: "Connecting to device...".into() });
  let mut executor = FlashExecutor::connect().await.map_err(|e| {
    warn!(error = %e, "connect failed");
    e.to_string()
  })?;

  let path = Path::new(&image_path);
  if !path.exists() {
    warn!(%image_path, "image not found");
    return Err(format!("image not found: {image_path}"));
  }

  send_progress(&on_event, ProgressEvent::Phase { phase: "flashing".into(), message: format!("Flashing {partition}...") });
  debug!(%partition, %image_path, "flashing raw image");
  let resp = executor.flash_raw_image(&partition, path).await.map_err(|e| {
    warn!(%partition, error = %e, "flash_raw_image failed");
    e.to_string()
  })?;

  let _ = app.emit("flash-complete", ());
  info!(%partition, response = %resp, "raw flash complete");
  send_progress(&on_event, ProgressEvent::FlashComplete { partition, success: true, response: Some(resp.clone()) });
  send_progress(&on_event, ProgressEvent::Done { ok: true, detail: "Raw flash complete".into() });

  Ok(resp)
}


// ── App entry ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  init_logging();
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
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

    ])
    .run(tauri::generate_context!())
    .expect("error while running pawflash");
}
