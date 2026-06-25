# Plan 010: Deduplicate chunk-sending loop in `flash_sparse_*` functions

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/flash/sparse.rs`
> If this file changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

`src/flash/sparse.rs` contains three functions that each implement a
nearly-identical loop for sending chunk data to the fastboot transport:
`flash_sparse_image` (lines 143–192), `flash_sparse_wrapped` (lines 222–263),
and `sparse_wrap_file` (lines 373–387). The chunk-sending logic (download
header, iterate chunks, seek file, extend_from_slice, finish, flash) is
duplicated ~40 lines per function. A bug fix in one loop risks missing the
others.

## Current state

The three loops differ in exactly two ways:

1. **File seek behavior**: `flash_sparse_image` (line 167) seeks to
   `chunk.offset as u64` for each data chunk. `flash_sparse_wrapped` (line
   244) does the same. `sparse_wrap_file` (line 377) also does the same.

2. **Read padding**: `flash_sparse_image` uses `read_exact_padded_or_truncate`
   (which checks for truncation), while `flash_sparse_wrapped` and
   `sparse_wrap_file` use plain `read_exact_padded` (which doesn't check).

3. **Progress bar**: `flash_sparse_image` has progress bar updates per chunk
   (lines 155–156, 162–163, 174–175). `flash_sparse_wrapped` and
   `sparse_wrap_file` don't update progress per chunk.

All three share this pattern:
```rust
let mut sender = fb.download(sparse_size).await?;
sender.extend_from_slice(&split.header.to_bytes()).await?;
for chunk in &split.chunks {
    sender.extend_from_slice(&chunk.header.to_bytes()).await?;
    if chunk.size > 0 {
        file.seek(SeekFrom::Start(chunk.offset)).await?;
        let mut remaining = chunk.size;
        let mut buf = vec![0u8; 1024 * 1024];
        while remaining > 0 {
            let to_read = buf.len().min(remaining);
            read_exact_padded(&mut file, &mut buf[..to_read]).await?;
            sender.extend_from_slice(&buf[..to_read]).await?;
            remaining = remaining.saturating_sub(to_read);
        }
    }
}
sender.finish().await?;
last_resp = fb.flash(partition).await?;
```

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/flash/sparse.rs` — extract shared chunk-sending logic

**Out of scope**:
- Any file outside `src/flash/sparse.rs`
- The test module in `flash/sparse.rs` (lines 594–633) — existing tests must
  still compile and pass

## Git workflow

- Branch: `advisor/010-deduplicate-sparse-chunk-loop`
- Single commit: `refactor: extract shared sparse chunk-send loop helper`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add a `send_chunks` helper function

Add a private helper function in `src/flash/sparse.rs`:

```rust
/// Send sparse split chunks to the fastboot transport.
///
/// Downloads the split header, then each chunk's header + data via
/// the provided sender. Shared between the three flash entry points.
async fn send_chunks<'a>(
    sender: &mut fastboot_protocol::nusb::DownloadSender<'a>,
    file: &mut tokio::fs::File,
    split: &SplitChunk,
    check_truncation: bool,
    progress_bar: Option<&ProgressBar>,
) -> Result<()> {
    sender.extend_from_slice(&split.header.to_bytes()).await?;
    if let Some(pb) = progress_bar {
        pb.inc(FILE_HEADER_BYTES_LEN as u64);
    }

    for chunk in &split.chunks {
        sender.extend_from_slice(&chunk.header.to_bytes()).await?;
        if let Some(pb) = progress_bar {
            pb.inc(CHUNK_HEADER_BYTES_LEN as u64);
        }

        if chunk.size > 0 {
            file.seek(SeekFrom::Start(chunk.offset as u64)).await?;

            let mut remaining = chunk.size;
            let mut buf = vec![0u8; 1024 * 1024];
            while remaining > 0 {
                let to_read = buf.len().min(remaining);
                if check_truncation {
                    read_exact_padded_or_truncate(&mut file, &mut buf[..to_read], chunk.size).await?;
                } else {
                    read_exact_padded(&mut file, &mut buf[..to_read]).await?;
                }
                sender.extend_from_slice(&buf[..to_read]).await?;
                if let Some(pb) = progress_bar {
                    pb.inc(to_read as u64);
                }
                remaining = remaining.saturating_sub(to_read);
            }
        }
    }

    Ok(())
}
```

**Verify**: `cargo build` exits 0.

### Step 2: Replace the chunk loop in `flash_sparse_image`

Replace lines 143–192 in `flash_sparse_image` with:

```rust
let mut last_resp = String::new();
for (i, split) in splits.iter().enumerate() {
    debug!(%partition, part = i, "sending sparse split");

    let sparse_size = u32::try_from(split.sparse_size())
        .map_err(|_| FlashError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "sparse split size exceeds u32 range",
        )))?;
    let mut sender = fb.download(sparse_size).await?;

    send_chunks(&mut sender, &mut file, split, true, progress_bar).await?;

    sender.finish().await?;
    last_resp = fb.flash(partition).await?;
}
```

Note: `flash_sparse_image` uses `check_truncation: true` (it calls
`read_exact_padded_or_truncate`).

**Verify**: `cargo build` exits 0.

### Step 3: Replace the chunk loop in `flash_sparse_wrapped`

Replace lines 222–263 in `flash_sparse_wrapped` with:

