# Rust Best Practices Refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring pawflash into full compliance with Apollo Rust Best Practices — eliminate `anyhow` from the library crate, wrap all `unsafe` with SAFETY comments, fix edition mismatch, split over-large files, and resolve performance anti-patterns.

**Architecture:** 3-phase approach: (1) safety & policy violations, (2) performance & design, (3) maintainability. Each task produces independently testable, clippy-clean code.

**Tech Stack:** Rust 2024 edition, thiserror, async-trait, tokio, serde, tracing, clippy pedantic-deny

**Global Constraints:**
- Zero new `#[allow(...)]` — use `#[expect(...)]` with justification
- No `anyhow` in `pawflash-core` — only in `pawflash-cli` and `pawflash-tauri`
- All `unsafe` blocks must have `// SAFETY:` comment
- Every public function returning a value must have `#[must_use]`
- Files target ≤400 lines; over-large files must be split
- All changes must pass `cargo clippy --all-targets --all-features --locked -- -D warnings`
- All changes must pass `cargo test --workspace` (119 tests)

---

### Task 1: Remove `anyhow` from `pawflash-core` — Create `GsiError`

**Files:**
- Create: `crates/pawflash-core/src/gsi/error.rs`
- Modify: `crates/pawflash-core/src/gsi/mod.rs`
- Modify: `crates/pawflash-core/src/gsi/flash.rs` (lines 1-337)
- Modify: `crates/pawflash-core/Cargo.toml` (remove `anyhow` dep)

**Interfaces:**
- Consumes: `FlashError`, `std::io::Error`
- Produces: `GsiError` enum with `#[from]` for `FlashError`, `std::io::Error`; `type Result<T> = std::result::Result<T, GsiError>`

- [ ] **Step 1: Create `GsiError`**

