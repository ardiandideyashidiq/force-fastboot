use std::collections::HashMap;
use std::time::Duration;

use fastboot_protocol::nusb::NusbFastBoot;
use tracing::{debug, info, warn};

use crate::flash::error::{FlashError, Result};
use super::{BootTarget, expected_serial, FlashExecutor};

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
