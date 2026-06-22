use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, info, trace, warn};

use crate::force_fastboot::{fastboot, serial};
use crate::output;

/// Force a `MediaTek` device into fastboot mode via preloader handshake.
///
/// # Errors
///
/// Returns an error if no preloader serial port is found, the serial
/// port cannot be opened, or the handshake otherwise fails.
pub async fn run() -> Result<()> {
    let start_all = Instant::now();
    info!("starting");

    if fastboot::in_fastboot_mode().await {
        info!("already in fastboot mode — no handshake needed");
        fastboot::list_fastboot_devices().await;
        info!(total_secs = start_all.elapsed().as_secs_f32(), sends = 0u64, "force-fastboot complete");
        return Ok(());
    }

    info!("waiting for preloader serial port");

    let mut port = serial::wait_for_preloader(false).await?
        .context("preloader wait returned without a port")?;

    info!(%port, "found preloader");

    let mut dev = serial::open_serial(&port)?;
    let mut count: u64 = 0;
    let start = Instant::now();

    let spinner = output::spinner::start("Waiting for preloader handshake...");

    loop {
        trace!(sends = count, elapsed = ?start.elapsed(), "writing FASTBOOT");
        match dev.write_all(b"FASTBOOT").await {
            Ok(()) => {
                let _ = dev.flush().await;
                count += 1;

                if count % 5 == 0 {
                    debug!(sends = count, "batch progress");
                }
            }
            Err(err) => {
                warn!(%err, %port, sends = count, "serial write failed");

                if fastboot::in_fastboot_mode().await {
                    debug!("fastboot mode detected after write failure");
                    break;
                }

                drop(dev);
                warn!(%port, "port lost, waiting for reconnect");

                if let Some(new_port) = serial::wait_for_preloader(true).await? {
                    port = new_port;
                    debug!(%port, "reconnected after port loss");
                    dev = serial::open_serial(&port)?;
                    continue;
                }

                debug!("preloader wait returned None — fastboot detected");
                break;
            }
        }

        if fastboot::in_fastboot_mode().await {
            debug!(sends = count, "fastboot mode detected in main loop");
            break;
        }

        sleep(Duration::from_millis(500)).await;
    }

    output::spinner::succeed(&spinner);

    let elapsed = start.elapsed().as_secs_f32();
    debug!(sends = count, elapsed_secs = elapsed, "handshake succeeded");
    debug!(sends = count, elapsed_secs = elapsed, "force-fastboot handshake loop exited");

    fastboot::list_fastboot_devices().await;
    info!(total_secs = start_all.elapsed().as_secs_f32(), sends = count, "force-fastboot complete");
    Ok(())
}
