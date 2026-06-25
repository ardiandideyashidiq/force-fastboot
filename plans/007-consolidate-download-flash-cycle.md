# Plan 007: Consolidate duplicated download→send→flash cycle into shared helper

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/flash/`
> If these files changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: MEDIUM (refactors core protocol interaction; must preserve error handling)
- **Depends on**: Plan 005 (characterization tests must land first)
- **Category**: tech-debt
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

The pattern `fb.download(size) → sender.extend_from_slice(data) → sender.finish() → fb.flash(partition)` appears in four places across `src/flash/`:
1. `flash_raw_partition` in `executor.rs:471-503`
2. `flash_sparse_image` in `sparse.rs:85-193`
3. `flash_sparse_wrapped` in `sparse.rs:201-267`
4. `sparse_wrap_file` in `sparse.rs:285-389`

Each has subtle differences in error reporting, progress bar management, and chunk streaming. Any change to the download protocol (retries, progress, validation, cancellation) must be replicated in all four, and they've already drifted slightly (e.g., `flash_sparse_image` increments the progress bar per-chunk-header, others don't). Consolidating into a shared helper reduces duplication and the risk of divergent bugs.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/flash/sparse.rs` — refactor to use a shared session helper
- `src/flash/executor.rs` — refactor `flash_raw_partition` to use the shared helper
- `src/flash/mod.rs` — optionally expose the new helper module

**Out of scope** (do NOT touch):
- The external API (`FlashExecutor::flash_raw_image`, `FlashExecutor::execute_plan`) — signatures must not change
- `src/gsi/flash.rs` — calls `flash_raw_image` which is the public API; no changes needed
- `src/flash/format.rs` — calls `sparse_wrap_file` through public API; no changes needed

## Git workflow

- Branch: `advisor/007-consolidate-download-flash-cycle`
- Commit message: `refactor: extract download→send→flash cycle into shared FlashSession helper`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Design the shared helper

Before writing code, define the interface. The helper should encapsulate:
1. `fb.download(size)` — start a download transaction
2. Streaming data via a callback or chunk iterator
3. `sender.finish()` — finalize the download
4. `fb.flash(partition)` — commit to partition

Proposed API (add to a new module `src/flash/session.rs`):

```rust
pub(crate) struct FlashSession<'a> {
    fb: &'a mut NusbFastBoot,
    partition: &'a str,
    progress_bar: Option<&'a ProgressBar>,
}

impl FlashSession<'_> {
    /// Create a new session for the given partition.
    pub fn new(fb: &mut NusbFastBoot, partition: &str, progress_bar: Option<&ProgressBar>) -> FlashSession<'_> { ... }

    /// Download data from a reader and flash it in a single transaction.
    /// The reader is read in chunks until EOF.
    pub async fn download_and_flash(
        &mut self,
        size: u32,
        reader: impl AsyncRead + Unpin,
    ) -> Result<String> { ... }

    /// Download pre-built sparse chunks and flash.
    /// `chunks` provides the header bytes + data offsets.
    pub async fn download_sparse_and_flash(
        &mut self,
        sparse_size: u32,
        chunk_data: &[SparseChunk],
        file: &mut tokio::fs::File,
    ) -> Result<String> { ... }
}
```

### Step 2: Create `src/flash/session.rs`

Create the new module with `FlashSession`. Move the common download→send→flash pattern into `download_and_flash`:

```rust
pub(crate) async fn download_and_flash(
    fb: &mut NusbFastBoot,
    partition: &str,
    size: u32,
    data: impl FnOnce(&mut dyn FnMut(&[u8]) -> ...) -> ...,
) -> Result<String>
```

Or simpler: an async function that takes a closure for sending data:

```rust
pub(crate) async fn download_and_flash(
    fb: &mut NusbFastBoot,
    partition: &str,
    total_size: u32,
    send_fn: impl AsyncFnOnce(&mut NusbFastBootDownloadSender<'_>) -> Result<()>,
) -> Result<String>
```

**However**, Rust's async closures are unstable. Use a concrete approach instead:

```rust
/// Download data and flash it to a partition.
/// Writes `total_size` bytes through the sender using `write_chunk`, then flashes.
pub(crate) async fn download_and_flash(
    fb: &mut NusbFastBoot,
    partition: &str,
    total_size: u32,
    mut write_chunk: impl FnMut(&mut NusbFastBootDownloadSender<'_>) -> BoxFuture<'_, Result<()>>,
) -> Result<String>
```

Actually, the simplest approach that works with stable Rust: pass the sender as a `&mut` and have the caller write to it:

```rust
pub(crate) async fn download_and_flash<F, Fut>(
    fb: &mut NusbFastBoot,
    partition: &str,
    total_size: u32,
    write_fn: F,
) -> Result<String>
where
    F: FnOnce(&mut NusbFastBootDownloadSender<'_>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut sender = fb.download(total_size).await?;
    write_fn(&mut sender).await?;
    sender.finish().await?;
    let resp = fb.flash(partition).await?;
    Ok(resp)
}
```

This captures the common pattern. Create this function and then refactor the four sites to use it.

**Verify**: `cargo build` exits 0.

### Step 3: Refactor `flash_raw_partition` in `executor.rs`

Change `flash_raw_partition` to use `download_and_flash`. The existing code at lines 477-503 becomes:

