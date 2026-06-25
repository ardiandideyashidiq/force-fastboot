# Plan 003: Add USB device selection confirmation before destructive operations

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/flash/executor.rs src/cli/args.rs`
> If these files changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

`FlashExecutor::connect()` takes the first fastboot device found by USB enumeration — the order is determined by the kernel/driver, not by the user. If multiple devices are connected (e.g. a development phone + a testing device, or a malicious USB device), the tool silently selects the wrong one and performs destructive operations (erase, format, flash) on it. A one-line warning ("multiple devices found — using the first one") is insufficient.

## Current state

`src/flash/executor.rs:48-65`:

```rust
let all: Vec<_> = fastboot_protocol::nusb::devices()
    .await
    .map_err(|_| FlashError::NoDevice)?
    .collect();

if all.len() > 1 {
    warn!(
        count = all.len(),
        "multiple fastboot devices found – using the first one; \
         disconnect extras to avoid targeting the wrong device"
    );
}

let info = all.into_iter().next().ok_or_else(|| { ... })?;
```

There is no `--serial` flag on the CLI to specify an expected device by serial number. The `FlashExecutor` stores device variables (including `serialno`) at `executor.rs:39` as `device_vars: HashMap<String, String>` but only uses it for display and for `verify_device` (which checks `product`, not serial).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/cli/args.rs` — add `--serial` flag to relevant subcommands
- `src/flash/executor.rs` — modify `connect()` to accept an optional expected serial; verify device identity
- Callers of `FlashExecutor::connect()` in `src/cli/*.rs` — pass through the `--serial` flag

**Out of scope** (do NOT touch):
- Interactive prompt for device selection when multiple devices are present (deferred — requires `inquire` in a non-CLI context). Instead, require `--serial` when multiple devices exist.
- The `verify_device` method (it checks platform/product; leave it as-is)

## Git workflow

- Branch: `advisor/003-add-usb-device-confirmation`
- Commit message: `feat: add --serial flag to specify expected fastboot device`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add `--serial` argument to relevant CLI subcommands

In `src/cli/args.rs`, add a `--serial` flag to the `Flash` subcommand (and any other subcommand that calls `FlashExecutor::connect`). The cleanest approach is to add it as a `Cli`-level global argument so all subcommands inherit it.

Under `pub struct Cli`, add:
```rust
/// Expected device serial number; when set, verifies the connected device
/// matches (and skips the device if it doesn't).
#[arg(long, global = true)]
pub serial: Option<String>,
```

This makes `--serial` available on all subcommands. `global = true` means it can appear anywhere on the command line.

**Verify**: `cargo build` exits 0.

### Step 2: Modify `FlashExecutor::connect()` to accept optional serial

Change the `connect` method signature. Add a parameter:

```rust
pub async fn connect(expected_serial: Option<&str>) -> Result<Self> {
```

Inside `connect`, after getting `device_vars` (around line 94-111, where individual `get_var` calls happen if `getvar:all` failed), add a serial verification step:

```rust
if let Some(ref expected) = expected_serial {
    let actual = device_vars.get("serialno").map(String::as_str);
    match actual {
        Some(s) if s == expected => {
            debug!(serial = %s, "device serial matches expected");
        }
        Some(s) => {
            return Err(FlashError::DeviceMismatch {
                expected: expected.to_string(),
                actual: s.to_string(),
            });
        }
        None => {
            warn!("--serial set but device did not report serialno; proceeding anyway");
        }
    }
}
```

Place this immediately after the `device_vars` are populated (after line 111, before the `info!` log at line 113).

Also, update the multi-device warning at lines 53-59: when `expected_serial` is `Some`, filter devices by serial instead of warning:

```rust
let all: Vec<_> = fastboot_protocol::nusb::devices()
    .await
    .map_err(|_| FlashError::NoDevice)?
    .filter(|info| {
        expected_serial.map_or(true, |expected| {
            info.serial_number() == Some(expected)
        })
    })
    .collect();
```

This way, when `--serial` is provided, only devices with matching serials are considered. If none match, `NoDevice` is returned. If multiple match (unlikely but possible with duplicate serials), the existing warning triggers.

**Verify**: `cargo build` exits 0.

### Step 3: Update all callers of `FlashExecutor::connect()`

Search for all calls to `FlashExecutor::connect()` in `src/`:

```
$ grep -rn "FlashExecutor::connect" src/
```

You'll find callers in:
- `src/cli/flash.rs` — `FlashExecutor::connect()`
- `src/cli/format_data.rs` — `FlashExecutor::connect()`
- `src/cli/disable_vbmeta.rs` — `FlashExecutor::connect()`
- `src/cli/gsi.rs` — `FlashExecutor::connect()`
- `src/cli/interactive.rs` — `FlashExecutor::connect()`
- `src/cli/device.rs` — `FlashExecutor::connect()`
- `src/flash/executor.rs` — `wait_for_device` calls `Self::connect()`

For each, pass the `serial` from the CLI args. The pattern for the CLI callers is:

```rust
FlashExecutor::connect(cli.serial.as_deref()).await?
```

For `wait_for_device` (executor.rs:288), pass `None` since this is a reconnection wait (device may have rebooted, serial is preserved in the USB descriptor but not re-verified).

**Verify**: `cargo build` exits 0.

### Step 4: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

No new tests required — the change is in hardware-interaction code that can't be easily unit-tested without a mock USB layer. The existing test suite acts as a regression check.

A manual verification: connect a real fastboot device, run:
```
pawflash device info --serial <correct_serial>   # should succeed
pawflash device info --serial WRONG_SERIAL        # should fail with DeviceMismatch
pawflash device info                              # should succeed (no filter)
```

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `--serial <serial>` flag is accepted by all subcommands
- [ ] `FlashExecutor::connect()` accepts `Option<&str>` parameter
- [ ] When `--serial` is set and no device matches, `NoDevice` error is returned
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` fails after the changes.
- `cargo test` reports any test failure (pre-existing or induced).
- The `nusb::DeviceInfo::serial_number()` return type differs from `Option<&str>` (check the vendored fastboot-rs API).

## Maintenance notes

- The `--serial` flag is a simple filter; future work could add interactive device selection via `inquire::Select` when multiple devices match.
- `FlashExecutor::wait_for_device` passes `None` for expected_serial, which is correct — after a reboot, the serial is the same but we don't want to fail on the rare case where the USB descriptor temporarily returns a different value.
- If a device doesn't report a serial number (some early or broken bootloaders), `--serial` will not filter it out — the code warns and proceeds.
