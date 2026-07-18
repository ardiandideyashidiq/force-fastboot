use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use fastboot_protocol::nusb::NusbFastBoot;
use indicatif::ProgressBar;

use crate::flash::transport::FlashTransport;
use tokio::io::AsyncReadExt;
use tracing::{debug, info, warn};

static EXPECTED_SERIAL: OnceLock<String> = OnceLock::new();

/// Set an expected device serial number to verify against when connecting.
/// When set, `connect()` will filter devices and reject mismatches.
pub fn set_expected_serial(serial: &str) {
    _ = EXPECTED_SERIAL.set(serial.to_string());
}

fn expected_serial() -> Option<&'static str> {
    EXPECTED_SERIAL.get().map(String::as_str)
}

use crate::flash::error::FlashError;
use crate::flash::error::Result;
use crate::flash::results::{FlashOutcome, FlashResult};
use crate::scatter_parser::types::FlashPlan;

const EMPTY_VBMETA: &[u8] = &[
    0x41, 0x56, 0x42, 0x30, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x61, 0x76, 0x62, 0x74,
    0x6f, 0x6f, 0x6c, 0x20, 0x31, 0x2e, 0x33, 0x2e, 0x30, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];

/// Reboot target modes understood by fastboot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BootTarget {
    System,
    Bootloader,
    Fastboot,
    Recovery,
}

impl BootTarget {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Bootloader => "bootloader",
            Self::Fastboot => "fastboot",
            Self::Recovery => "recovery",
        }
    }
}

impl fmt::Display for BootTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Fastboot flash executor.
pub struct FlashExecutor<T: FlashTransport = NusbFastBoot> {
    pub(crate) fb: T,
    device_vars: HashMap<String, String>,
}

impl<T: FlashTransport> FlashExecutor<T> {
    pub const fn new(fb: T, device_vars: HashMap<String, String>) -> Self {
        Self { fb, device_vars }
    }
}