Create `crates/pawflash-core/src/gsi/error.rs`:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GsiError {
    #[error("{0}")]
    Flash(#[from] crate::flash::error::FlashError),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("image check failed: {0}")]
    ImageCheck(String),

    #[error("format tools: {0}")]
    FormatTools(String),

    #[error("GSI flash cancelled by user")]
    Cancelled,

    #[error("partition resolution: {0}")]
    PartitionResolution(String),

    #[error("sparse header: {0}")]
    SparseHeader(String),
}

pub type Result<T> = std::result::Result<T, GsiError>;
```

- [ ] **Step 2: Update `gsi/mod.rs`**

Add `pub mod error;` line to the module declarations.

- [ ] **Step 3: Migrate `gsi/flash.rs`**

Replace:
```rust
use anyhow::{bail, Context, Result};
```
with:
```rust
use crate::gsi::error::{GsiError, Result};
```

Replace all `anyhow::bail!(...)` with `return Err(GsiError::...)`.

Replace `anyhow::anyhow!(...)` with `GsiError::...`.

Replace `.with_context(|| ...)` with `.map_err(|e: FlashError| GsiError::from(e))` or `.map_err(GsiError::from)`.

Replace `.context(...)` with `.map_err(GsiError::from)` for `FlashError`/`std::io::Error`, or `.map_err(|e| GsiError::ImageCheck(format!(...)))` for other errors.

Specific replacements in `gsi/flash.rs`:
- Line 56: `.map_err(|_| anyhow::anyhow!("failed to parse sparse file header"))?` → `.map_err(|_| GsiError::SparseHeader("failed to parse sparse file header".into()))?`
- Line 233: `anyhow::bail!("GSI flash cancelled by user");` → `return Err(GsiError::Cancelled);`
- Line 277: `.map_err(|e| anyhow::anyhow!("failed to extract format tools: {e}"))?` → `.map_err(|e| GsiError::FormatTools(format!("{e}")))?`
- Lines 89, 117, 151, 161, 278, 332-334: `.with_context(|| ...)` / `.context(...)` → `?` (since `GsiError: From<FlashError> + From<std::io::Error>`)
- Lines 50: `.with_context(|| ...)` → `.map_err(|e| GsiError::ImageCheck(format!(...)))` or similar

- [ ] **Step 4: Remove `anyhow` from core deps**

In `crates/pawflash-core/Cargo.toml`, remove the `anyhow = "1"` line.

- [ ] **Step 5: Verify**

Run: `cargo clippy --all-targets --all-features --locked -- -D warnings` and `cargo test --workspace`
Expected: both pass cleanly.

- [ ] **Step 6: Commit**

```bash
git add crates/pawflash-core/src/gsi/error.rs \
       crates/pawflash-core/src/gsi/mod.rs \
       crates/pawflash-core/src/gsi/flash.rs \
       crates/pawflash-core/Cargo.toml
git commit -m "refactor(core): replace anyhow with GsiError in gsi module"
```

---

### Task 2: Replace `#[allow]` with `#[expect]` across workspace

**Files:**
- Modify: `crates/pawflash-core/src/output/tables.rs:222`

- [ ] **Step 1: Read and verify current line**

Read tables.rs to confirm line 222 content.

- [ ] **Step 2: Migrate allow to expect**

In `tables.rs:222`, change:
```rust
#[allow(clippy::implicit_hasher)]
```
to:
```rust
// HashMap<K,V> required by tabled derive — workaround until tabled supports hasher param
#[expect(clippy::implicit_hasher)]
```

- [ ] **Step 3: Verify**

Run: `cargo clippy --all-targets --all-features --locked -- -D warnings`

- [ ] **Step 4: Commit**

```bash
git add crates/pawflash-core/src/output/tables.rs
git commit -m "style: use #[expect] instead of #[allow] for clippy"
```

---

### Task 3: Add `// SAFETY:` comments to all `unsafe` blocks

**Files:**
- Modify: `crates/pawflash-core/src/flash/sparse.rs:426,455,484`

- [ ] **Step 1: Add safety comments on `libc::lseek` calls**

At line 426:
```rust
// SAFETY: `fd` is a valid file descriptor from `AsRawFd` on a real file.
// `lseek` with SEEK_DATA is safe per POSIX; we check for -1 return which
// covers both errors (checked via errno/ENXIO) and absence of data.
let data_start = match unsafe { libc::lseek(fd, seek_offset, libc::SEEK_DATA) } {
```

At line 455:
```rust
// SAFETY: same fd as above; SEEK_HOLE after a data region is valid per POSIX.
let hole_start = match unsafe { libc::lseek(fd, hole_seek, libc::SEEK_HOLE) } {
```

- [ ] **Step 2: Fix the `align_to` call (rewrite without unsafe)**

Read the exact code around line 482-485. Replace the unsafe `align_to::<u128>` call with a safe zero-check.

The `is_all_zero` function should be:
```rust
fn is_all_zero(buf: &[u8]) -> bool {
    buf.chunks_exact(16).all(|c| c.iter().all(|&b| b == 0))
        && buf.iter().rev().take(buf.len() % 16).all(|&b| b == 0)
}
```

- [ ] **Step 3: Verify**

Run: `cargo clippy --all-targets --all-features --locked -- -D warnings` and `cargo test --workspace`

- [ ] **Step 4: Commit**

```bash
git add crates/pawflash-core/src/flash/sparse.rs
git commit -m "fix: add SAFETY comments to unsafe blocks, rewrite align_to without unsafe"
```

---

### Task 4: Fix Tauri crate edition to 2024

**Files:**
- Modify: `src-tauri/Cargo.toml:6`

- [ ] **Step 1: Bump edition**

Change `edition = "2021"` to `edition.workspace = true`.

- [ ] **Step 2: Verify**

Run: `cargo build -p pawflash-tauri 2>&1`
Expected: No edition-related errors. If `unsafe_op_in_unsafe_fn` fires, wrap inner operations of each `unsafe fn` in `unsafe { }` blocks.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "build: sync tauri crate edition to workspace 2024"
```

---

### Task 5: Lift 1 MiB buffer out of sparse hot loop

**Files:**
- Modify: `crates/pawflash-core/src/flash/sparse.rs:171,248,380`

- [ ] **Step 1: Extract reusable `XferBuf` helper**

Add at top of `flash/sparse.rs` (after imports):

```rust
/// Reusable 1 MiB transfer buffer to avoid per-chunk allocation.
struct XferBuf {
    buf: Vec<u8>,
}

impl XferBuf {
    fn new() -> Self {
        Self { buf: vec![0u8; 1024 * 1024] }
    }

    fn get(&mut self, size_hint: usize) -> &mut [u8] {
        let needed = size_hint.max(1024 * 1024);
        if self.buf.len() < needed {
            self.buf.resize(needed, 0);
        }
        &mut self.buf[..needed.min(self.buf.len())]
    }
}
```

- [ ] **Step 2: Thread `XferBuf` through functions**

Add `buf: &mut XferBuf` parameter to `flash_sparse_image`, `flash_sparse_wrapped`, `sparse_wrap_file`, and the inner loop helpers.

Replace `let mut buf = vec![0u8; 1024 * 1024];` with `let buf = buf.get(1024 * 1024);` in each loop body.

In `flash_sparse_image` (line 171): replace the per-iteration allocation.
In `flash_sparse_wrapped` (line 248): replace the per-iteration allocation.
In `sparse_wrap_file` (line 380): replace the per-iteration allocation.

Create a single `XferBuf` at the top of `flash_sparse_image` and pass into the read loops.

- [ ] **Step 3: Verify**

Run: `cargo clippy && cargo test --workspace`

- [ ] **Step 4: Commit**

```bash
git add crates/pawflash-core/src/flash/sparse.rs
git commit -m "perf: reuse 1 MiB xfer buffer instead of allocating per sparse chunk"
```

---

### Task 6: Return `Result<FormatDataResult, FlashError>` from `format_data`

**Files:**
- Modify: `crates/pawflash-core/src/flash/format.rs` (signature + body)
- Modify: `crates/pawflash-core/src/gsi/flash.rs` (add `?` to `format_data` call)
- Modify: `crates/pawflash-cli/src/cli/format_data.rs` (add `?` to `format_data` call)
- Modify: `crates/pawflash-cli/src/cli/interactive.rs` (add `?` to `format_data` call)
- Modify: `crates/pawflash-cli/src/cli/flash/scatter.rs` (add `?` to `format_data` call)
- Modify: `src-tauri/src/lib.rs` (add `?` to `format_data` call)

- [ ] **Step 1: Update `format_data` signature**

In `flash/format.rs`, change return type from `FormatDataResult` to `Result<FormatDataResult, FlashError>`.

Move the tool-extraction failure (lines 96-108) from pushing a `Failed` outcome to returning `Err(FlashError::GeneratorFailed { reason })`.

- [ ] **Step 2: Update all callers**

In `gsi/flash.rs`: add `?` after `executor.format_data(0, clean_test, None).await;`

In `cli/format_data.rs`, `cli/interactive.rs`, `cli/flash/scatter.rs`, and `src-tauri/lib.rs`: add `?` after the `.await`.

- [ ] **Step 3: Verify**

Run: `cargo build --workspace && cargo test --workspace`

- [ ] **Step 4: Commit**

```bash
git add crates/pawflash-core/src/flash/format.rs \
       crates/pawflash-core/src/gsi/flash.rs \
       crates/pawflash-cli/src/cli/format_data.rs \
       crates/pawflash-cli/src/cli/interactive.rs \
       crates/pawflash-cli/src/cli/flash/scatter.rs \
       src-tauri/src/lib.rs
git commit -m "refactor(core): return Result from format_data instead of swallowing errors"
```

---

### Task 7: Replace `GsiCounters` atomics with plain `u64`

**Files:**
- Modify: `crates/pawflash-core/src/gsi/flash.rs:170-209,330-335`

- [ ] **Step 1: Rewrite `GsiCounters`**

Replace:
```rust
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
```
with:
```rust
use std::sync::atomic::AtomicBool;
```

Replace the `GsiCounters` struct:
```rust
struct GsiCounters {
    flash_count: u64,
    wipe_count: u64,
    skipped_count: u64,
    total_bytes: u64,
}

impl GsiCounters {
    const fn new() -> Self {
        Self {
            flash_count: 0,
            wipe_count: 0,
            skipped_count: 0,
            total_bytes: 0,
        }
    }
}
```

- [ ] **Step 2: Update `make_reporter`**

Change signature to accept `counters: &mut GsiCounters`. Replace `fetch_add(1, Ordering::Relaxed)` with `*flash_count += 1` etc.

```rust
fn make_reporter<'a>(
    counters: &'a mut GsiCounters,
    inner: &'a mut impl FnMut(GsiEvent),
) -> impl FnMut(GsiEvent) + 'a {
    |event: GsiEvent| {
        match &event {
            GsiEvent::Flashing { size_bytes, .. } => {
                counters.flash_count += 1;
                counters.total_bytes += size_bytes;
            }
            GsiEvent::Wiping { .. } => counters.wipe_count += 1,
            GsiEvent::PartitionSkipped { .. } => counters.skipped_count += 1,
            _ => {}
        }
        inner(event);
    }
}
```

- [ ] **Step 3: Update `execute_gsi_flash`**

Change `let counters = GsiCounters::new();` to `let mut counters = GsiCounters::new();`.

Change `make_reporter(&counters, &mut user_report)` to `make_reporter(&mut counters, &mut user_report)`.

Replace `.load(Ordering::Relaxed)` with direct field reads.

- [ ] **Step 4: Verify**

Run: `cargo clippy && cargo test --workspace`

- [ ] **Step 5: Commit**

```bash
git add crates/pawflash-core/src/gsi/flash.rs
git commit -m "refactor(gsi): remove unnecessary atomics from GsiCounters"
```

---

### Task 8: Add `BootTarget::from_str` in core

**Files:**
- Modify: `crates/pawflash-core/src/flash/executor.rs` (add `FromStr` impl)
- Modify: `crates/pawflash-cli/src/cli/device.rs:38-45`
- Modify: `src-tauri/src/lib.rs:166-174`

- [ ] **Step 1: Add `FromStr` impl**

In `executor.rs`, after the `Display` impl for `BootTarget`:
```rust
impl std::str::FromStr for BootTarget {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "system" => Ok(Self::System),
            "bootloader" => Ok(Self::Bootloader),
            "fastbootd" | "fastboot" => Ok(Self::Fastboot),
            "recovery" => Ok(Self::Recovery),
            _ => Err(format!("unknown reboot target '{s}'")),
        }
    }
}
```

- [ ] **Step 2: Update CLI handler**

In `device.rs`, replace the `match` block (lines 38-45) with:
```rust
let boot_target: BootTarget = target.parse()?;
```

- [ ] **Step 3: Update Tauri handler**

In `lib.rs`, replace lines 166-174 with:
```rust
let boot_target: BootTarget = target.parse().map_err(|e: String| e)?;
```

- [ ] **Step 4: Verify**

Run: `cargo build --workspace && cargo test --workspace`

- [ ] **Step 5: Commit**

```bash
git add crates/pawflash-core/src/flash/executor.rs \
       crates/pawflash-cli/src/cli/device.rs \
       src-tauri/src/lib.rs
