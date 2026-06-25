//! Linux udev rule management and group-adding for MediaTek preloader serial ports.

use std::process::Command;
use tracing::warn;

const RULE_PATH: &str = "/etc/udev/rules.d/99-mediatek-preloader.rules";

const MEDIATEK_UDEV_RULES: &str = r#"# MediaTek Preloader / BROM / Download Agent
# IDs: 0e8d:2000 preloader, 0e8d:0003 DA/BROM

SUBSYSTEM=="usb", ATTR{idVendor}=="0e8d", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="tty", ATTRS{idVendor}=="0e8d", MODE="0666", TAG+="uaccess"
"#;

/// Return the udev rules content for `MediaTek` devices.
#[must_use]
pub const fn udev_rules_content() -> &'static str {
    MEDIATEK_UDEV_RULES
}

// ── Linux implementation ─────────────────────────────────────────
#[cfg(target_os = "linux")]
mod platform {
    use super::{warn, RULE_PATH, MEDIATEK_UDEV_RULES, Command};

    /// Install udev rules via `sudo tee`. Returns `true` if rules were
    /// written (or already up-to-date).
    pub fn install_udev_rules() -> bool {
        let existing = std::fs::read_to_string(RULE_PATH).ok();
        let rules = MEDIATEK_UDEV_RULES;

        if existing.as_deref() == Some(rules) {
            return true;
        }

        warn!("Normal user cannot open preloader serial port. Installing udev rules with sudo.");

        let written = Command::new("sudo")
            .args(["tee", RULE_PATH])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(rules.as_bytes())?;
                }
                child.wait()
            })
            .is_ok_and(|status| status.success());

        if !written {
            warn!("Failed to write udev rules to {RULE_PATH}");
            return false;
        }

        let _ = Command::new("sudo")
            .args(["udevadm", "control", "--reload-rules"])
            .status();
        let _ = Command::new("sudo")
            .args(["udevadm", "trigger"])
            .status();

        warn!("udev rules installed. Reconnect the device if the port still has old permissions.");
        true
    }

    /// Add the current user to dialout (and optionally plugdev) group.
    /// Returns `true` if any group was updated.
    pub fn add_user_to_group() -> bool {
        let Ok(user) = std::env::var("USER") else { return false };

        for group in &["dialout", "plugdev"] {
            let already_in = Command::new("id")
                .arg("-nG")
                .arg(&user)
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .is_some_and(|s| s.split_whitespace().any(|g| g == *group));

            if already_in {
                continue;
            }

            if Command::new("sudo")
                .args(["usermod", "-aG", group, &user])
                .status()
                .is_ok_and(|s| s.success())
            {
                warn!(
                    "Added user '{user}' to group '{group}'. Log out and back in for this to take effect."
                );
                return true;
            }
        }

        false
    }
}

// ── Non-Linux stubs ─────────────────────────────────────────────
#[cfg(not(target_os = "linux"))]
mod platform {
    /// Non-Linux: no udev to install. Returns `false`.
    pub fn install_udev_rules() -> bool {
        warn!("udev rule installation is only supported on Linux");
        false
    }

    /// Non-Linux: no usermod. Returns `false`.
    pub fn add_user_to_group() -> bool {
        false
    }
}

/// Print manual setup instructions to the user.
pub fn print_manual_guidance() {
    #[cfg(target_os = "linux")]
    {
        let user = std::env::var("USER").unwrap_or_else(|_| "<your-username>".into());
        warn!(
            "Permission denied opening serial port.\n\
             Install udev rules manually:\n\
             sudo tee {RULE_PATH} >/dev/null <<'EOF'\n{MEDIATEK_UDEV_RULES}EOF\n\
             sudo udevadm control --reload-rules\n\
             sudo udevadm trigger\n\n\
             Or add yourself to the dialout group:\n\
             sudo usermod -a -G dialout {user}\n\
             Then log out and back in."
        );
    }

    #[cfg(target_os = "windows")]
    {
        warn!(
            "Permission denied opening serial port on Windows.\n\
             Install the WinUSB driver using Zadig (https://zadig.akeo.ie)."
        );
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        warn!(
            "Permission denied opening serial port on {}.\n\
             Run the tool as root or fix permissions for the serial device.",
            std::env::consts::OS
        );
    }
}

pub use platform::{install_udev_rules, add_user_to_group};
