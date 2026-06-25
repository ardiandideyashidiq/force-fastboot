# Plan 006: Fix silent sentinel fallback in i64→u64 for `lseek` results

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/flash/sparse.rs`
> If this file changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: MEDIUM (changes to unsafe lseek call sites require careful verification)
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

In `scan_extents` (the sparse file extent scanner), `lseek` results are cast from `i64` (libc return type) to `u64` using `unwrap_or(u64::MAX)` as a sentinel. If `lseek` returns a position that legitimately exceeds `i64::MAX` (theoretical on some architectures, or with files >9 EB), the conversion silently produces `u64::MAX`, which propagates through the scan logic and can cause data regions to be silently dropped from the sparse image. The same pattern exists for `i64::try_from(u64).unwrap_or(i64::MAX)` feeding into `lseek` itself.

## Current state

`src/flash/sparse.rs:415-468` — the `do_scan` function (Unix `#[cfg(unix)]` branch):

```rust
let seek_offset = i64::try_from(offset).unwrap_or(i64::MAX);
let data_start = match unsafe { libc::lseek(fd, seek_offset, libc::SEEK_DATA) } {
    -1 => { /* handle ENXIO */ }
    pos => u64::try_from(pos).unwrap_or(u64::MAX),  // line 439: silent sentinel
};
// ...
let hole_seek = i64::try_from(aligned).unwrap_or(i64::MAX); // line 445
let hole_start = match unsafe { libc::lseek(fd, hole_seek, libc::SEEK_HOLE) } {
    -1 => { /* handle ENXIO */ }
    pos => u64::try_from(pos).unwrap_or(u64::MAX),  // line 459: silent sentinel
};
```

The problem: `lseek` returns `off_t` which is `i64` on 64-bit Linux. For files under 9 EB, `lseek` never returns a positive value exceeding `i64::MAX`. But the sentinel pattern is fragile: if a position ever exceeds `i64::MAX`, the `u64::try_from(pos).unwrap_or(u64::MAX)` converts it to `u64::MAX` without error, and subsequent arithmetic (aligned calc, saturating sub) silently produces zero-length extents.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/flash/sparse.rs` — change the four `unwrap_or` conversion sites to use explicit error handling

**Out of scope** (do NOT touch):
- The `scan_extents` function signature or behavior on normal-sized files
- The fallback `#[cfg(not(unix))]` branch (block-by-block read — separate concern)
- Any other file

## Git workflow

- Branch: `advisor/006-fix-i64-u64-sentinel-fallback`
- Commit message: `fix: replace silent sentinel fallbacks in sparse lseek conversions with explicit errors`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Fix `i64::try_from(offset).unwrap_or(i64::MAX)` — line 424

Replace:

```rust
let seek_offset = i64::try_from(offset).unwrap_or(i64::MAX);
```

with:

```rust
let seek_offset = i64::try_from(offset)
    .map_err(|_| FlashError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("seek offset {offset:#x} exceeds i64 range"),
    )))?;
```

This propagates the error through the `Result` return from `do_scan`, stopping the scan with a clear error instead of seeking to `i64::MAX`.

**Verify**: `cargo build` exits 0.

### Step 2: Fix `u64::try_from(pos).unwrap_or(u64::MAX)` — line 439 (data_start)

Replace the match arm:

```rust
pos => u64::try_from(pos).unwrap_or(u64::MAX),
```

with:

```rust
pos => u64::try_from(pos)
    .map_err(|_| FlashError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("SEEK_DATA returned position {pos} that exceeds u64 range"),
    )))?,
```

Note: on 64-bit Linux `off_t` is `i64`, so the maximum positive value is `i64::MAX` (~9.2 EB), which fits in `u64`. This conversion is infallible in practice, but making it explicit with `?` is correct.

**Verify**: `cargo build` exits 0.

### Step 3: Fix `i64::try_from(aligned).unwrap_or(i64::MAX)` — line 445

Replace:

```rust
let hole_seek = i64::try_from(aligned).unwrap_or(i64::MAX);
```

with:

```rust
let hole_seek = i64::try_from(aligned)
    .map_err(|_| FlashError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("hole seek offset {aligned:#x} exceeds i64 range"),
    )))?;
```

**Verify**: `cargo build` exits 0.

### Step 4: Fix `u64::try_from(pos).unwrap_or(u64::MAX)` — line 459 (hole_start)

Replace with the same error-propagation pattern as step 2.

**Verify**: `cargo build` exits 0.

### Step 5: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

The existing tests continue to pass. No new tests needed — the fix changes error handling from silent fallback to explicit propagation, and the code paths involved (sparse image extent scanning) require real hardware filesystem features (SEEK_DATA/SEEK_HOLE) that aren't easily unit-testable. The `cargo build` verification ensures the code compiles.

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] All four `unwrap_or(i64::MAX)` and `unwrap_or(u64::MAX)` conversion sites in `do_scan` are replaced with explicit error propagation
- [ ] `grep -n "unwrap_or.*MAX" src/flash/sparse.rs` returns no matches
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` fails after the changes.
- `cargo test` reports any test failure.
- You find that the error type returned by `do_scan` is not `Result<Vec<(u64, u64, bool)>>` (it returns `Result<Vec<(u64, u64, bool)>>` where `type Result<T> = std::result::Result<T, FlashError>` from the crate root — verify this).

## Maintenance notes

- After this fix, if a sparse image has a pathological layout that causes `lseek` to return extreme positions, the tool will fail with a clear error message instead of silently producing a corrupt sparse image.
- The `do_scan` function is only called from `scan_extents`, which is only called from `sparse_wrap_file`. The error propagates up through the format-data pipeline and will be displayed to the user.
- The fallback path `#[cfg(not(unix))]` does not have this issue — it reads blocks sequentially and doesn't use `lseek`.