impl FlashExecutor<NusbFastBoot> {
    /// # Errors
    /// Returns `NoDevice` if no fastboot device is found, or
    /// `DeviceMismatch` if the device serial does not match the expected value.
    pub async fn connect() -> Result<Self> {
        let expected = expected_serial();
        let all: Vec<_> = fastboot_protocol::nusb::devices()
            .await
            .map_err(|_| FlashError::NoDevice)?
            .filter(|info| expected.is_none_or(|exp| info.serial_number() == Some(exp)))
            .collect();
        if all.len() > 1 {
            warn!(
                count = all.len(),
                "multiple fastboot devices found – using the first one; \
                 disconnect extras to avoid targeting the wrong device"
            );
        }
        let info = all.into_iter().next().ok_or(FlashError::NoDevice)?;
        debug!(
            vidpid = format_args!("{:04x}:{:04x}", info.vendor_id(), info.product_id()),
            serial = info.serial_number().unwrap_or("?"),
            "connecting to fastboot device"
        );
        let mut fb = NusbFastBoot::from_info(&info).await?;
        let device_vars = match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            fb.get_all_vars(),
        )
            .await
        {
            Ok(Ok(vars)) => vars,
            Ok(Err(e)) => {
                debug!(error = %e, "getvar:all failed, falling back to individual queries");
                HashMap::new()
            }
            Err(_) => {
                debug!("getvar:all timed out, falling back to individual queries");
                HashMap::new()
            }
        };
        let device_vars = if device_vars.is_empty() {
            let mut vars: HashMap<String, String> = HashMap::new();
            for var in ["version", "product", "serialno", "current-slot", "max-download-size"] {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    fb.get_var(var),
                )
                    .await
                {
                    Ok(Ok(v)) => { vars.insert(var.to_string(), v); }
                    Ok(Err(e)) => { debug!(%var, error = %e, "getvar failed"); }
                    Err(_) => { debug!(%var, "getvar timed out"); }
                }
            }
            vars
        } else {
            device_vars
        };
        if let Some(expected) = expected {
            match device_vars.get("serialno").map(String::as_str) {
                Some(s) if s == expected => {
                    debug!(serial = %s, "device serial matches expected");
                }
                Some(s) => {
                    return Err(FlashError::DeviceMismatch {
                        expected: expected.to_string(),
                        actual: s.to_string(),
                    });
                }
                None => {
                    warn!("--serial set but device did not report serialno; proceeding");
                }
            }
        }
        info!(
            product = device_vars.get("product").map_or("?", |s| s.as_str()),
            serial = device_vars.get("serialno").map_or("?", |s| s.as_str()),
            version = device_vars.get("version").map_or("?", |s| s.as_str()),
            "connected to fastboot device"
        );
        Ok(Self { fb, device_vars })
    }

    /// # Errors
    /// Returns `NoDevice` if no fastboot device appears within the timeout.
    pub async fn wait_for_device(timeout: Duration) -> Result<Self> {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let start = std::time::Instant::now();
        let mut last_log = start;
        loop {
            if start.elapsed() > timeout {
                return Err(FlashError::NoDevice);
            }
            match Self::connect().await {
                Ok(executor) => return Ok(executor),
                Err(e) => {
                    if last_log.elapsed() > Duration::from_secs(5) {
                        warn!("waiting for fastboot device after reboot (error: {e}) ...");
                        last_log = std::time::Instant::now();
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }

    /// # Errors
    /// Returns an error if the device does not reappear within 120 seconds.
    pub async fn reboot_and_wait(mut self, target: BootTarget) -> Result<Self> {
        debug!(?target, "rebooting device and waiting for reconnect");
        if let Err(e) = self.fb.reboot_to(target.as_str()).await {
            warn!(?target, error = %e, "reboot command error (device may have disconnected)");
        }
        drop(self);
        Self::wait_for_device(Duration::from_secs(120)).await
    }

    /// # Errors
    /// Returns an error if the device cannot transition to fastbootd.
    pub async fn ensure_fastbootd(mut self) -> Result<Self> {
        let is_fastbootd = self.fb.get_var("is-userspace").await.is_ok_and(|v| v == "yes");
        if is_fastbootd {
            debug!("already in fastbootd mode");
            return Ok(self);
        }
        info!("device is in bootloader mode, rebooting to fastbootd");
        self.reboot_and_wait(BootTarget::Fastboot).await
    }
}

impl<T: FlashTransport> FlashExecutor<T> {
    #[must_use]
    pub const fn device_vars(&self) -> &HashMap<String, String> {
        &self.device_vars
    }

    /// # Errors
    /// Returns an error if the device does not respond.
    pub async fn get_var(&mut self, var: &str) -> Result<String> {
        self.fb.get_var(var).await
    }

    /// # Errors
    /// Returns an error if the device does not respond within the timeout.
    pub async fn get_all_vars(&mut self) -> Result<HashMap<String, String>> {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.fb.get_all_vars(),
        )
            .await
            .map_err(|_| FlashError::NoDevice)?
    }

    /// # Errors
    /// Returns an error if the reboot command fails.
    pub async fn reboot(&mut self) -> Result<()> {
        self.fb.reboot().await.map(drop)
    }

    /// # Errors
    /// Returns an error if the reboot command fails.
    pub async fn reboot_to(&mut self, target: BootTarget) -> Result<()> {
        self.fb.reboot_to(target.as_str()).await.map(drop)
    }

    /// # Errors
    /// Returns an error if the flashing command fails.
    pub async fn flashing_lock(&mut self) -> Result<String> {
        self.fb.flashing("lock").await
    }

    /// # Errors
    /// Returns an error if the flashing command fails.
    pub async fn flashing_unlock(&mut self) -> Result<String> {
        self.fb.flashing("unlock").await
    }

    /// # Errors
    /// Returns an error if the `set_active` command fails.
    pub async fn set_active_slot(&mut self, slot: &str) -> Result<String> {
        self.fb.set_active(slot).await
    }

    /// # Errors
    /// Returns an error if the flash command fails.
    pub async fn execute_plan(
        &mut self,
        plan: &FlashPlan,
        dry_run: bool,
        progress_bar: Option<&ProgressBar>,
    ) -> FlashResult {
        let all_actions: Vec<_> = plan.actions.iter().filter(|a| a.action == "flash").collect();
        let total = all_actions.len();
        let mut outcomes = Vec::with_capacity(total);
        if dry_run {
            info!(total, "DRY RUN — no data will be written");
        } else {
            info!(total, "starting flash execution");
        }
        let max_download = self.fb.get_var("max-download-size").await.ok()
            .and_then(|s| fastboot_protocol::protocol::parse_u32(&s).ok())
            .unwrap_or(256 * 1024 * 1024);
        for action in &all_actions {
            let partition = &action.partition;
            info!(%partition, "Writing partition");
            let start = Instant::now();
            let result = self
                .flash_partition(action, dry_run, max_download, progress_bar)
                .await;
            let duration = start.elapsed();
            match result {
                Ok(response) => {
                    info!(%partition, duration = ?duration, response, "flash successful");
                    outcomes.push(FlashOutcome {
                        partition: partition.clone(),
                        success: true,
                        response: Some(response),
                        duration,
                        error: None,
                    });
                }
                Err(e) => {
                    warn!(%partition, duration = ?duration, error = %e, "flash failed, skipping");
                    if let Some(pb) = progress_bar {
                        pb.abandon_with_message(format!("{partition} failed"));
                    }
                    outcomes.push(FlashOutcome {
                        partition: partition.clone(),
                        success: false,
                        response: None,
                        duration,
                        error: Some(e),
                    });
                }
            }
        }
        let succeeded = outcomes.iter().filter(|o| o.success).count();
        let failed = outcomes.iter().filter(|o| !o.success).count();
        info!(succeeded, failed, total, "flash plan execution complete");
        FlashResult { total, succeeded, failed, outcomes }
    }

    /// # Errors
    /// Returns an error if the fastboot query fails.
    pub async fn is_logical(&mut self, partition: &str) -> Result<bool> {
        self.fb.is_logical(partition).await
    }

    /// # Errors
    /// Returns an error if the resize command fails.
    pub async fn resize_logical_partition(&mut self, partition: &str, size: u64) -> Result<()> {
        self.fb.resize_logical_partition(partition, size).await
    }

    /// # Errors
    /// Returns an error if the download or flash command fails.
    ///
    /// # Panics
    /// Panics if `EMPTY_VBMETA` exceeds 4 GiB (impossible for a 512-byte image).
    pub async fn flash_empty_vbmeta(&mut self) -> Result<String> {
        let data = EMPTY_VBMETA;
        debug!("flashing empty vbmeta to both slots");
        let mut last_resp = String::new();
        for slot in &["a", "b"] {
            let partition = format!("vbmeta_{slot}");
            info!(%partition, "flashing empty vbmeta");
            let mut sender = self.fb.download(
                u32::try_from(data.len())
                    .expect("EMPTY_VBMETA is 512 bytes, always fits in u32"),
            ).await?;
            sender.extend_from_slice(data).await?;
            sender.finish().await?;
            last_resp = self.fb.flash(&partition).await?;
        }
        Ok(last_resp)
    }

    /// Flash a raw image to a partition. Public entry point for `flash-raw`.
    /// Returns the device response message.
    ///
    /// # Errors
    ///
    /// Returns an error if the image file cannot be read, the device cannot
    /// accept the data, or the flash command fails.
    pub async fn flash_raw_image(
        &mut self,
        partition: &str,
        image_path: &Path,
    ) -> Result<String> {
        debug!(%partition, image_path = %image_path.display(), "flash_raw_image entry");
        let max_download = parse_max_download(&mut self.fb).await?;

        self.flash_image_to_partition(partition, image_path, max_download, None).await
    }

    /// Shared helper: erase partition, then download+flash (single or chunked).
    /// Detects Android sparse images and routes to the sparse-aware handler.
    /// Returns the device response message.
    async fn flash_image_to_partition(
        &mut self,
        partition: &str,
        path: &Path,
        max_download: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<String> {
        // Shared transfer buffer reused across all sparse operations.
        let mut xbuf = crate::flash::sparse::XferBuf::new();

        // Route Android sparse images through the sparse-aware handler.
        if crate::flash::sparse::is_sparse_image(path).await.unwrap_or(false) {
            let file_len = tokio::fs::metadata(path).await?.len();
            return crate::flash::sparse::flash_sparse_image(
                &mut self.fb,
                partition,
                path,
                file_len,
                max_download,
                progress_bar,
                &mut xbuf,
            )
            .await;
        }

        let file_len = tokio::fs::metadata(path).await?.len();
        let size = u32::try_from(file_len).unwrap_or(u32::MAX);

        if let Some(pb) = progress_bar {
            pb.set_length(file_len);
            pb.set_prefix(partition.to_string());
            pb.reset();
            pb.set_position(0);
        }

        debug!(%partition, file_size = file_len, max_download, "flashing image to partition");

        if size > max_download {
            // Route through sparse wrapping to avoid each flash overwriting from
            // offset 0 (the fastbootd flash handler writes downloaded data at the
            // start of the partition; raw chunked flash would only leave the last
            // chunk intact).  Sparse wrapping encodes offset metadata so the device
            // writes each split to the correct position, matching AOSP behaviour.
            info!(%partition, file_len, %max_download, "image exceeds max download, wrapping in sparse format");
            crate::flash::sparse::flash_sparse_wrapped(
                &mut self.fb,
                partition,
                path,
                file_len,
                max_download,
                &mut xbuf,
            )
            .await
        } else {
            self.flash_raw_partition(partition, path, size, progress_bar).await
        }
    }

    async fn flash_partition(
        &mut self,
        action: &crate::scatter_parser::types::FlashAction,
        dry_run: bool,
        max_download: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<String> {
        let partition = &action.partition;
        let Some(image_path) = action.image_resolved_path() else {
            return Err(FlashError::ActionFailed {
                partition: partition.clone(),
                reason: "no resolved image path".into(),
            });
        };

        let path = Path::new(image_path);
        if !path.exists() {
            return Err(FlashError::ImageNotFound(path.to_path_buf()));
        }

        debug!(
            %partition,
            %image_path,
            max_download,
            "checking image"
        );

        if dry_run {
            let file_len = tokio::fs::metadata(path).await?.len();
            info!(%partition, %image_path, size = file_len, "dry run: would flash this image");
            return Ok(String::new());
        }

        self.flash_image_to_partition(partition, path, max_download, progress_bar).await
    }

    /// Flash a partition that fits in a single download.
    /// Returns the device response message.
    pub(crate) async fn flash_raw_partition(
        &mut self,
        partition: &str,
        path: &Path,
        size: u32,
        progress_bar: Option<&ProgressBar>,
    ) -> Result<String> {
        debug!(%partition, file_size = size, "flashing raw partition");
        let mut file = tokio::fs::File::open(path).await?;
        let mut sender = self.fb.download(size).await?;

        let mut buf = vec![0u8; 1024 * 1024];
        let mut written = 0u64;
        loop {
            let n = file.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            sender.extend_from_slice(&buf[..n]).await?;
            written += n as u64;
            if let Some(pb) = progress_bar {
                pb.set_position(written);
            }
        }

        sender.finish().await?;
        let resp = self.fb.flash(partition).await?;
        if let Some(pb) = progress_bar {
            pb.set_position(u64::from(size));
        }
        debug!(%partition, response = resp, "raw partition flash complete");
        Ok(resp)
    }
}

/// Query `max-download-size` from the device and validate it is
/// reasonable.  Returns an error if the value is present but below 1 MiB.
pub(crate) async fn parse_max_download(fb: &mut impl FlashTransport) -> Result<u32> {
    let raw = fb.get_var("max-download-size").await.ok();
    let val = raw.as_deref()
        .and_then(|s| fastboot_protocol::protocol::parse_u32(s).ok())
        .unwrap_or(256 * 1024 * 1024);
    if val < 1024 * 1024 {
        return Err(FlashError::ActionFailed {
            partition: "(global)".into(),
            reason: format!("device max-download-size ({val}) is unreasonably small"),
        });
    }
    Ok(val)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use serde_json::json;
    use crate::flash::mock::MockTransport;
    use crate::flash::executor::FlashExecutor;
    use crate::scatter_parser::types::{FlashPlan, FlashAction};
    use crate::scatter_parser::types::FlashPlanSummary;

    fn mock_executor() -> FlashExecutor<MockTransport> {
        let fb = MockTransport::new();
        FlashExecutor::new(fb, HashMap::new())
    }

    fn make_action(partition: &str, resolved: Option<&str>) -> FlashAction {
        let image = resolved.map(|p| json!({ "path": { "resolved_path": p } }));
        FlashAction {
            action: "flash".into(),
            partition: partition.into(),
            base_name: String::new(),
            slot: None,
            layout: String::new(),
            region: String::new(),
            start: 0,
            start_hex: String::new(),
            size: 0,
            size_hex: String::new(),
            size_human: String::new(),
            image,
            image_type: None,
            safety_class: String::new(),
            reason: String::new(),
            warnings: Vec::new(),
        }
    }

    fn make_empty_plan(actions: Vec<FlashAction>) -> FlashPlan {
        FlashPlan {
            actions,
            mode: String::new(),
            storage_selection: String::new(),
            selected_layouts: Vec::new(),
            platform: None,
            project: None,
            firmware_dir: None,
            package_root: None,
            options: json!({}),
            summary: FlashPlanSummary::default(),
            skipped: Vec::new(),
            incomplete_slots: std::collections::BTreeMap::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[tokio::test]
    async fn execute_plan_happy_path_dry_run() {
        let dir = tempfile::TempDir::new().unwrap();
        let img = dir.path().join("boot.img");
        std::fs::write(&img, b"fake image data").unwrap();
        let mut exec = mock_executor();
        let plan = make_empty_plan(vec![
            make_action("boot", img.to_str()),
        ]);
        let result = exec.execute_plan(&plan, true, None).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.failed, 0);
    }

    #[tokio::test]
    async fn execute_plan_missing_image_skipped() {
        let mut exec = mock_executor();
        let plan = make_empty_plan(vec![
            make_action("boot", None),
        ]);
        let result = exec.execute_plan(&plan, false, None).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.succeeded, 0);
        assert_eq!(result.failed, 1);
    }

    #[tokio::test]
    async fn execute_plan_download_failure_continues() {
        let dir = tempfile::TempDir::new().unwrap();
        let img = dir.path().join("boot.img");
        std::fs::write(&img, b"fake image data").unwrap();
        let fb = MockTransport::new();
        let mut exec = FlashExecutor::new(fb, HashMap::new());
        exec.fb.fail_download = true;
        let plan = make_empty_plan(vec![
            make_action("boot", img.to_str()),
        ]);
        let result = exec.execute_plan(&plan, false, None).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.succeeded, 0);
        assert_eq!(result.failed, 1);
    }

    #[tokio::test]
    async fn execute_plan_dry_run_sends_no_commands() {
        let dir = tempfile::TempDir::new().unwrap();
        let img = dir.path().join("boot.img");
        std::fs::write(&img, b"fake").unwrap();
        let mut exec = mock_executor();
        let plan = make_empty_plan(vec![
            make_action("boot", img.to_str()),
        ]);
        let result = exec.execute_plan(&plan, true, None).await;
        assert_eq!(result.total, 1);
        assert_eq!(result.succeeded, 1);
        // Only get_var for max-download-size, no download/flash commands
        let cmds = exec.fb.commands();
        assert!(cmds.iter().all(|c| c.starts_with("get_var:")), "dry run should only query vars, got: {cmds:?}");
    }
}