```rust
pub(crate) async fn flash_raw_partition(
    &mut self,
    partition: &str,
    path: &Path,
    size: u32,
    progress_bar: Option<&ProgressBar>,
) -> Result<String> {
    debug!(%partition, file_size = size, "flashing raw partition");
    let mut file = tokio::fs::File::open(path).await?;

    let resp = crate::flash::session::download_and_flash(
        &mut self.fb,
        partition,
        size,
        |sender| async move {
            let mut buf = vec![0u8; 1024 * 1024];
            let mut written = 0u64;
            loop {
                let n = file.read(&mut buf).await?;
                if n == 0 { break; }
                sender.extend_from_slice(&buf[..n]).await?;
                written += n as u64;
                if let Some(pb) = progress_bar {
                    pb.set_position(written);
                }
            }
            Ok(())
        },
    ).await?;

    if let Some(pb) = progress_bar {
        pb.set_position(u64::from(size));
    }
    debug!(%partition, response = resp, "raw partition flash complete");
    Ok(resp)
}
```

Note: The `buf` allocation is now scoped to the closure, so it's still allocated per-call. A future optimization (Plan 005's PERF-1) can hoist it.

**Verify**: `cargo build` exits 0.

### Step 4: Refactor `flash_sparse_image` in `sparse.rs`

Replace the download→send→flash loop (lines 141-185) — the core loop that processes each split — with `download_and_flash` calls inside the split loop.

The split loop sends one sparse split per iteration:

```rust
for (i, split) in splits.iter().enumerate() {
    debug!(%partition, part = i, "sending sparse split");
    let sparse_size = u32::try_from(split.sparse_size())?;

    let resp = crate::flash::session::download_and_flash(
        fb, partition, sparse_size,
        |sender| async move {
            // file header for this split
            sender.extend_from_slice(&split.header.to_bytes()).await?;
            if let Some(pb) = progress_bar { pb.inc(FILE_HEADER_BYTES_LEN as u64); }

            // chunk headers + data
            for chunk in &split.chunks {
                sender.extend_from_slice(&chunk.header.to_bytes()).await?;
                if let Some(pb) = progress_bar { pb.inc(CHUNK_HEADER_BYTES_LEN as u64); }

                if chunk.size > 0 {
                    file.seek(SeekFrom::Start(chunk.offset as u64)).await?;
                    let mut remaining = chunk.size;
                    let mut buf = vec![0u8; 1024 * 1024];
                    while remaining > 0 {
                        let to_read = buf.len().min(remaining);
                        read_exact_padded_or_truncate(&mut file, &mut buf[..to_read], chunk.size).await?;
                        sender.extend_from_slice(&buf[..to_read]).await?;
                        if let Some(pb) = progress_bar { pb.inc(to_read as u64); }
                        remaining = remaining.saturating_sub(to_read);
                    }
                }
            }
            Ok(())
        },
    ).await?;

    last_resp = resp;
}
```

**Verify**: `cargo build` exits 0.

### Step 5: Refactor `flash_sparse_wrapped` in `sparse.rs`

Same pattern as step 4 — replace lines 232-262 with `download_and_flash` calls inside the split loop.

**Verify**: `cargo build` exits 0.

### Step 6: Refactor `sparse_wrap_file` in `sparse.rs`

Replace lines 366-385 with `download_and_flash`:

```rust
let resp = crate::flash::session::download_and_flash(
    fb, partition, sparse_size,
    |sender| async move {
        sender.extend_from_slice(&header.to_bytes()).await?;
        for chunk in &chunks {
            sender.extend_from_slice(&chunk.header.to_bytes()).await?;
            if chunk.size > 0 {
                file.seek(SeekFrom::Start(chunk.offset as u64)).await?;
                // ... read and send chunk data ...
            }
        }
        Ok(())
    },
).await?;
```

**Verify**: `cargo build` exits 0.

### Step 7: Update `src/flash/mod.rs`

Add `pub mod session;` to the module declarations in `src/flash/mod.rs`.

**Verify**: `cargo build` exits 0.

### Step 8: Run tests and clippy

**Verify**: `cargo test` exits 0, all tests pass (including Plan 005's characterization tests).
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

The characterization tests from Plan 005 must be in place before this refactor. Run `cargo test` after the refactor — all tests must pass, confirming the behavior is preserved.

Additionally, manually verify that:
- Progress bar updates behave identically before and after (the refactored code moves progress bar increments into the same locations)
- Error propagation preserves the same error types and messages
- The `buf = vec![0u8; 1024 * 1024]` allocation is still present (deferred optimization)

## Done criteria

ALL must hold:

- [ ] Plan 005 tests are in place and passing before starting this plan
- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `src/flash/session.rs` exists with `download_and_flash` function
- [ ] All four flash sites (`flash_raw_partition`, `flash_sparse_image`, `flash_sparse_wrapped`, `sparse_wrap_file`) use `download_and_flash`
- [ ] No `fb.download(...)` call remains outside `session.rs`
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` fails after the changes.
- Any test fails (pre-existing or induced).
- The `NusbFastBootDownloadSender` type is not publicly accessible from `session.rs` (check the vendored `fastboot-protocol` crate's `nusb` module).
- The closure-based approach doesn't compile due to lifetime issues (the async closure borrows `fb` mutably — if the compiler rejects it, switch to a macro-based approach instead).

## Maintenance notes

- After consolidation, any improvement to the download protocol (retries, progress, validation, cancellation) goes in one place: `session.rs`.
- The `buf = vec![0u8; 1024 * 1024]` allocation is still duplicated across the four call sites. A follow-up optimization can hoist it into `download_and_flash` as a reusable buffer, but that's deferred to keep this refactor behavior-preserving.
- A reviewer should verify that all error paths that were `return Err(...)` in the original code are still present and correctly propagated through the closure.
