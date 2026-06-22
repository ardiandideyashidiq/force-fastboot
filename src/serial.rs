use anyhow::{Context, Result};
use indicatif::ProgressBar;
use std::collections::HashSet;
use std::thread::sleep;
use std::time::Duration;
use tracing::{info, warn};

use crate::fastboot::in_fastboot_mode;

const BAUD: u32 = 115_200;

pub fn serial_ports() -> HashSet<String> {
    let ports = match serialport::available_ports() {
        Ok(ports) => ports,
        Err(err) => {
            warn!("failed to enumerate serial ports: {err}");
            return HashSet::new();
        }
    };

    ports
        .into_iter()
        .filter_map(|p| {
            if is_candidate_serial_port(&p.port_name) {
                Some(p.port_name)
            } else {
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

pub fn open_serial(port: &str) -> Result<Box<dyn serialport::SerialPort>> {
    serialport::new(port, BAUD)
        .timeout(Duration::from_millis(250))
        .open()
        .with_context(|| format!("failed to open serial port {port}"))
}

pub fn wait_for_preloader(
    check_fastboot: bool,
    pb: &ProgressBar,
) -> Result<Option<String>> {
    let mut old = serial_ports();

    loop {
        if check_fastboot && in_fastboot_mode() {
            info!("fastboot detected while waiting for preloader");
            return Ok(None);
        }

        let new = serial_ports();

        if let Some(port) = new.difference(&old).next().cloned() {
            info!("new serial port appeared: {port}");
            return Ok(Some(port));
        }

        if old.difference(&new).next().is_some() {
            old = new;
        }

        pb.set_message("Waiting for preloader...");
        sleep(Duration::from_millis(250));
    }
}