git commit -m "refactor: centralize BootTarget::from_str in core crate"
```

---

### Task 9: Cache format-tools extraction

**Files:**
- Modify: `crates/pawflash-core/src/format/generator.rs`
- Modify: `crates/pawflash-core/src/gsi/flash.rs`

- [ ] **Step 1: Read exact code around extract_format_tools and its callers**

- [ ] **Step 2: Add caching to `extract_format_tools`**

Change `extract_format_tools` signature from returning `(TempDir, PathBuf)` to returning `io::Result<PathBuf>` (only the path), and internally use `OnceLock` to cache the extraction:

```rust
use std::sync::OnceLock;

static TOOLS_ROOT: OnceLock<io::Result<PathBuf>> = OnceLock::new();

pub fn extract_format_tools() -> &io::Result<PathBuf> {
    TOOLS_ROOT.get_or_init(|| {
        let dir = TempDir::new()?;
        let root = dir.path().to_path_buf();
        // ... write files ...
        // Leak the TempDir so it lives for program lifetime
        std::mem::forget(dir);
        Ok(root)
    })
}
```

- [ ] **Step 3: Update callers**

In `gsi/flash.rs` and `flash/format.rs`, update to use the new signature. The output path is now `extract_format_tools()?.clone()` or `extract_format_tools().as_ref().map_err(|e| ...)?` followed by `.clone()`.

In `gsi/flash.rs` line 276:
```rust
let tools_root = generator::extract_format_tools()
    .as_ref()
    .map_err(|e| GsiError::FormatTools(format!("{e}")))?
    .clone();
