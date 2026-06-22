use super::error::Error;
use super::error::Result;
use super::fastboot::in_fastboot_mode;
use std::collections::HashSet;
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, info, trace, warn};

/// Baud rate used for preloader serial communication.
const BAUD: u32 = 115_200;

/// Polling interval when waiting for a preloader port to appear.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Serial-port timeout for reads and writes.
const PORT_TIMEOUT: Duration = Duration::from_millis(250);

/// Return the set of candidate serial port names visible on the system.
///
/// A candidate is a port that looks like a preloader port (e.g. `COM*` on
/// Windows, `/dev/ttyACM*` or `/dev/ttyUSB*` on Linux).
pub fn serial_ports() -> HashSet<String> {
    let ports = match serialport::available_ports() {
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

/// Returns `true` when `name` looks like a plausible preloader serial port.
fn is_candidate_serial_port(name: &str) -> bool {
    if cfg!(target_os = "windows") {
        name.to_ascii_uppercase().starts_with("COM")
    } else if cfg!(target_os = "linux") {
        name.starts_with("/dev/ttyACM") || name.starts_with("/dev/ttyUSB")
    } else {
        false
    }
}

/// Open a serial port with the preloader baud rate and a short timeout.
///
/// # Errors
///
/// Returns [`Error::OpenSerialPort`] when the port cannot be opened (wrong
/// path, permissions, or the port disappeared).
pub fn open_serial(port: &str) -> Result<Box<dyn serialport::SerialPort>> {
    debug!(%port, baud = BAUD, "opening serial port");
    serialport::new(port, BAUD)
        .timeout(PORT_TIMEOUT)
        .open()
        .map_err(|source| Error::OpenSerialPort {
            port: port.to_owned(),
            source,
        })
        .inspect(|_| info!(%port, "serial port opened"))
}

/// Wait for a new preloader serial port to appear.
///
/// Polls the system serial ports every 250 ms. Returns `Some(port_name)` when
/// a new candidate port appears. When `check_fastboot` is `true`, also returns
/// `None` if fastboot mode is detected before a port is found.
///
/// # Errors
///
/// Propagates [`Error::OpenSerialPort`] if a detected preloader port cannot
/// be opened (though the call itself does not open the port — that is deferred
/// to [`open_serial`]).
pub fn wait_for_preloader(
    check_fastboot: bool,
) -> Result<Option<String>> {
    info!(check_fastboot, "waiting for preloader serial port");
    let mut old = serial_ports();
    let mut iterations = 0u64;

    loop {
        iterations += 1;
        trace!(iterations, ports = ?old, "polling for new serial port");

        if check_fastboot && in_fastboot_mode() {
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

        sleep(POLL_INTERVAL);
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
        // No hardware dependency — just checks error handling doesn't panic.
        let _ports = serial_ports();
    }

    #[test]
    fn open_serial_should_error_on_bogus_port() {
        let err = open_serial("/dev/__force_fastboot_nonexistent__").unwrap_err();
        assert!(
            err.to_string().contains("/dev/__force_fastboot_nonexistent__"),
            "error should mention the port name: {err}",
        );
    }
}
