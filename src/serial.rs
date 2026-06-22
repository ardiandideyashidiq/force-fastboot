use anyhow::{Context, Result};
use indicatif::ProgressBar;
use std::collections::HashSet;
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, info, trace, warn};

use crate::fastboot::in_fastboot_mode;

const BAUD: u32 = 115_200;

pub fn serial_ports() -> HashSet<String> {
    let ports = match serialport::available_ports() {
        Ok(ports) => ports,
        Err(err) => {
            warn!(%err, "failed to enumerate serial ports");
            return HashSet::new();
        }
    };

    let result: HashSet<String> = ports
        .into_iter()
        .filter_map(|p| {
            if is_candidate_serial_port(&p.port_name) {
                Some(p.port_name)
            } else {
                trace!(port = %p.port_name, "skipping non-candidate serial port");
                None
            }
        })
        .collect();

    debug!(candidates = %result.iter().cloned().collect::<Vec<_>>().join(", "), "serial port scan");
    result
}

fn is_candidate_serial_port(name: &str) -> bool {
    let candidate = if cfg!(target_os = "windows") {
        name.to_ascii_uppercase().starts_with("COM")
    } else if cfg!(target_os = "linux") {
        name.starts_with("/dev/ttyACM") || name.starts_with("/dev/ttyUSB")
    } else {
        false
    };
    trace!(port = %name, candidate, "serial port candidate check");
    candidate
}

pub fn open_serial(port: &str) -> Result<Box<dyn serialport::SerialPort>> {
    debug!(%port, baud = BAUD, "opening serial port");
    let port_handle = serialport::new(port, BAUD)
        .timeout(Duration::from_millis(250))
        .open()
        .with_context(|| format!("failed to open serial port {port}"))?;
    info!(%port, "serial port opened");
    Ok(port_handle)
}

pub fn wait_for_preloader(
    check_fastboot: bool,
    pb: &ProgressBar,
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

        pb.set_message("Waiting for preloader...");
        sleep(Duration::from_millis(250));
    }
}
