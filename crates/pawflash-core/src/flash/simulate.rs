use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::flash::error::Result;
use crate::flash::transport::DownloadSender;
use crate::flash::transport::FlashTransport;
use crate::scatter_parser::types::ScatterFile;

// ── Helpers ─────────────────────────────────────────────────────────

/// Convert a byte count + throughput to a [`Duration`] using integer
/// arithmetic (nanosecond precision, saturating).
const fn bytes_to_delay(bytes: u64, bytes_per_sec: u64) -> Duration {
    let mul = bytes.saturating_mul(1_000_000_000);
    let nanos = match mul.checked_div(bytes_per_sec) {
        Some(n) => n,
        None => u64::MAX,
    };
    Duration::from_nanos(nanos)
}

// ── Speed configuration ────────────────────────────────────────────

/// Transfer and flash write speeds for simulation (integer bytes/sec).
///
/// Controls how fast data appears to travel over USB and how fast
/// the device appears to write to flash storage.
#[derive(Debug, Clone)]
pub struct SpeedConfig {
    /// USB transfer throughput (bytes/sec).
    /// USB 3.0 ≈ `209_715_200` (200 MiB/s), USB 2.0 ≈ `36_700_160` (35 MiB/s).
    pub usb_bytes_per_sec: u64,
    /// Flash write throughput (bytes/sec).
    /// UFS ≈ `209_715_200`, eMMC ≈ `52_428_800`.
    pub flash_bytes_per_sec: u64,
    /// Base latency applied to every command.
    pub command_latency: Duration,
}

impl Default for SpeedConfig {
    fn default() -> Self {
        Self {
            usb_bytes_per_sec: 200 * 1024 * 1024,
            flash_bytes_per_sec: 200 * 1024 * 1024,
            command_latency: Duration::from_millis(50),
        }
    }
}

impl SpeedConfig {
    /// Conservative USB 2.0 + eMMC profile.
    #[must_use]
    pub const fn usb2() -> Self {
        Self {
            usb_bytes_per_sec: 35 * 1024 * 1024,
            flash_bytes_per_sec: 50 * 1024 * 1024,
            command_latency: Duration::from_millis(100),
        }
    }
}

// ── Download sink ──────────────────────────────────────────────────

/// Accumulates downloaded data and simulates USB transfer delay.
///
/// Each `extend_from_slice` call sleeps proportionally to the chunk size
/// divided by the configured USB speed, making progress-bar updates
/// feel realistic.
pub struct SimulatedDownloadSink {
    speed: Arc<SpeedConfig>,
    data: Vec<u8>,
}

impl SimulatedDownloadSink {
    #[must_use]
    pub const fn new(speed: Arc<SpeedConfig>) -> Self {
        Self { speed, data: Vec::new() }
    }

    /// Simulate USB transfer delay for a data chunk.
    ///
    /// # Errors
    /// Returns an error if the data chunk is malformed (never in practice).
    pub async fn extend_from_slice(&mut self, data: &[u8]) -> Result<()> {
        let delay = bytes_to_delay(data.len() as u64, self.speed.usb_bytes_per_sec);
        if delay > Duration::from_millis(1) {
            tokio::time::sleep(delay).await;
        }
        self.data.extend_from_slice(data);
        Ok(())
    }

    /// Finalise the download with a short latency.
    ///
    /// # Errors
    /// Returns an error if finalisation fails (never in practice).
    pub async fn finish(self) -> Result<()> {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(())
    }

    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

// ── Simulated transport ────────────────────────────────────────────

/// Fastboot transport backed by simulation — no real device required.
///
/// Behaves like a real fastboot device but routes all I/O through
/// configurable speed profiles instead of actual USB hardware.
///
/// **Scatter flash uses real disk I/O**: image files are read from
/// disk by the executor; only the USB transfer + flash write phases
/// are replaced with timed delays.  Total wall-clock time therefore
/// includes real filesystem read time + simulated device time.
pub struct SimulatedTransport {
    speed: Arc<SpeedConfig>,
    device_vars: HashMap<String, String>,
    pub(crate) commands: Vec<String>,
    /// Accumulated downloaded bytes across all cycles (for metrics).
    total_downloaded: u64,
    /// Size of the last download payload — used by `flash()` to compute
    /// flash write delay.
    last_download_size: u32,
}

impl SimulatedTransport {
    /// Create a transport with the given device variable responses.
    #[must_use]
    pub fn new(device_vars: HashMap<String, String>) -> Self {
        Self {
            speed: Arc::new(SpeedConfig::default()),
            device_vars,
            commands: Vec::new(),
            total_downloaded: 0,
            last_download_size: 0,
        }
    }