```

In `flash/format.rs` line 96:
```rust
let (_tools, tools_dir) = match generator::extract_format_tools().as_ref() {
    Ok(path) => (path.clone(), path.clone()),  // caller discards _tools
    Err(e) => { return Err(FlashError::GeneratorFailed { reason: format!("{e}") }); }
};
```

- [ ] **Step 4: Verify**

Run: `cargo build --workspace && cargo test --workspace`

- [ ] **Step 5: Commit**

```bash
git add crates/pawflash-core/src/format/generator.rs \
       crates/pawflash-core/src/gsi/flash.rs \
       crates/pawflash-core/src/flash/format.rs
git commit -m "perf: cache format-tools extraction via OnceLock"
```

---

### Task 10: Add `#[must_use]` to public functions

**Files:**
- Modify: All public `fn` across `pawflash-core/src/`

- [ ] **Step 1: Audit with clippy**

Run: `cargo clippy -- -W clippy::must_use_candidate 2>&1 | grep "must_use_candidate" | grep -oP '\S+\.rs:\d+:\d+'` to get exact locations.

- [ ] **Step 2: Add `#[must_use]`**

Add `#[must_use]` to every pure/computed public function. Key targets:
- `BootTarget::as_str`
- `FastbootMode::as_str`
- `GsiStep::as_str`
- `FsType::from_partition_type`
- `canonical_name`
- `verbosity()`
- `FlashExecutor::new`
- `FlashExecutor::device_vars`
- All `*_colored` functions in `status.rs`
- All `scatter_metadata`, `plan_summary`, `plan_actions`, etc. in `tables.rs`

