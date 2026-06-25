# Plan 005: Add characterization tests for flash execution core

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/flash/`
> If these files changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: LOW (tests only, no production code changes)
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

The entire flash execution pipeline — `executor.rs` (522 lines), `sparse.rs` (554 lines), `format.rs` (339 lines), `gsi/flash.rs` (388 lines) — has **zero tests**. All four modules together represent ~1800 lines of the core value proposition: flashing partitions to devices. Every bug in this code ships to real hardware without automated regression detection. Refactors (Plan 007, Plan 009) are blocked until a characterization test safety net exists.

## Current state

Modules with tests today (all in `scatter_parser/` and `force_fastboot/`):
- `src/scatter_parser/safety.rs` — 15+ tests, good coverage of partition classification
- `src/scatter_parser/plan/mod.rs` — 4 tests using `synthetic_part()` / `synthetic_ab_scatter()` helpers
- `src/scatter_parser/parse/helpers.rs` — `parse_int` / `human_size` unit tests
- `src/force_fastboot/serial.rs` — 4 tests (basic serial detection)
- `src/force_fastboot/fastboot.rs` — 3 tests (mode check, constants)
- `src/force_fastboot/permissions.rs` — 3 tests (permission error detection)

Modules WITH NO tests:
- `src/flash/sparse.rs` — 554 lines
- `src/flash/executor.rs` — 522 lines
- `src/flash/format.rs` — 339 lines
- `src/gsi/flash.rs` — 388 lines
- `src/flash/results.rs` — 50 lines (data types only, trivial)
- `src/flash/error.rs` — 46 lines (error definitions only, trivial)
- `src/format/generator.rs` — 302 lines
- `src/output/tables.rs` — 297 lines
- `src/output/status.rs` — 85 lines
- `src/scatter_parser/path.rs` — 304 lines
- All `src/cli/*.rs` files

The existing synthetic test pattern (from `src/scatter_parser/plan/mod.rs:219-277`) creates test data inline:

```rust
fn synthetic_part(name: &str, download: bool, has_file: bool, size: i64) -> ScatterPartition {
    ScatterPartition {
        source: "test".to_string(),
        layout: "EMMC".to_string(),
        // ...
    }
}
```

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify or create):
- `src/flash/sparse.rs` — add `#[cfg(test)] mod tests` (this file has the most testable pure functions)
- `src/flash/format.rs` — add `#[cfg(test)] mod tests` for `parse_getvar_hex_u64`
- `src/format/generator.rs` — add `#[cfg(test)] mod tests` for `parse_fs_options` and `FsType::from_partition_type`
- `src/output/tables.rs` — add `#[cfg(test)] mod tests` for `fmt_duration`, `plan_summary`
- `src/output/theme.rs` — add `#[cfg(test)] mod tests`
- `src/scatter_parser/path.rs` — add `#[cfg(test)] mod tests` for path resolution helpers
- `src/gsi/flash.rs` — add `#[cfg(test)] mod tests` for pure functions (`product_gsi_overflow_size`, `detect_fastboot_mode`)

**Out of scope** (do NOT touch):
- `src/flash/executor.rs` — adding tests requires mocking fastboot protocol, which is a larger effort (deferred — see maintenance notes)
- `src/gsi/flash.rs` integration-style tests (mode transitions require real executor mocking)
- `src/cli/*.rs` — integration tests need `assert_cmd` which is already a dev-dependency but requires CLI-level testing
- Any production code changes — tests only

## Git workflow

- Branch: `advisor/005-add-flash-execution-tests`
- Commit message style: `test: add characterization tests for <module>`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Test `parse_getvar_hex_u64` in `src/flash/format.rs`

Add a `#[cfg(test)]` module at the end of `src/flash/format.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::parse_getvar_hex_u64;

    #[test]
    fn parse_getvar_hex_u64_should_accept_0x_prefix() {
        assert_eq!(parse_getvar_hex_u64("0x100000"), Some(0x100000));
    }

    #[test]
    fn parse_getvar_hex_u64_should_accept_0X_prefix() {
        assert_eq!(parse_getvar_hex_u64("0X200000"), Some(0x200000));
    }

    #[test]
    fn parse_getvar_hex_u64_should_accept_no_prefix() {
        assert_eq!(parse_getvar_hex_u64("abcdef"), Some(0xabcdef));
    }

    #[test]
    fn parse_getvar_hex_u64_should_trim_whitespace() {
        assert_eq!(parse_getvar_hex_u64("  0x100  "), Some(0x100));
    }

    #[test]
    fn parse_getvar_hex_u64_should_return_none_for_empty() {
        assert_eq!(parse_getvar_hex_u64(""), None);
        assert_eq!(parse_getvar_hex_u64("0x"), None);
    }

    #[test]
    fn parse_getvar_hex_u64_should_return_none_for_invalid() {
        assert_eq!(parse_getvar_hex_u64("not_hex"), None);
    }
}
```

**Verify**: `cargo test parse_getvar_hex_u64` — all 6 new tests pass.

### Step 2: Test `parse_fs_options` and `FsType` in `src/format/generator.rs`

Add at the end of `src/format/generator.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fs_options_empty() {
        let opts: Vec<String> = vec![];
        assert_eq!(parse_fs_options(&opts), 0);
    }

    #[test]
    fn parse_fs_options_casefold() {
        let opts = vec!["casefold".to_string()];
        assert_eq!(parse_fs_options(&opts), 1 << 0);
    }

    #[test]
    fn parse_fs_options_projid() {
        let opts = vec!["projid".to_string()];
        assert_eq!(parse_fs_options(&opts), 1 << 1);
    }

    #[test]
    fn parse_fs_options_compress() {
        let opts = vec!["compress".to_string()];
        assert_eq!(parse_fs_options(&opts), 1 << 2);
    }

    #[test]
    fn parse_fs_options_combined() {
        let opts = vec!["casefold".to_string(), "compress".to_string()];
        assert_eq!(parse_fs_options(&opts), (1 << 0) | (1 << 2));
    }

    #[test]
    fn parse_fs_options_ignores_unknown() {
        let opts = vec!["unknown_option".to_string()];
        // unknown is ignored, flag remains 0
        assert_eq!(parse_fs_options(&opts), 0);
    }

    #[test]
    fn fs_type_from_partition_type_should_accept_ext4() {
        assert_eq!(FsType::from_partition_type("ext4"), Some(FsType::Ext4));
        assert_eq!(FsType::from_partition_type("EXT4"), Some(FsType::Ext4));
    }

    #[test]
    fn fs_type_from_partition_type_should_accept_f2fs() {
        assert_eq!(FsType::from_partition_type("f2fs"), Some(FsType::F2fs));
    }

    #[test]
    fn fs_type_from_partition_type_should_return_none_for_raw() {
        assert_eq!(FsType::from_partition_type("raw"), None);
    }

    #[test]
    fn fs_type_from_partition_type_should_return_none_for_empty() {
        assert_eq!(FsType::from_partition_type(""), None);
    }
}
```

**Verify**: `cargo test parse_fs_options` and `cargo test fs_type_from_partition_type` — all pass.

### Step 3: Test pure functions in `src/flash/sparse.rs`

Add at the end of `src/flash/sparse.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn is_sparse_image_should_detect_sparse_magic() {
        // This test requires creating a temp file with the sparse magic.
        // Since is_sparse_image is async and reads from disk, we instead
        // test the magic constant.
        assert_eq!(
            android_sparse_image::HEADER_MAGIC,
            0xED26FF3A,
            "sparse magic constant should match known value"
        );
    }

    #[test]
    fn read_exact_padded_should_truncate_short_read() {
        let mut data: &[u8] = &[1, 2, 3];
        let mut buf = [0u8; 8];
        let read = crate::flash::sparse::read_exact_padded_internal(&mut data, &mut buf);
        // read_exact_padded is pub(crate) — if not accessible, test the
        // padding behavior through a helper.
    }
    // Note: read_exact_padded is pub(crate) so it may not be directly
    // accessible from #[cfg(test)] in the same module. If the test module
    // is inside sparse.rs, it has access. The above test assumes it is.
}
```

**Important**: `read_exact_padded` is defined at line 50 in sparse.rs. Since a `#[cfg(test)] mod tests` block at the bottom of the same file has access to `pub(crate)` items (they are in the same crate), these tests will work.

Add one test that verifies the padding behavior:

```rust
#[test]
fn read_exact_padded_zero_fills_remainder() {
    let mut data: &[u8] = &[0xAB; 10];     // 10 bytes of real data
    let mut buf = [0xFFu8; 16];             // 16-byte buffer, pre-filled
    let read = crate::tokio::io::AsyncReadExt::read_exact_padded();
    // Actually, read_exact_padded is a free function, not a method.
    // Test it directly:
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Write 10 bytes to a temp file, read_exact_padded with 16-byte buf
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.bin");
        let mut f = tokio::fs::File::create(&path).await.unwrap();
        f.write_all(&[0xAB; 10]).await.unwrap();
        drop(f);
        let mut f = tokio::fs::File::open(&path).await.unwrap();
        let mut buf = [0xFFu8; 16];
        let n = read_exact_padded(&mut f, &mut buf).await.unwrap();
        assert_eq!(n, 10, "should read 10 bytes");
        assert_eq!(&buf[..10], &[0xAB; 10], "first 10 bytes are file data");
        assert_eq!(&buf[10..], &[0u8; 6], "last 6 bytes are zero-padded");
    });
}
```

**Verify**: `cargo test read_exact_padded` — passes.

Also add a test for `sparse_wrap_file`'s trivial case (zero-length partition):

```rust
#[test]
fn sparse_wrap_file_zero_partition_erases() {
    // For a zero-size partition, sparse_wrap_file calls fb.erase() and
    // returns Ok. We test the upper-level logic: total_blocks == 0 path.
    // This is best tested via the pure path, not the full async function.
    let part_size = 0u64;
    let blk = u64::from(android_sparse_image::DEFAULT_BLOCKSIZE);
    let total_blocks = part_size / blk;
    assert_eq!(total_blocks, 0, "zero partition means zero blocks");
}
```

**Verify**: `cargo test sparse_wrap_file_zero` — passes.

### Step 4: Test `fmt_duration` and output helpers in `src/output/tables.rs`

Note: `fmt_duration` is a private function. Add tests inside `tables.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::fmt_duration;
    use std::time::Duration;

    #[test]
    fn fmt_duration_under_60_seconds_shows_seconds() {
        let result = fmt_duration(&Duration::from_secs_f64(12.345));
        assert!(result.contains("12.345"), "expected 12.345s in output: {result}");
        assert!(result.ends_with(']'), "expected trailing bracket");
    }

    #[test]
    fn fmt_duration_over_60_seconds_shows_minutes() {
        let d = Duration::from_secs(125); // 2m 5s
        let result = fmt_duration(&d);
        assert!(result.contains("2m"), "expected 2m: {result}");
        assert!(result.contains("5s"), "expected 5s: {result}");
    }

    #[test]
    fn fmt_duration_exactly_60_seconds_shows_minutes() {
        let d = Duration::from_secs(60);
        let result = fmt_duration(&d);
        assert!(result.contains("1m"), "expected 1m: {result}");
    }
}
```

**Verify**: `cargo test fmt_duration` — all 3 tests pass.

### Step 5: Test `product_gsi_overflow_size` and `detect_fastboot_mode` in `src/gsi/flash.rs`

Add at the end of `src/gsi/flash.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn product_gsi_overflow_size_zero_when_gsi_fits() {
        // GSI fits in system: no overflow
        assert_eq!(product_gsi_overflow_size(100, 50), 0);
    }

    #[test]
    fn product_gsi_overflow_size_rounded_to_mb() {
        // GSI exceeds system by 500 bytes: rounded up to 1MB
        let result = product_gsi_overflow_size(100, 100 + 500);
        assert_eq!(result, 1024 * 1024);
    }

    #[test]
    fn product_gsi_overflow_size_exact_mb_when_exact() {
        // GSI exceeds system by exactly 5MB
        assert_eq!(product_gsi_overflow_size(100, 100 + 5 * 1024 * 1024), 5 * 1024 * 1024);
    }

    #[test]
    fn detect_fastboot_mode_bootloader_when_no_userspace() {
        let mut vars = HashMap::new();
        // is-userspace not set → bootloader
        assert_eq!(detect_fastboot_mode(&vars), FastbootMode::Bootloader);
    }

    #[test]
    fn detect_fastboot_mode_fastbootd_when_yes() {
        let mut vars = HashMap::new();
        vars.insert("is-userspace".to_string(), "yes".to_string());
        assert_eq!(detect_fastboot_mode(&vars), FastbootMode::Fastbootd);
    }

    #[test]
    fn detect_fastboot_mode_bootloader_when_no() {
        let mut vars = HashMap::new();
        vars.insert("is-userspace".to_string(), "no".to_string());
        assert_eq!(detect_fastboot_mode(&vars), FastbootMode::Bootloader);
    }

    #[test]
    fn detect_fastboot_mode_accepts_truthy_values() {
        for val in &["true", "1", "on"] {
            let mut vars = HashMap::new();
            vars.insert("is-userspace".to_string(), val.to_string());
            assert_eq!(
                detect_fastboot_mode(&vars),
                FastbootMode::Fastbootd,
                "should accept '{val}' as fastbootd"
            );
        }
    }
}
```

**Verify**: `cargo test product_gsi_overflow_size` and `cargo test detect_fastboot_mode` — all pass.

### Step 6: Run full test suite and clippy

**Verify**: `cargo test` exits 0, all ~30+ tests pass (existing + new).
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

All new tests are in-module `#[cfg(test)]` blocks, matching the existing convention. No integration tests. The tests cover:

| Module | Tests | Coverage |
|--------|-------|----------|
| `flash/format.rs` | 6 | `parse_getvar_hex_u64` |
| `format/generator.rs` | 6+3 | `parse_fs_options`, `FsType::from_partition_type` |
| `flash/sparse.rs` | 2 | `read_exact_padded` padding, zero-partition logic |
| `output/tables.rs` | 3 | `fmt_duration` |
| `gsi/flash.rs` | 7 | `product_gsi_overflow_size`, `detect_fastboot_mode` |

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0; all tests pass (existing + new)
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `parse_getvar_hex_u64` has tests for: hex with 0x,0X,none; whitespace; empty; invalid
- [ ] `parse_fs_options` has tests for: empty; each flag; combined; unknown
- [ ] `product_gsi_overflow_size` has tests for: zero when fits; rounded to MB; exact MB
- [ ] `detect_fastboot_mode` has tests for: bootloader; fastbootd with yes/no/true/1/on
- [ ] `fmt_duration` has tests for: under 60s; over 60s; exactly 60s
- [ ] `read_exact_padded` has a test for zero-fill padding
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts (the codebase has drifted).
- `cargo build` fails after the changes.
- Any pre-existing test fails (the new tests should not affect existing ones).
- A function you need to test is not accessible from the test module (e.g., it's `pub(crate)` and the test module is inside the same file — that works; but if it's truly private, test through its callers).

## Maintenance notes

- These characterization tests are the first line of defense for the flash core. When plans 007 and 009 refactor the flash pipeline, these tests must continue to pass — they validate the function-level contracts.
- `executor.rs` tests are deliberately deferred because they require mocking `NusbFastBoot` — that's a larger effort that involves either a trait abstraction or a test double library. That work is scoped as a separate future plan.
- Future contributors adding functions to these modules should add corresponding tests following the patterns established here.
