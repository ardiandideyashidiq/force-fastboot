use miette::{Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::time::{sleep, Duration, Instant};
use tracing::{debug, info, trace, warn};

use pawflash_core::force_fastboot::{fastboot, serial};
use pawflash_core::output;

/// Force a `MediaTek` device into fastboot mode via preloader handshake.
///
/// When `simulate` is true, replays a realistic 10-second handshake
/// without any serial port or USB interaction.
///
/// # Errors
///
/// Returns an error if no preloader serial port is found, the serial
/// port cannot be opened, or the handshake otherwise fails.
pub async fn run(simulate: bool) -> Result<()> {
    if simulate {
        return run_simulated().await;
    }
    let start_all = Instant::now();
    info!("starting");

    output::status::heading("Scanning USB for fastboot devices...");
    if fastboot::in_fastboot_mode().await {
        output::status::ok("[+]", "fastboot mode detected");
        fastboot::list_fastboot_devices().await;
        info!(total_secs = start_all.elapsed().as_secs_f32(), sends = 0u64, "force-fastboot complete");
        return Ok(());
    }

    let mut port = output::spinner::run_with_spinner(
        "Waiting for preloader serial port (120s timeout)...",
        serial::wait_for_preloader(false),
    )
    .await?
    .context("preloader wait returned without a port")?;

    output::status::ok("[+]", format!("{port} appeared"));
    output::status::blank();

    let mut dev = serial::open_with_permission_recovery(&port)?;
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
                output::status::warn("[!]", format!("{port} disconnected"));

                if fastboot::in_fastboot_mode().await {
                    debug!("fastboot mode detected after write failure");
                    break;
                }

                drop(dev);
                output::status::warn("[!]", format!("{port} lost, waiting for reconnect"));

                if let Some(new_port) = output::spinner::run_with_spinner(
                    "Waiting for device to reconnect...",
                    serial::wait_for_preloader(true),
                ).await? {
                    port = new_port;
                    output::status::ok("[+]", format!("{port} reconnected"));
                    debug!(%port, "reconnected after port loss");
                    dev = serial::open_with_permission_recovery(&port)?;
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
    output::status::blank();
    output::status::ok("[+]", format!("fastboot mode detected ({count} writes)"));
    debug!(sends = count, elapsed_secs = elapsed, "handshake succeeded");

    fastboot::list_fastboot_devices().await;
    info!(total_secs = start_all.elapsed().as_secs_f32(), sends = count, "force-fastboot complete");
    Ok(())
}

/// Simulated handshake: 5 stages with realistic timing and terminal output.
async fn run_simulated() -> Result<()> {
    output::status::heading("[!] SIMULATED MODE — no device will be touched");
    output::status::blank();

    // ── Stage 1: Check fastboot mode ─────────────────────────────────
    output::status::heading("[1/5] Checking for fastboot mode...");
    sleep(Duration::from_secs(1)).await;
    output::status::dim("  [x] no fastboot device found");
    output::status::blank();

    // ── Stage 2: Wait for preloader serial port ──────────────────────
    output::status::heading("[2/5] Waiting for preloader serial port...");
    let sp = output::spinner::start("Scanning serial ports...");
    for _ in 0..12 {
        sleep(Duration::from_millis(250)).await;
    }
    output::spinner::succeed(&sp);
    output::status::ok("[+]", "/dev/ttyACM0 appeared");
    output::status::blank();

    // ── Stage 3: Send handshake until disconnect ─────────────────────
    output::status::heading("[3/5] Sending FASTBOOT handshake...");
    for n in 1..=5 {
        sleep(Duration::from_millis(500)).await;
        output::status::dim(format!("  -> write #{n}"));
    }
    output::status::warn("[!]", "FASTBOOT write failed — device disconnected");
    output::status::blank();

    // ── Stage 4: Wait for reconnect ──────────────────────────────────
    output::status::heading("[4/5] Waiting for device to reconnect...");
    let sp = output::spinner::start("Scanning serial ports...");
    for _ in 0..8 {
        sleep(Duration::from_millis(250)).await;
    }
    output::spinner::succeed(&sp);
    output::status::ok("[+]", "/dev/ttyACM0 reconnected");
    output::status::blank();

    // ── Stage 5: Continue handshake -> fastboot mode ──────────────────
    output::status::heading("[5/5] Continuing handshake...");
    for n in 6..=10 {
        sleep(Duration::from_millis(500)).await;
        output::status::dim(format!("  -> write #{n}"));
    }
    sleep(Duration::from_millis(500)).await;

    output::status::blank();
    output::status::ok("[+]", "fastboot mode detected (simulated)");
    debug!(sends = 10u64, elapsed_secs = 11.0, "force-fastboot simulated handshake complete");
    info!(total_secs = 11.0_f32, sends = 10u64, "SIM force-fastboot complete");
    Ok(())
}
