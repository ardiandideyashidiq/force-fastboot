use std::sync::OnceLock;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

fn multi() -> &'static MultiProgress {
    static MP: OnceLock<MultiProgress> = OnceLock::new();
    MP.get_or_init(MultiProgress::new)
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {msg}")
        .unwrap()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
}

/// Start a new spinner with the given message.
#[must_use]
pub fn start(msg: &str) -> ProgressBar {
    let pb = multi().add(ProgressBar::new_spinner());
    pb.set_style(spinner_style());
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Mark a spinner as succeeded and clear.
pub fn succeed(pb: &ProgressBar) {
    pb.finish_and_clear();
}

/// Mark a spinner as failed and clear.
pub fn fail(pb: &ProgressBar) {
    pb.finish_and_clear();
}

/// Create a progress bar for flash/download operations.
#[allow(clippy::missing_panics_doc)]
#[must_use]
pub fn progress_bar(len: u64) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::with_template(
            "{prefix:>16}: [{bar:40.green/red}] {bytes}/{total_bytes}  {bytes_per_sec}  ETA {eta}  [{elapsed_precise}]",
        )
        .unwrap()
        .progress_chars("█▉▊▋▌▍▎▏ "),
    );
    pb
}

/// Print a line above any active progress bars. If no progress bar is active,
/// falls back to writing to stderr via the underlying `MultiProgress`.
///
/// # Errors
///
/// Returns an error if the underlying `MultiProgress` write fails.
pub fn print(msg: &str) -> std::io::Result<()> {
    multi().println(msg)
}

/// Run an async future while showing a spinner. Clears on completion.
pub async fn run_with_spinner<F, T>(msg: &str, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let pb = start(msg);
    let result = fut.await;
    succeed(&pb);
    result
}
