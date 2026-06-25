use owo_colors::OwoColorize;

use crate::output::{self, spinner};

fn strip(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for b in s.bytes() {
        if in_escape {
            if b == b'm' || b.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if b == b'\x1b' {
            in_escape = true;
        } else {
            out.push(b as char);
        }
    }
    out
}

pub fn warn_colored(msg: impl AsRef<str>) -> String {
    msg.as_ref().yellow().to_string()
}

pub fn error_colored(msg: impl AsRef<str>) -> String {
    msg.as_ref().red().bold().to_string()
}

pub fn dim_colored(msg: impl AsRef<str>) -> String {
    msg.as_ref().dimmed().to_string()
}

pub fn ok_colored(msg: impl AsRef<str>) -> String {
    msg.as_ref().green().to_string()
}

pub fn info_colored(msg: impl AsRef<str>) -> String {
    msg.as_ref().bright_blue().to_string()
}

pub fn heading_colored(msg: impl AsRef<str>) -> String {
    msg.as_ref().white().bold().to_string()
}

/// Print command data output (tables, JSON, device info) — always to stdout.
/// When `-v` is active, also emits via `tracing::info!` (ANSI-stripped).
pub fn data(output: impl AsRef<str>) {
    let out = output.as_ref();
    if output::verbosity() >= 1 {
        tracing::info!("{}", strip(out));
    }
    println!("{out}");
}

/// Print a success status line (e.g., `OKAY (resp)`).
pub fn ok(label: impl AsRef<str>, detail: impl AsRef<str>) {
    emit_status("info", ok_colored(label), detail.as_ref());
}

/// Print a warning status line.
pub fn warn(label: impl AsRef<str>, detail: impl AsRef<str>) {
    emit_status("warn", warn_colored(label), detail.as_ref());
}

/// Print a failure status line.
pub fn fail(label: impl AsRef<str>, detail: impl AsRef<str>) {
    emit_status("error", error_colored(label), detail.as_ref());
}

/// Print a dim/neutral status message (e.g., "Skipped partitions:").
pub fn dim(msg: impl AsRef<str>) {
    let colored = dim_colored(msg.as_ref());
    if output::verbosity() >= 1 {
        tracing::info!("{}", strip(&colored));
    }
    let _ = spinner::print(&format!("  {colored}"));
}

/// Print a section heading (e.g., "Flash Plan").
pub fn heading(msg: impl AsRef<str>) {
    let s = msg.as_ref();
    if output::verbosity() >= 1 {
        tracing::info!("{s}");
    }
    println!("{}", heading_colored(s));
}

/// Print a blank line as section separator.
pub fn blank() {
    if output::verbosity() >= 1 {
        tracing::info!("");
    }
    println!();
}

/// Print a block of text to stderr (for error details, flash results, etc.).
/// When `-v` is active, also emits via `tracing::error!` (ANSI-stripped).
pub fn stderr(output: impl AsRef<str>) {
    let out = output.as_ref();
    if output::verbosity() >= 1 {
        tracing::error!("{}", strip(out));
    }
    let _ = spinner::print(out);
}

fn emit_status(level: &str, label: String, detail: &str) {
    let msg = if detail.is_empty() {
        label
    } else {
        format!("  {label} ({detail})")
    };

    if output::verbosity() >= 1 {
        let plain = strip(&msg);
        match level {
            "error" => tracing::error!("{plain}"),
            "warn" => tracing::warn!("{plain}"),
            _ => tracing::info!("{plain}"),
        }
    }
    let _ = spinner::print(&msg);
}
