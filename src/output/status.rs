use crate::output::{self, spinner, theme};

/// Print command data output (tables, JSON, device info) — always to stdout.
/// When `-v` is active, also emits via `tracing::info!`.
pub fn data(output: impl AsRef<str>) {
    if output::verbosity() >= 1 {
        tracing::info!("{}", output.as_ref());
    }
    println!("{}", output.as_ref());
}

/// Print a success status line (e.g., `OKAY (resp)`).
pub fn ok(label: impl AsRef<str>, detail: impl AsRef<str>) {
    emit_status("info", theme::ok(label), detail.as_ref());
}

/// Print a warning status line.
pub fn warn(label: impl AsRef<str>, detail: impl AsRef<str>) {
    emit_status("warn", theme::warn(label), detail.as_ref());
}

/// Print a failure status line.
pub fn fail(label: impl AsRef<str>, detail: impl AsRef<str>) {
    emit_status("error", theme::error(label), detail.as_ref());
}

/// Print a dim/neutral status message (e.g., "Skipped partitions:").
pub fn dim(msg: impl AsRef<str>) {
    let colored = theme::dim(msg.as_ref());
    if output::verbosity() >= 1 {
        tracing::info!("{colored}");
    }
    let _ = spinner::print(&format!("  {colored}"));
}

/// Print a section heading (e.g., "Flash Plan").
pub fn heading(msg: impl AsRef<str>) {
    let s = msg.as_ref();
    if output::verbosity() >= 1 {
        tracing::info!("{s}");
    }
    println!("{}", theme::heading(s));
}

/// Print a blank line as section separator.
pub fn blank() {
    if output::verbosity() >= 1 {
        tracing::info!("");
    }
    println!();
}

/// Print a block of text to stderr (for error details, flash results, etc.).
/// When `-v` is active, also emits via `tracing::error!`.
pub fn stderr(output: impl AsRef<str>) {
    let output = output.as_ref();
    if output::verbosity() >= 1 {
        tracing::error!("{output}");
    }
    let _ = spinner::print(output);
}

fn emit_status(level: &str, label: String, detail: &str) {
    let msg = if detail.is_empty() {
        label
    } else {
        format!("  {label} ({detail})")
    };

    if output::verbosity() >= 1 {
        match level {
            "error" => tracing::error!("{msg}"),
            "warn" => tracing::warn!("{msg}"),
            _ => tracing::info!("{msg}"),
        }
    }
    let _ = spinner::print(&msg);
}
