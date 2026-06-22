use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tracing::{debug, info, trace, warn};
use tracing_subscriber::{fmt, prelude::*, registry::Registry, EnvFilter};

use crate::force_fastboot::{fastboot, serial};

const LOG_FILE: &str = "handshake.log";

/// Run the force-fastboot handshake loop.
///
/// Returns `Ok(())` on success or an error if something fails.
pub fn run(verbose: bool) -> Result<()> {
    let default_level = if verbose { "trace" } else { "info" };
    let _log_guard = init_logging(default_level);
    let start_all = Instant::now();

    info!("pawflash force-fastboot starting");

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .context("invalid progress template")?
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    if fastboot::in_fastboot_mode() {
        pb.finish_with_message("Already in fastboot mode — no handshake needed");
        info!("already in fastboot mode — no handshake needed");
        fastboot::list_fastboot_devices();
        summary_info(start_all, 0);
        pause_on_exit();
        return Ok(());
    }

    info!("waiting for preloader serial port");
    pb.set_message("Waiting for preloader...");

    let mut port = serial::wait_for_preloader(false, &pb)?
        .context("preloader wait returned without a port")?;

    pb.println(format!("Found preloader on {port}"));
    info!(%port, "found preloader");

    let mut dev = serial::open_serial(&port)?;
    let mut count: u64 = 0;
    let start = Instant::now();

    loop {
        trace!(sends = count, elapsed = ?start.elapsed(), "writing FASTBOOT");
        match dev.write_all(b"FASTBOOT") {
            Ok(()) => {
                let _ = dev.flush();
                count += 1;

                if count % 5 == 0 {
                    debug!(sends = count, "batch progress");
                }
            }
            Err(err) => {
                warn!(%err, %port, sends = count, "serial write failed");

                if fastboot::in_fastboot_mode() {
                    info!("fastboot mode detected after write failure");
                    break;
                }

                drop(dev);
                pb.set_message("Port lost, waiting for reconnect or fastboot...");
                warn!("port {port} lost, waiting for reconnect");

                if let Some(new_port) = serial::wait_for_preloader(true, &pb)? {
                    port = new_port;
                    pb.println(format!("Reconnected on {port}"));
                    info!(%port, "reconnected after port loss");
                    dev = serial::open_serial(&port)?;
                    continue;
                }

                info!("preloader wait returned None — fastboot detected");
                break;
            }
        }

        if fastboot::in_fastboot_mode() {
            info!(sends = count, "fastboot mode detected in main loop");
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
    info!(sends = count, elapsed_secs = elapsed, "handshake succeeded");

    fastboot::list_fastboot_devices();
    summary_info(start_all, count);
    pause_on_exit();
    Ok(())
}

fn init_logging(default_level: &str) -> tracing_appender::non_blocking::WorkerGuard {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level));

    let file_appender = tracing_appender::rolling::never(".", LOG_FILE);
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    let subscriber = Registry::default()
        .with(filter)
        .with(
            fmt::Layer::new()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_target(false)
                .with_level(true)
                .compact(),
        )
        .with(
            fmt::Layer::new()
                .with_writer(std::io::stderr)
                .with_ansi(true)
                .with_target(true)
                .with_level(true)
                .compact(),
        );

    tracing::subscriber::set_global_default(subscriber)
        .expect("init_logging called more than once");
    debug!("logging initialized, file={LOG_FILE}, default_level={default_level}");

    guard
}

fn summary_info(start_all: Instant, sends: u64) {
    let total = start_all.elapsed().as_secs_f32();
    info!(total_secs = total, sends, "force-fastboot complete");
}

#[cfg(windows)]
fn pause_on_exit() {
    use std::io::Read;
    println!("Press Enter to exit...");
    let _ = std::io::stdin().read(&mut [0u8]);
}

#[cfg(not(windows))]
const fn pause_on_exit() {}
