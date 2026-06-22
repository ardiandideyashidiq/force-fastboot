mod fastboot;
mod serial;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tracing::{info, warn};

const LOG_FILE: &str = "handshake.log";

fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let file_appender = tracing_appender::rolling::never(".", LOG_FILE);
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .init();

    guard
}

fn main() -> Result<()> {
    let _log_guard = init_logging();

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    if fastboot::in_fastboot_mode() {
        pb.finish_with_message("Already in fastboot mode — no handshake needed");
        info!("Already in fastboot mode — no handshake needed");
        return Ok(());
    }

    info!("Waiting for preloader...");
    pb.set_message("Waiting for preloader...");

    let mut port = serial::wait_for_preloader(false, &pb)?
        .context("preloader wait returned without a port")?;

    pb.println(format!("Found preloader on {port}"));
    info!("Found preloader on {port}");

    let mut dev = serial::open_serial(&port)?;
    let mut count: u64 = 0;
    let start = Instant::now();

    loop {
        match dev.write_all(b"FASTBOOT") {
            Ok(()) => {
                let _ = dev.flush();
                count += 1;

                if count.is_multiple_of(5) {
                    info!("Sent FASTBOOT x{count}");
                }
            }
            Err(err) => {
                warn!("serial write failed on {port}: {err}");

                if fastboot::in_fastboot_mode() {
                    break;
                }

                drop(dev);
                pb.set_message("Port lost, waiting for reconnect or fastboot...");
                info!("Port lost, waiting for reconnect or fastboot...");

                match serial::wait_for_preloader(true, &pb)? {
                    Some(new_port) => {
                        port = new_port;
                        pb.println(format!("Reconnected on {port}"));
                        info!("Reconnected on {port}");
                        dev = serial::open_serial(&port)?;
                        continue;
                    }
                    None => break,
                }
            }
        }

        if fastboot::in_fastboot_mode() {
            break;
        }

        let elapsed = start.elapsed().as_secs_f32();
        pb.set_message(format!(
            "Sending FASTBOOT... sends={count} elapsed={elapsed:.1}s"
        ));

        sleep(Duration::from_millis(500));
    }

    let elapsed = start.elapsed().as_secs_f32();
    pb.finish_with_message(format!(
        "Fastboot mode detected after {count} sends in {elapsed:.1}s"
    ));
    info!("Fastboot mode detected after {count} sends in {elapsed:.1}s");

    fs::write("comport.txt", &port)
        .with_context(|| format!("failed to write comport.txt with port {port}"))?;

    println!("Handshake complete");
    info!("Handshake complete");

    Ok(())
}