- [ ] **Step 3: Verify**

Run: `cargo clippy --all-targets --all-features --locked -- -D warnings`

- [ ] **Step 4: Commit**

```bash
git add crates/pawflash-core/src/
git commit -m "style: add #[must_use] to all pure public functions"
```

---

### Task 11: Split over-large files

**Files to split:**
- `crates/pawflash-core/src/flash/sparse.rs` (670 → 3 files)
- `crates/pawflash-core/src/flash/executor.rs` (652 → 3 files)
- `crates/pawflash-core/src/gsi/flash.rs` (491 → 3 files)
- `crates/pawflash-core/src/flash/format.rs` (405 → 2 files)

- [ ] **Step 1: Split `sparse.rs` → `sparse/` directory**

Create directory `flash/sparse/`. Move code:
- `flash/sparse/mod.rs` — re-exports, `CRYPT_FOOTER_OFFSET`, `read_exact_padded`, `read_exact_padded_or_truncate`, `is_sparse_image`, `XferBuf`
- `flash/sparse/chunked.rs` — `flash_sparse_image`, `flash_sparse_wrapped`, `sparse_wrap_file`
- `flash/sparse/scan.rs` — `do_scan_impl`, `scan_extents`, `build_split_chunks`, `is_all_zero`

Update `flash/mod.rs` to replace `pub mod sparse;` with `pub mod sparse;` (it becomes a directory module).

