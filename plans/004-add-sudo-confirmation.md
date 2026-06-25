# Plan 004: Add user confirmation before sudo privilege escalation

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/force_fastboot/udev.rs src/force_fastboot/serial.rs src/output/prompts.rs`
> If these files changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW (adds a prompt; existing behavior is unchanged when confirmed)
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

When the tool fails to open a serial port due to permission errors, `serial::open_with_permission_recovery()` automatically runs `sudo tee /etc/udev/rules.d/...` and `sudo usermod -aG dialout <user>` without asking the user first. While the content written is safe (MediaTek preloader udev rules), the automatic privilege escalation is surprising and could be exploited by a compromised tool or a malicious process controlling the tool's environment.

## Current state

`src/force_fastboot/serial.rs:72-102` — `open_with_permission_recovery()`:

```rust
pub fn open_with_permission_recovery(port: &str) -> Result<tokio_serial::SerialStream> {
    match open_serial(port) {
        Ok(stream) => return Ok(stream),
        Err(err) => {
            if !permissions::is_permission_error(&err) {
                return Err(err);
            }
        }
    }

    warn!(%port, "permission denied — attempting recovery");

    if udev::install_udev_rules() {
        if let Ok(stream) = open_serial(port) {
            info!(%port, "reconnected after udev rule install");
            return Ok(stream);
        }
    }

    if udev::add_user_to_group() {
        if let Ok(stream) = open_serial(port) {
            info!(%port, "reconnected after group add");
            return Ok(stream);
        }
    }

    udev::print_manual_guidance();
    open_serial(port)
}
```

`src/force_fastboot/udev.rs:38-49` — `install_udev_rules()`:

```rust
let written = Command::new("sudo")
    .args(["tee", RULE_PATH])
    .stdin(std::process::Stdio::piped())
    // ...
```

`src/force_fastboot/udev.rs:89-92` — `add_user_to_group()`:

```rust
if Command::new("sudo")
    .args(["usermod", "-aG", group, &user])
    .status()
    // ...
```

The `output::prompts` module already has `confirm_yes` and `confirm_no` helpers in `src/output/prompts.rs:5-18`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/force_fastboot/udev.rs` — modify `install_udev_rules()` and `add_user_to_group()` to take a "confirmed" callback or return a "needs confirmation" signal
- `src/force_fastboot/serial.rs` — modify `open_with_permission_recovery()` to prompt the user before each escalation step

**Out of scope** (do NOT touch):
- The `is_permission_error` logic in `permissions.rs` — it works correctly
- `print_manual_guidance()` — keep as fallback
- Any Windows-specific code paths

## Git workflow

- Branch: `advisor/004-add-sudo-confirmation`
- Commit message: `fix: prompt user before sudo privilege escalation in serial port recovery`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add `confirm_or_skip` pattern in `open_with_permission_recovery`

In `src/force_fastboot/serial.rs`, modify `open_with_permission_recovery` to prompt the user before each escalation step using `crate::output::prompts::confirm_yes`.

Add `use crate::output::prompts;` to the imports at the top.

Then change the function body to insert prompts:

```rust
pub fn open_with_permission_recovery(port: &str) -> Result<tokio_serial::SerialStream> {
    match open_serial(port) {
        Ok(stream) => return Ok(stream),
        Err(err) => {
            if !permissions::is_permission_error(&err) {
                return Err(err);
            }
        }
    }

    warn!(%port, "permission denied — attempting recovery");

    // Prompt before installing udev rules
    if prompts::confirm_no(
        "Permission denied. Install udev rules for MediaTek preloader? (requires sudo)"
    ).unwrap_or(false) {
        if udev::install_udev_rules() {
            if let Ok(stream) = open_serial(port) {
                info!(%port, "reconnected after udev rule install");
                return Ok(stream);
            }
        }
    }

    // Prompt before adding user to dialout group
    if prompts::confirm_no(
        "Add current user to dialout/plugdev groups? (requires sudo, log out/in to take effect)"
    ).unwrap_or(false) {
        if udev::add_user_to_group() {
            if let Ok(stream) = open_serial(port) {
                info!(%port, "reconnected after group add");
                return Ok(stream);
            }
        }
    }

    udev::print_manual_guidance();
    open_serial(port)
}
```

Note: `confirm_no` (default "no") is used so the user must actively opt in.

**Verify**: `cargo build` exits 0.

### Step 2: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

The existing test suite continues to pass. The `serial.rs` tests are basic port-opening tests that don't exercise the permission recovery path. No new tests needed for this confirmation logic — testing interactive prompts requires a TTY or stdin mock which the project doesn't have infrastructure for.

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `open_with_permission_recovery` prompts before each sudo invocation
- [ ] When user declines, manual guidance is printed instead
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` fails after the changes.
- `cargo test` reports any test failure.
- The `output::prompts::confirm_no` function signature differs from `Result<bool>` (check it returns `anyhow::Result<bool>`).

## Maintenance notes

- The prompts use `confirm_no` (default "no") deliberately — opt-in for system-level changes.
- If a user declines both prompts, they see the manual guidance text and can follow the instructions printed by `print_manual_guidance()`.
- Future enhancement: add `--yes` flag to skip all prompts for headless/scripted use.
