# Plan 001: Fix `sparse_wrap_file` truncation destroying filesystem data

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/flash/sparse.rs`
> If this file changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: HIGH (the fix touches a core data path; a mistake could still produce corrupted sparse images)
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

Every `format-data` call generates a valid ext4 or f2fs filesystem image via `mke2fs`/`make_f2fs`, then passes it to `sparse_wrap_file` which opens the path with `File::create(path)` — **truncating the file to 0 bytes** before reading it. The file is then extended to the partition size with `set_len` (all zeros) and scanned for data extents. The scan finds nothing but zeros, producing only DONTCARE sparse chunks. The device receives a sparse image with no actual filesystem data, causing `userdata`, `metadata`, and `cache` to be effectively wiped but unformatted — potentially leading to bootloops or factory-reset loops.

## Current state

The relevant code is in `src/flash/sparse.rs:285-389`, specifically lines 303-315:

```rust
// src/flash/sparse.rs:303-315
// Save original file size before extending.
let orig_size = {
    let m = tokio::fs::metadata(path).await?;
    m.len()
};

// Extend the output file to effective_size. On Linux this creates a
// sparse file (holes) so the extra space costs no disk I/O.
{
    let f = tokio::fs::File::create(path).await?;  // <-- BUG: truncates file
    f.set_len(effective_size).await?;
    drop(f);
}
```

`tokio::fs::File::create(path)` is equivalent to `OpenOptions::new().write(true).create(true).truncate(true).open(path)`. It destroys all existing content. The file at `path` is the output of `generator::generate_empty_fs()` called from `src/flash/format.rs:290`, which just wrote a valid filesystem to it.

After truncation, `set_len(effective_size)` extends the file to the full partition size (all zeros via sparse file support on Linux). The subsequent scan at line 324 finds no data, producing a worthless sparse image.

This function is called from `src/flash/format.rs:309` in the `wipe_partition` method, which is itself called from `format_data()` for each of `["userdata", "metadata", "cache"]`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/flash/sparse.rs` — change `File::create` to non-truncating open

**Out of scope** (do NOT touch):
- `src/flash/format.rs` — the caller is correct; only `sparse_wrap_file` needs fixing
- Any other file — this plan is a single-line bug fix

## Git workflow

- Branch: `advisor/001-fix-sparse-wrap-file-truncation`
- Commit message style: `fix: use non-truncating open in sparse_wrap_file to preserve filesystem data`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Replace `File::create` with non-truncating open

In `src/flash/sparse.rs`, change lines 311-314 from:

```rust
{
    let f = tokio::fs::File::create(path).await?;
    f.set_len(effective_size).await?;
    drop(f);
}
```

to:

```rust
{
    let f = tokio::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .await?;
    f.set_len(effective_size).await?;
    drop(f);
}
```

The key change: `File::create` truncates the file to 0; `OpenOptions::new().write(true).open(path)` opens the existing file for writing without truncating, preserving the filesystem data that `generate_empty_fs` just wrote.

**Verify**: `cargo build` exits 0.

### Step 2: Verify with tests

Run the existing test suite:

**Verify**: `cargo test` exits 0, all tests pass.

### Step 3: Verify with clippy

**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

The existing test suite should continue to pass. No new tests are needed for this fix because the bug is in a file-IO interaction (requires a real filesystem image) — but the `cargo test` suite acts as a regression check. The executor should verify that `cargo test` reports no regressions.

A manual verification would be: create a valid ext4 image with `mke2fs`, then call `sparse_wrap_file` with that path and confirm the resulting sparse image contains RAW chunks (not DONTCARE-only). This is not automated in the current test suite.

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `git diff` shows only the change to `src/flash/sparse.rs`
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at `src/flash/sparse.rs:311-315` doesn't match the "Current state" excerpts (the codebase has drifted).
- `cargo build` fails after the change.
- `cargo test` reports any test failure (pre-existing or induced).
- You discover that the `path` argument to `sparse_wrap_file` might not exist yet when called — check the call site at `src/flash/format.rs:309-317` to confirm a file was written first.

## Maintenance notes

- The same pattern (`File::create` for extending a file) appears nowhere else in the codebase; this is the only instance.
- If future callers of `sparse_wrap_file` pass a path that doesn't exist yet, the non-truncating open will fail with `NotFound` — which is correct behavior (fail-fast rather than silently truncate a missing file).
- A reviewer should scrutinize that the file at `path` is guaranteed to exist before `sparse_wrap_file` is called. It is: `generate_empty_fs` at `format.rs:290` writes it first.
