# Plan 011: Wire up orphaned `cancel_token` in GSI flash

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/gsi/ src/cli/gsi.rs src/cli/args.rs`
> If these files changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW (additive change; no existing behavior is modified)
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

The `GsiFlashOptions` struct has a `cancel_token: Option<Arc<AtomicBool>>` field that is never read or wired into any loop or step check. Users running a GSI flash have no way to cancel mid-operation — they must kill the process, potentially leaving the device in an inconsistent state (e.g., after vbmeta disable but before userdata wipe). Wiring up the existing token gives users a safe abort path.

## Current state

`src/gsi/types.rs:76-81`:
```rust
pub struct GsiFlashOptions {
    pub wipe_data: bool,
    pub cancel_token: Option<Arc<AtomicBool>>,  // defined but never read
}
```

`src/gsi/flash.rs:192-313` — `execute_gsi_flash()` takes no cancel token parameter and never checks one. The function signature is:
```rust
pub async fn execute_gsi_flash(
    mut executor: FlashExecutor,
    image: &Path,
    clean_test: bool,
    mut user_report: impl FnMut(GsiEvent),
) -> Result<GsiFlashOutcome>
```

`src/cli/gsi.rs:33` — the CLI handler calls:
```rust
let outcome = crate::gsi::execute_gsi_flash(executor, &image, clean_test, report).await?;
```

No cancel token is passed or created.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/gsi/types.rs` — add `GsiFlashOptions` as a parameter struct (already exists; make it used)
- `src/gsi/flash.rs` — add cancel_token checking in `execute_gsi_flash`, `flash_system_and_product`
- `src/cli/gsi.rs` — accept `--cancel` or create a signal handler; or simpler: just thread the option through as None (wiring completion, not CLI exposure)
- `src/cli/args.rs` — optionally add `--cancel-file` or similar

**Out of scope** (do NOT touch):
- The fastboot download→flash loop in `src/flash/` (sparse.rs, executor.rs) — cancel in the GSI workflow checks between steps, not mid-download
- The existing `GsiFlashOptions` struct's `wipe_data` field — it's already unused (`clean_test` flag serves the same purpose) but leave it for now
- Actually: just wire the token into `execute_gsi_flash`; don't add CLI args yet — the simplest first step is to pass `None` from the CLI handler, making the infrastructure available for a future `--cancel-file` or Ctrl+C handler

## Git workflow

- Branch: `advisor/011-wire-up-gsi-cancel-token`
- Commit message: `feat: wire cancel_token parameter into GSI flash workflow steps`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add `cancel_token` parameter to `execute_gsi_flash`

Change the function signature in `src/gsi/flash.rs`:

Before:
```rust
pub async fn execute_gsi_flash(
    mut executor: FlashExecutor,
    image: &Path,
    clean_test: bool,
    mut user_report: impl FnMut(GsiEvent),
) -> Result<GsiFlashOutcome>
```

After:
```rust
pub async fn execute_gsi_flash(
    mut executor: FlashExecutor,
    image: &Path,
    clean_test: bool,
    cancel_token: Option<Arc<AtomicBool>>,
    mut user_report: impl FnMut(GsiEvent),
) -> Result<GsiFlashOutcome>
```

Add `use std::sync::Arc;` and `use std::sync::atomic::AtomicBool;` to the imports at the top of `flash.rs`.

**Verify**: `cargo build` exits 0.

### Step 2: Add cancel checks between each workflow step

In `src/gsi/flash.rs`, add a helper function:

```rust
fn check_cancelled(cancel_token: &Option<Arc<AtomicBool>>) -> Result<()> {
    if let Some(token) = cancel_token {
        if token.load(std::sync::atomic::Ordering::Relaxed) {
            anyhow::bail!("GSI flash cancelled by user");
        }
    }
    Ok(())
}
```

Then insert `check_cancelled(&cancel_token)?;` before each major step in both paths:

1. Before `executor.flash_empty_vbmeta().await?;` (both paths)
2. Before `executor.format_data(...).await;` (both paths)
3. Before `transition_mode(...)` (both paths)
4. Before `resolve_system_partition(...)` (both paths — called inside `flash_system_and_product`)
5. Before `executor.flash_raw_image(...)` calls (both inside `flash_system_and_product`)

Also check in `flash_system_and_product` (lines 331-388) at the top and before each `resize_logical_partition`/`flash_raw_image` call.

**Verify**: `cargo build` exits 0.

### Step 3: Add `cancel_token` parameter to `flash_system_and_product`

The shared helper `flash_system_and_product` is called from both paths. Add `cancel_token: Option<Arc<AtomicBool>>` to its parameter list and add `check_cancelled` calls at the function entry and before each device operation:

```rust
async fn flash_system_and_product(
    executor: &mut FlashExecutor,
    image: &Path,
    gsi_expanded_size: u64,
    system_partition: &str,
    product_overflow_size: u64,
    tools_root: &Path,
    cancel_token: Option<Arc<AtomicBool>>,
    report: &mut impl FnMut(GsiEvent),
) -> Result<()>
```

**Verify**: `cargo build` exits 0.

### Step 4: Update the CLI handler in `src/cli/gsi.rs`

Change the call to `execute_gsi_flash` to pass `None` for now:

```rust
let outcome = crate::gsi::execute_gsi_flash(
    executor,
    &image,
    clean_test,
    None,  // cancel_token — not wired to CLI yet
    report,
).await?;
```

**Verify**: `cargo build` exits 0.

### Step 5: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

The existing test suite passes. No new tests needed — the cancel token is a thread-safe flag that's checked between steps; with `None` passed from CLI, no behavior changes.

A manual verification would: create an `Arc<AtomicBool>`, set it to `true`, pass it to `execute_gsi_flash`, and confirm the workflow stops at the first `check_cancelled` call point.

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `execute_gsi_flash` accepts `Option<Arc<AtomicBool>>` as a parameter
- [ ] Cancel checks are placed before each workflow step (vbmeta, wipe, transition, partition query, flash)
- [ ] `cli/gsi.rs` passes `None` as the cancel_token
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` fails after the changes.
- `cargo test` reports any test failure.
- The `check_cancelled` helper somehow blocks the normal (non-cancelled) flow — it must check `if let Some(token) = cancel_token` before loading, so `None` means no-op.

## Maintenance notes

- After this plan, a future PR can connect the cancel token to a Ctrl+C signal handler or a `--cancel-file` CLI flag.
- The check is between steps, not mid-download — cancelling during a large sparse image download will still wait for the current download to complete. That's acceptable for a v1 implementation.
- The same pattern could extend to regular (non-GSI) flash operations in the future.