```rust
let mut last_resp = String::new();
for (i, split) in splits.iter().enumerate() {
    debug!(%partition, part = i, "sending sparse-wrapped split");

    let sparse_size = u32::try_from(split.sparse_size())
        .map_err(|_| FlashError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "sparse split size exceeds u32 range",
        )))?;
    let mut sender = fb.download(sparse_size).await?;

    send_chunks(&mut sender, &mut file, split, false, None).await?;

    sender.finish().await?;
    last_resp = fb.flash(partition).await?;
}
```

Note: `flash_sparse_wrapped` uses `check_truncation: false` (plain
`read_exact_padded`) and `progress_bar: None`.

**Verify**: `cargo build` exits 0.

### Step 4: Replace the chunk loop in `sparse_wrap_file`

Replace lines 371–390 in `sparse_wrap_file` with:

```rust
let mut sender = fb.download(sparse_size).await?;

// The header for the full sparse image (not a split)
sender.extend_from_slice(&header.to_bytes()).await?;

for chunk in &chunks {
    sender.extend_from_slice(&chunk.header.to_bytes()).await?;

    if chunk.size > 0 {
        file.seek(SeekFrom::Start(chunk.offset as u64)).await?;
        let mut remaining = chunk.size;
        let mut buf = vec![0u8; 1024 * 1024];
        while remaining > 0 {
            let to_read = buf.len().min(remaining);
            read_exact_padded(&mut file, &mut buf[..to_read]).await?;
            sender.extend_from_slice(&buf[..to_read]).await?;
            remaining = remaining.saturating_sub(to_read);
        }
    }
}

sender.finish().await?;
let resp = fb.flash(partition).await?;
```

Wait — `sparse_wrap_file` has a slightly different structure. It sends the
header separately (line 372), then loops over chunks (lines 374–387). It
doesn't use `SplitChunk` — it uses custom `SplitChunk` objects from
`build_split_chunks`. The chunk loop is similar but not identical to the
split level of the other two functions.

**Correct approach for sparse_wrap_file**: `sparse_wrap_file` sends a single
download with a file header followed by chunk headers+data. The other two
functions send multiple downloads, each with a file header + subset of chunks.
So `sparse_wrap_file` doesn't use `SplitChunk` at the top level — it uses
the raw `chunks: Vec<SplitChunk>`. The data-sending part (seek + read + send)
_is_ duplicated. Extract just that inner loop:

Add a helper `send_chunk_data`:

```rust
async fn send_chunk_data(
    file: &mut tokio::fs::File,
    sender: &mut fastboot_protocol::nusb::DownloadSender<'_>,
    chunk: &SplitChunk,
) -> Result<()> {
    if chunk.size > 0 {
        file.seek(SeekFrom::Start(chunk.offset as u64)).await?;
        let mut remaining = chunk.size;
        let mut buf = vec![0u8; 1024 * 1024];
        while remaining > 0 {
            let to_read = buf.len().min(remaining);
            read_exact_padded(&mut file, &mut buf[..to_read]).await?;
            sender.extend_from_slice(&buf[..to_read]).await?;
            remaining = remaining.saturating_sub(to_read);
        }
    }
    Ok(())
}
```

Then use it in all three functions' inner chunk loops:
- `flash_sparse_image`: progress bar inc + `send_chunk_data`
- `flash_sparse_wrapped`: `send_chunk_data`
- `sparse_wrap_file`: chunk header extend + `send_chunk_data`

**Verify**: `cargo build` exits 0.

### Step 5: Run tests and lint

**Verify**:
- `cargo test` — all 83 tests pass (including existing sparse tests)
- `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean

## Test plan

Existing tests cover:
- `read_exact_padded_should_zero_fill_short_file` (sparse.rs:599)
- `sparse_magic_constant_is_correct` (sparse.rs:618)
- `zero_partition_yields_zero_blocks` (sparse.rs:627)

No new tests needed — this is a pure structural refactor. The existing tests
don't test the chunk loop behavior directly (they're unit tests for helper
functions). Run `cargo test` to confirm no regressions.

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, 83 tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] A `send_chunk_data` helper exists and is called from all three functions
- [ ] The chunk data-sending loop appears exactly once in the file (not three
      times)
- [ ] The `check_truncation` parameter exists for the `flash_sparse_image` path
      (where it previously used `read_exact_padded_or_truncate`)
- [ ] No files outside `src/flash/sparse.rs` are modified (`git status`)
- [ ] `plans/README.md` status row for 010 updated

## STOP conditions

- If the three functions differ in more ways than documented, stop and report
  the actual differences — they may have diverged independently and need
  individual attention, not mechanical unification.
- If `send_chunk_data` can't share the progress bar logic cleanly (because
  `flash_sparse_image` increments per chunk header+data while the others
  don't), use a callback or progress-bar parameter.
- If `sparse_wrap_file`'s raw `chunks` don't implement `SplitChunk` — check
  the type. `build_split_chunks` returns `Vec<SplitChunk>`, so they are
  `SplitChunk`.

## Maintenance notes

- Any new sparse-flash function must use the shared helpers.
- The `check_truncation` parameter exists because `flash_sparse_image` reads
  from a file that should contain exactly the amount of data the chunk header
  specifies, while the other two read from files that may be shorter (zero
  padding is expected). This subtlety should be documented in the helper.
