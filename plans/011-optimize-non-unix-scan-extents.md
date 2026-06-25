# Plan 011: Optimize non-Unix `scan_extents` fallback for large partitions

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
- **Effort**: L
- **Risk**: MED
- **Depends on**: none
- **Category**: perf
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

The `scan_extents` function in `src/flash/sparse.rs` (lines 405–539) has a
non-Unix fallback path (lines 490–531) that reads every single block of a
partition sequentially to detect data vs. hole runs. For a 32 GiB partition
with 4 KiB blocks, that's 8 million `read_exact` calls — each reading 4 KiB
from a possibly-sparse file. This can take minutes on Windows, making
format-data effectively unusable there.

However, `format-data` is primarily used on Linux (where the native
`SEEK_DATA`/`SEEK_HOLE` path is O(extents)). Windows users are a secondary
concern. This is a P3 because the fix requires a fundamentally different
approach (e.g., scanning at a coarser granularity or using a sampling-based
strategy).

## Current state

```rust
// sparse.rs:490-531, non-Unix fallback
fn do_scan(
    file: &std::fs::File,
    effective_size: u64,
    blk: u64,
) -> Result<Vec<(u64, u64, bool)>> {
    use std::io::{BufReader, Read};
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut extents: Vec<(u64, u64, bool)> = Vec::new();
    let mut block_buf = vec![0u8; blk as usize];
    let scan_blocks = effective_size / blk;
    // ...
    while current_block < scan_blocks {
        reader.read_exact(&mut block_buf).map_err(|_| { ... })?;
        let is_data = !block_buf.iter().all(|&b| b == 0);
        // ... track run transitions
        current_block += 1;
    }
    // ...
}
```

The `BufReader` has a 1 MiB buffer, but the inner loop still reads block-by_block_ (each `read_exact` is a 4 KiB read from the BufReader's buffer, so the I/O is efficient — it's the CPU work of checking 4 KiB of zeros per block). For 8 million blocks, that's checking 32 GiB of zeroes byte-by-byte to decide if the block is all-zero.

The Unix path (`SEEK_DATA`/`SEEK_HOLE`) runs in O(extents) time — for a mostly-zero 32 GiB partition with a few metadata blocks, that's microseconds.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/flash/sparse.rs` — the `do_scan` non-Unix fallback

**Out of scope**:
- The Unix `do_scan` (lines 415–488) — no change
- `build_split_chunks` (lines 541–592) — no change
- Any test file — existing sparse tests should pass
- Windows-specific code outside `src/flash/sparse.rs`

## Git workflow

- Branch: `advisor/011-optimize-scan-extents-win`
- Single commit: `perf: optimize non-Unix scan_extents with block-group zero-check`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Implement a block-group scanning strategy

Replace the block-by-block inner loop with a grouped approach: read blocks in
batches of 64 or 128 (configurable), check the batch for all-zeros using
`byteorder` or `chunks_exact` for fast comparison. This reduces the number of
zero-check calls by 64–128× while keeping the same I/O pattern.

The key optimization: use `chunks_exact(8)` to check 8 bytes at a time as
`u64` values instead of byte-by-byte. Combined with batch reading:

```rust
fn is_all_zero(buf: &[u8]) -> bool {
    // Check 8 bytes at a time as u64 using chunks_exact
    buf.chunks_exact(8).all(|c| u64::from_ne_bytes(c.try_into().unwrap()) == 0)
        && buf.chunks_exact(8).remainder().iter().all(|&b| b == 0)
}
```

Then the scanning loop becomes:

```rust
const BLOCKS_PER_BATCH: u64 = 128;
let batch_bytes = blk * BLOCKS_PER_BATCH;
let mut batch_buf = vec![0u8; batch_bytes as usize];
let scan_blocks = effective_size / blk;

while current_block < scan_blocks {
    let blocks_this_batch = (scan_blocks - current_block).min(BLOCKS_PER_BATCH);
    let bytes_this_batch = blocks_this_batch * blk;

    reader.read_exact(&mut batch_buf[..bytes_this_batch as usize])?;

    // Scan individual blocks within the batch
    for block_idx in 0..blocks_this_batch {
        let start = (block_idx * blk) as usize;
        let end = start + blk as usize;
        let block_data = &batch_buf[start..end];
        let is_data = !is_all_zero(block_data);
        // ... track run transitions (same as current logic)
    }
    current_block += blocks_this_batch;
}
```

**Verify**: `cargo build` exits 0.

### Step 2: Benchmark the optimization

On a development machine, create a 1 GiB sparse file with known data/hole
layout and run both implementations (use a `#[cfg(test)]` benchmark):

```rust
#[cfg(test)]
mod bench {
    use super::*;

    #[test]
    fn scan_extents_benchmark() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("scan_test.img");
        // Create a 256 MiB sparse file with 4 MiB of data at the start
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(256 * 1024 * 1024).unwrap();
        drop(f);

        let start = std::time::Instant::now();
        let result = scan_extents(&path, 256 * 1024 * 1024, 256 * 1024 * 1024, 4096);
        let elapsed = start.elapsed();
        assert!(result.is_ok());
        // On Unix, this uses SEEK_DATA/SEEK_HOLE. On non-Unix, the
        // fallback should still complete within a reasonable time.
        // This is informational, not a hard assertion.
        eprintln!("scan_extents took {elapsed:?}");
    }
}
```

**Verify**: `cargo test scan_extents_benchmark` runs and prints timing.

### Step 3: Run existing tests

**Verify**:
- `cargo test` — all 83 tests pass (only existing sparse tests plus the new one)
- `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean

## Test plan

- New benchmark test in `sparse.rs` `#[cfg(test)]` module
- Existing 3 sparse tests continue to pass
- No integration test needed (the function is only called from
  `sparse_wrap_file` in `format.rs`, which requires a real fastboot device)

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0; 84+ tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] The non-Unix `do_scan` no longer calls `read_exact` once per 4 KiB block
      at the application level (reads are batched)
- [ ] An `is_all_zero` helper using `u64` word comparison exists
- [ ] A benchmark test exists that measures scan_extents performance
- [ ] The Unix `SEEK_DATA`/`SEEK_HOOK` path is unchanged
- [ ] No files outside `src/flash/sparse.rs` are modified (`git status`)
- [ ] `plans/README.md` status row for 011 updated

## STOP conditions

- If the grouped reading approach changes correctness for partitions where
  the last batch is not a multiple of `BLOCKS_PER_BATCH` — verify the
  remainder logic by testing with non-power-of-2 partition sizes.
- If the Windows platform has different block alignment requirements (e.g.,
  `blk` is not always 4096) — verify formats where `logical-block-size`
  returns 512 or 2048.
- If `is_all_zero` using `u64` casts is slower than the existing byte-by-byte
  approach on the target platform — run the benchmark on Windows to confirm
  improvement. If regression, keep the existing code.

## Maintenance notes

- The Unix `SEEK_DATA`/`SEEK_HOLE` path (which covers >95% of real usage) is
  unaffected.
- If Windows fastboot support becomes a primary goal, consider porting the
  `SEEK_DATA`/`SEEK_HOLE` approach to Windows via `FSCTL_GET_RETRIEVAL_POINTERS`.
- The batch size (128 blocks = 512 KiB) is a reasonable default; if profiling
  shows cache-miss issues, halve it.
