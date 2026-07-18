/// CLI argument types.
pub mod args;
/// Force-fastboot handshake loop CLI handler.
pub mod force_fastboot;
/// Flash unified handler (scatter show/plan/execute + raw image).
pub mod flash;
/// Fastboot device operations CLI handler.
pub mod device;
/// Format-data CLI handler.
pub mod format_data;
/// Disable-vbmeta CLI handler.
pub mod disable_vbmeta;

/// Interactive flash prompt flow.
pub mod interactive;

/// Initialize tracing subscriber and output verbosity.
pub fn init_logging(verbosity: u8) {
    use tracing_subscriber::{fmt, prelude::*, registry::Registry, EnvFilter};

    pawflash_core::output::set_verbosity(verbosity);

    let level_str = match verbosity {
        0 => "error",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level_str));

    let subscriber = Registry::default()
        .with(filter)
        .with(
            fmt::Layer::new()
                .with_writer(std::io::stderr)
                .with_ansi(true)
                .with_target(true)
                .with_level(true)
                .compact(),
        );

    let _ = tracing::subscriber::set_global_default(subscriber);
}
