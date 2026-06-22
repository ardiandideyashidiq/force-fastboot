/// CLI argument types.
pub mod args;
/// Force-fastboot handshake loop CLI handler.
pub mod force_fastboot;
/// Scatter parse/plan CLI handler.
pub mod scatter;
/// Flash execution CLI handler.
pub mod flash;
/// Fastboot device operations CLI handler.
pub mod device;

/// Initialize stderr-only tracing for CLI commands.
pub fn init_stderr_logging(level: &str) {
    use tracing_subscriber::{fmt, prelude::*, registry::Registry, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

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