- [ ] **Step 2: Split `executor.rs` → `executor/` directory**

Create directory `flash/executor/`. Move code:
- `flash/executor/mod.rs` — re-exports, `FlashExecutor` struct, `BootTarget` enum, `EMPTY_VBMETA`, `set_expected_serial`, `expected_serial`, `parse_max_download`
- `flash/executor/connect.rs` — `FlashExecutor::connect`, `wait_for_device`, `reboot_and_wait`, `ensure_fastbootd`
- `flash/executor/flash.rs` — `execute_plan`, `flash_partition`, `flash_raw_partition`, `flash_image_to_partition`, `flash_empty_vbmeta`, `flash_raw_image`
- `flash/executor/manage.rs` — all the simple wrappers (`get_var`, `reboot`, `flashing_lock`, etc.)

Update `flash/mod.rs` to add `pub mod executor;`.

- [ ] **Step 3: Split `gsi/flash.rs`**

Create `gsi/product.rs` — `generate_product_gsi_image`, `flash_system_and_product`
Create `gsi/transition.rs` — `transition_mode`, `detect_fastboot_mode`
Keep in `gsi/flash.rs` — `execute_gsi_flash`, `GsiCounters`, `make_reporter`, `GsiStage`, `plan_stage_groups`, `check_cancelled`

- [ ] **Step 4: Split `flash/format.rs`** — only if >400 lines after other changes. Current is 405 lines, so a minor trim may suffice. Otherwise extract `wipe_partition`, `clear_bootloader_bcb`, `partition_type`, `determine_fs_type`, `query_partition_size` into `flash/format/wipe.rs`.

- [ ] **Step 5: Verify**

Run: `cargo clippy --all-targets --all-features --locked -- -D warnings` and `cargo test --workspace`

- [ ] **Step 6: Commit per split**

```bash
git add crates/pawflash-core/src/flash/sparse/
git commit -m "refactor: split sparse.rs into sparse/ submodule"
git add crates/pawflash-core/src/flash/executor/
git commit -m "refactor: split executor.rs into executor/ submodule"
git add crates/pawflash-core/src/gsi/product.rs crates/pawflash-core/src/gsi/transition.rs
git commit -m "refactor: split gsi/flash.rs into submodules"
git add crates/pawflash-core/src/flash/format/
git commit -m "refactor: split flash/format.rs into submodules"
```

---

### Task 12: Replace manual ANSI stripping

**Files:**
- Modify: `crates/pawflash-core/src/output/status.rs:5-19`
- Modify: `crates/pawflash-core/Cargo.toml`

- [ ] **Step 1: Add dependency**

In `Cargo.toml`, add:
```toml
strip-ansi-escapes = "0.2"
```

- [ ] **Step 2: Replace `strip()` function**

Replace the manual `fn strip(s: &str) -> String` (lines 5-19) with:

```rust
fn strip(s: &str) -> String {
    String::from_utf8(strip_ansi_escapes::strip(s)).unwrap_or_else(|_| s.to_string())
}
```

Add `use strip_ansi_escapes;` at the top of the file.

- [ ] **Step 3: Verify**

Run: `cargo clippy --all-targets && cargo test --workspace`

- [ ] **Step 4: Commit**

```bash
git add crates/pawflash-core/src/output/status.rs \
       crates/pawflash-core/Cargo.toml
git commit -m "refactor: replace manual ANSI strip with strip-ansi-escapes crate"
```

---

### Task 13: Add crate-level docs for CLI and Tauri

**Files:**
- Modify: `crates/pawflash-cli/src/lib.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `//!` doc to `pawflash-cli`**

Replace:
```rust
pub mod cli;
```
with:
```rust
//! CLI binary wiring for pawflash — argument parsing, logging init, and dispatch to
//! `pawflash-core` commands. Each subcommand has a handler in `cli/`.

pub mod cli;
```