    /// Build a transport whose `get_var` responses are seeded from a
    /// parsed scatter file: `partition-type:` and `partition-size:`
    /// for every partition, plus common device properties.
    #[must_use]
    pub fn from_scatter(scatter: &ScatterFile) -> Self {
        let mut vars = HashMap::new();

        vars.insert("max-download-size".into(), "0x10000000".into());
        vars.insert(
            "product".into(),
            format!(
                "SIM_{}",
                scatter.platform.as_deref().unwrap_or("MTK"),
            ),
        );
        vars.insert("serialno".into(), "SIM000001".into());
        vars.insert("version".into(), "0.5".into());
        vars.insert("current-slot".into(), "a".into());
        vars.insert("is-userspace".into(), "yes".into());

        for partitions in scatter.layouts.values() {
            for part in partitions {
                let name = &part.name;
                vars.insert(
                    format!("partition-type:{name}"),
                    part.image_type.clone().unwrap_or_else(|| "raw".into()),
                );
                vars.insert(
                    format!("partition-size:{name}"),
                    format!("{:#x}", part.size),
                );
            }
        }

        Self::new(vars)
    }

    /// Return the simulated device variables.
    #[must_use]
    pub const fn device_vars(&self) -> &HashMap<String, String> {
        &self.device_vars
    }
}

#[async_trait]
impl FlashTransport for SimulatedTransport {
    async fn get_var(&mut self, var: &str) -> Result<String> {
        self.commands.push(format!("SIM get_var:{var}"));
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.device_vars.get(var).cloned().ok_or_else(|| {
            crate::flash::error::FlashError::ActionFailed {
                partition: var.to_string(),
                reason: format!("simulated: no value configured for '{var}'"),
            }
        })
    }

    async fn get_all_vars(&mut self) -> Result<HashMap<String, String>> {
        self.commands.push("SIM get_all_vars".into());
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(self.device_vars.clone())
    }

    async fn download(&mut self, size: u32) -> Result<DownloadSender<'_>> {
        self.commands.push(format!("SIM download:{size}"));
        self.last_download_size = size;
        Ok(DownloadSender::Simulated(SimulatedDownloadSink::new(
            self.speed.clone(),
        )))
    }

    async fn flash(&mut self, partition: &str) -> Result<String> {
        self.commands.push(format!("SIM flash:{partition}"));

        let write_size = u64::from(self.last_download_size);
        self.total_downloaded += write_size;

        let delay = bytes_to_delay(write_size, self.speed.flash_bytes_per_sec);
        if delay > Duration::from_millis(1) {
            tokio::time::sleep(delay).await;
        } else {
            tokio::time::sleep(self.speed.command_latency).await;
        }

        Ok(format!("OKAY flashing {partition}"))
    }

    async fn erase(&mut self, partition: &str) -> Result<String> {
        self.commands.push(format!("SIM erase:{partition}"));
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(format!("OKAY erased {partition}"))
    }

    async fn reboot(&mut self) -> Result<String> {
        self.commands.push("SIM reboot".into());
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok("OKAY".into())
    }

    async fn reboot_to(&mut self, target: &str) -> Result<String> {
        self.commands.push(format!("SIM reboot_to:{target}"));
        tokio::time::sleep(Duration::from_secs(2)).await;
        Ok("OKAY".into())
    }

    async fn is_logical(&mut self, partition: &str) -> Result<bool> {
        self.commands.push(format!("SIM is_logical:{partition}"));
        Ok(partition == "metadata" || partition == "userdata" || partition == "cache")
    }

    async fn resize_logical_partition(&mut self, partition: &str, _size: u64) -> Result<()> {
        self.commands
            .push(format!("SIM resize_logical:{partition}:{_size}"));
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }

    async fn flashing(&mut self, cmd: &str) -> Result<String> {
        self.commands.push(format!("SIM flashing:{cmd}"));
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(format!("OKAY flashing {cmd}"))
    }

    async fn set_active(&mut self, slot: &str) -> Result<String> {
        self.commands.push(format!("SIM set_active:{slot}"));
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(format!("OKAY set_active {slot}"))
    }

    async fn snapshot_update(&mut self, cmd: &str) -> Result<String> {
        self.commands.push(format!("SIM snapshot_update:{cmd}"));
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(format!("OKAY snapshot_update {cmd}"))
    }
}
