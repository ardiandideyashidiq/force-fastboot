use super::error::Error;
use super::error::Result;
use super::fastboot::in_fastboot_mode;
use super::{permissions, udev};
use std::collections::HashSet;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, trace, warn};

const BAUD: u32 = 115_200;
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const PORT_TIMEOUT: Duration = Duration::from_millis(250);

pub fn serial_ports() -> HashSet<String> {
    let ports = match tokio_serial::available_ports() {
        Ok(ports) => ports,
        Err(err) => {
            warn!(%err, "failed to enumerate serial ports");
            return HashSet::new();
        }
    };

    ports
        .into_iter()
        .filter_map(|p| {
            if is_candidate_serial_port(&p.port_name) {
                Some(p.port_name)
            } else {
                trace!(port = %p.port_name, "skipping non-candidate serial port");
                None
            }
        })
        .collect()
}

fn is_candidate_serial_port(name: &str) -> bool {
    if cfg!(target_os = "windows") {
        name.to_ascii_uppercase().starts_with("COM")
    } else if cfg!(target_os = "linux") {
        name.starts_with("/dev/ttyACM") || name.starts_with("/dev/ttyUSB")
    } else {
        false
    }
}

/// Open a serial port to the preloader.
///
/// # Errors
///
/// Returns an error if the port cannot be opened.
pub fn open_serial(port: &str) -> Result<tokio_serial::SerialStream> {
    use tokio_serial::SerialPortBuilderExt;
    debug!(%port, baud = BAUD, "opening serial port");
    tokio_serial::new(port, BAUD)
        .timeout(PORT_TIMEOUT)
        .open_native_async()
        .map_err(|source| Error::OpenSerialPort {
            port: port.to_owned(),
            source,
        })
        .inspect(|_| info!(%port, "serial port opened"))
}

/// Open a serial port with automatic permission recovery.
///
/// On permission denied, attempts to install udev rules and add the user
/// to the dialout group before retrying.
///
/// # Errors
///
/// Returns an error if the port cannot be opened even after recovery
/// attempts.
pub fn open_with_permission_recovery(port: &str) -> Result<tokio_serial::SerialStream> {
    match open_serial(port) {
        Ok(stream) => return Ok(stream),
        Err(err) => {
            if !permissions::is_permission_error(&err) {
                return Err(err);
            }
        }
    }

    warn!(%port, "permission denied — attempting recovery");

    if udev::install_udev_rules() {
        if let Ok(stream) = open_serial(port) {
            info!(%port, "reconnected after udev rule install");
            return Ok(stream);
        }
    }

    if udev::add_user_to_group() {
        if let Ok(stream) = open_serial(port) {
            info!(%port, "reconnected after group add");
            return Ok(stream);
        }
    }

    udev::print_manual_guidance();

    // Re-wrap the original error
    open_serial(port)
}

/// Wait for a new preloader serial port to appear.
///
/// # Errors
///
/// Returns an error if serial port enumeration fails or the timeout (30s)
/// is exceeded.
pub async fn wait_for_preloader(
    check_fastboot: bool,
) -> Result<Option<String>> {
    info!(check_fastboot, "waiting for preloader serial port (max 30s)");
    let mut old = serial_ports();
    let mut iterations = 0u64;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    loop {
        if tokio::time::Instant::now() >= deadline {
            warn!("timed out waiting for preloader serial port after 30s");
            return Err(Error::PreloaderTimeout);
        }

        iterations += 1;
        trace!(iterations, ports = ?old, "polling for new serial port");

        if check_fastboot && in_fastboot_mode().await {
            info!("fastboot detected while waiting for preloader, returning None");
            return Ok(None);
        }

        let new = serial_ports();

        if let Some(port) = new.difference(&old).next().cloned() {
            info!(%port, iterations, "new preloader serial port appeared");
            return Ok(Some(port));
        }

        if old.difference(&new).next().is_some() {
            debug!("serial port set changed, refreshing baseline");
            old = new;
        }

        sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_candidate_serial_port_should_accept_linux_acm() {
        assert!(is_candidate_serial_port("/dev/ttyACM0"));
    }

    #[test]
    fn is_candidate_serial_port_should_reject_bogus_linux_path() {
        assert!(!is_candidate_serial_port("/dev/ttyS0"), "ttyS is not a preloader candidate");
    }

    #[test]
    fn is_candidate_serial_port_should_reject_empty() {
        assert!(!is_candidate_serial_port(""));
    }

    #[test]
    fn serial_ports_should_not_panic_when_no_ports() {
        let _ports = serial_ports();
    }

    #[tokio::test]
    async fn open_serial_should_error_on_bogus_port() {
        let err = open_serial("/dev/__force_fastboot_nonexistent__").unwrap_err();
        assert!(
            err.to_string().contains("/dev/__force_fastboot_nonexistent__"),
            "error should mention the port name: {err}",
        );
    }
}