- [ ] **Step 2: Add `//!` doc to `pawflash-tauri`**

Add at top of `src-tauri/src/lib.rs`:
```rust
//! Tauri v2 desktop app for pawflash — exposes core flashing operations as
//! IPC commands with progress reporting via `Channel<ProgressEvent>`.
```

- [ ] **Step 3: Verify**

Run: `cargo doc --no-deps --workspace 2>&1 | head -5`
Expected: no warnings. (Ignore any intra-doc link warnings from third-party crates.)

- [ ] **Step 4: Commit**

```bash
git add crates/pawflash-cli/src/lib.rs src-tauri/src/lib.rs
git commit -m "docs: add crate-level docs for cli and tauri"
```

---

### Task 14: Replace `EMPTY_VBMETA` byte array with `include_bytes!`

**Files:**
- Create: `vendor/empty-vbmeta.img`
- Modify: `crates/pawflash-core/src/flash/executor.rs`

- [ ] **Step 1: Generate the empty vbmeta binary blob**

Write the EXACT same 512 bytes from the current `EMPTY_VBMETA` constant into `vendor/empty-vbmeta.img`. Use a small script:

```bash
cd /home/rd/Projects/pawflash
python3 -c "
import sys
data = bytes([0x41, 0x56, 0x42, 0x30, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x61, 0x76, 0x62, 0x74,
    0x6f, 0x6f, 0x6c, 0x20, 0x31, 0x2e, 0x33, 0x2e, 0x30, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00])
assert len(data) == 512, f'Expected 512 bytes, got {len(data)}'
with open('vendor/empty-vbmeta.img', 'wb') as f:
    f.write(data)
print('Written 512 bytes')
"
```

- [ ] **Step 2: Replace inline array**

In `executor.rs`, replace:
```rust
const EMPTY_VBMETA: &[u8] = &[0x41, 0x56, ...];
```
with:
```rust
/// Empty vbmeta image with AVB_FLAGS_NO_AUTH_VERIFICATION (flags=3).
/// Generated by: avbtool make_vbmeta_image --flags 3 --output vendor/empty-vbmeta.img
const EMPTY_VBMETA: &[u8] = include_bytes!("../../../vendor/empty-vbmeta.img");
```

- [ ] **Step 3: Remove `pub(crate)` from `flash_raw_partition` if no longer needed**

- [ ] **Step 4: Verify**

Run: `cargo build -p pawflash-core && cargo test --workspace`

- [ ] **Step 5: Commit**

```bash
git add vendor/empty-vbmeta.img crates/pawflash-core/src/flash/executor.rs
git commit -m "refactor: replace inline vbmeta bytes with include_bytes!"
```

---

### Task 15: Extract helpers from `scatter_parser/parse/mod.rs`

**Files:**
- Create: `crates/pawflash-core/src/scatter_parser/parse/normalize.rs`
- Modify: `crates/pawflash-core/src/scatter_parser/parse/mod.rs`

- [ ] **Step 1: Read the current code**

Read `parse/mod.rs` lines 60-80+ to find `normalize_partition` and `validate_layouts`.

- [ ] **Step 2: Create `normalize.rs`**

Extract `normalize_partition()` and `validate_layouts()` into the new file. Both are called only from `parse_scatter()` in `mod.rs`. Keep all their helper function dependencies in the same file.

The new file should have `pub(super) fn normalize_partition(...)` and `pub(super) fn validate_layouts(...)`.

- [ ] **Step 3: Update `mod.rs`**

Add `mod normalize;` to the module declarations and `use normalize::{normalize_partition, validate_layouts};`. Remove the inline definitions.

- [ ] **Step 4: Verify**

Run: `cargo clippy --all-targets && cargo test --workspace`

- [ ] **Step 5: Commit**

```bash
git add crates/pawflash-core/src/scatter_parser/parse/normalize.rs \
       crates/pawflash-core/src/scatter_parser/parse/mod.rs
git commit -m "refactor: extract normalize/validate helpers from parse/mod.rs"
```
