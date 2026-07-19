/// CLI argument types.
pub mod args;
/// Force-fastboot handshake loop CLI handler.
pub mod force_fastboot;
/// Flash unified handler (scatter show/plan/execute + raw image).
pub mod flash;
/// Fastboot device operations CLI handler.
pub mod device;
/// Disable-vbmeta CLI handler.
pub mod disable_vbmeta;

/// Interactive flash prompt flow.
pub mod interactive;

/// Initialize tracing subscriber and output verbosity.
pub fn init_logging(verbosity: u8) {
    use tracing_subscriber::filter::LevelFilter;
    use tracing_subscriber::{fmt, prelude::*, registry::Registry};

    pawflash_core::output::set_verbosity(verbosity);

    let level = match verbosity {
        0 => LevelFilter::ERROR,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };

    let subscriber = Registry::default()
        .with(level)
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
