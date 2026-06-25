# Plan 008: Add integration tests for flash pipeline

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/flash/ src/gsi/ Cargo.toml`
> If any of these paths changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: MED
- **Depends on**: Plan 001 (CI gates must exist to run tests in CI)
- **Category**: tests
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

The flash pipeline (`FlashExecutor`, sparse handling, format-data, GSI flash)
is the most critical and dangerous code in the project — it writes to real
devices. Yet `flash/executor.rs` (618 lines), `flash/sparse.rs` (633 lines),
`flash/format.rs` (379 lines), and `gsi/flash.rs` (459 lines) have **zero
tests** beyond a handful of utility function tests. Every refactor of this
code is blind. Adding a mock fastboot transport and integration-level tests
makes the entire flash pipeline testable without a physical device.

## Current state

The codebase has 83 tests, all in-module `#[cfg(test)]` unit tests. The test
coverage is concentrated in:
- `scatter_parser/parse/helpers.rs` — parse_int, human_size, scalar_json (good)
- `scatter_parser/safety.rs` — classification (good)
- `scatter_parser/plan/mod.rs` — plan builder (good)
- `flash/sparse.rs` — read_exact_padded + magic constant (minimal)
- `flash/format.rs` — parse_getvar_hex_u64 (minimal)
- `force_fastboot/serial.rs` — candidate port matching (good)
- `force_fastboot/fastboot.rs` — constants (minimal)
- `force_fastboot/permissions.rs` — permission detection (good)
- `format/generator.rs` — fs_options parsing (good)
- `gsi/flash.rs` — overflow calculation + mode detection (good)
- `output/tables.rs` — duration formatting (good)

There are no integration tests (no `tests/` directory). The `Cargo.toml` has
`assert_cmd` and `predicates` in `[dev-dependencies]` but they're unused.

The `NusbFastBoot` type from the vendored `fastboot-rs` crate is the transport
that talks to real hardware. The vendored `fastboot-protocol` crate has a
trait or struct for this — check the vendored source for a test mock.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/tests/` — new directory for integration tests (or `tests/` at crate
  root, following the existing pattern of no integration tests)
- `Cargo.toml` — add `mockall` or manual mock helpers to `[dev-dependencies]`
- `src/flash/executor.rs` — add `#[cfg(test)]` test module with mock transport
- `src/flash/sparse.rs` — add `#[cfg(test)]` tests using mock fastboot
- `src/flash/format.rs` — add `#[cfg(test)]` tests using mock fastboot
- `src/gsi/flash.rs` — add `#[cfg(test)]` tests using mock executor

**Out of scope**:
- `src/flash/diagnostics.rs` — Linux sysfs diagnostics, not testable in CI
- `src/force_fastboot/` — real hardware serial/USB, not mockable without a
  hardware abstraction layer (too large a refactor for this plan)
- `src/format/generator.rs` — already has good unit tests
- `src/output/` — UI rendering, not worth mocking
- Vendored `fastboot-rs` crate — do not modify
- Existing 83 unit tests — do not modify

## Git workflow

- Branch: `advisor/008-flash-integration-tests`
- Commit per major test file; message style: conventional commits
  (`test: add mock fastboot transport for flash pipeline tests`)
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add a mock fastboot transport

Check the vendored `fastboot-protocol` crate to see if `NusbFastBoot` has a
trait that could be mocked. Look in `vendor/fastboot-rs/fastboot-protocol/src/`
for the public API. If `NusbFastBoot` is a concrete struct (no trait), create
a `#[cfg(test)]` mock struct in a new test utility module.

Create `src/test_util.rs` (only compiled in test mode):

```rust
#[cfg(test)]
pub(crate) mod test_util {
    /// A mock fastboot transport that records commands and returns
    /// canned responses. Used for integration tests of the flash pipeline.
    pub(crate) struct MockFastBoot {
        responses: HashMap<String, Result<String, MockError>>,
        commands: Vec<String>,
    }

    impl MockFastBoot {
        pub fn new() -> Self { /* ... */ }
        pub fn with_response(mut self, cmd: &str, resp: &str) -> Self { /* ... */ }
        pub fn get_var(&mut self, var: &str) -> Result<String> { /* ... */ }
    }
}
```

The mock must implement the subset of `NusbFastBoot` methods that
`FlashExecutor` uses: `get_var`, `get_all_vars`, `download`, `flash`,
`erase`, `reboot`, `reboot_to`, `is_logical`, `resize_logical_partition`,
`flashing`, `set_active`, `snapshot_update`.

If `NusbFastBoot` has a trait in the vendored crate, implement the trait for
the mock. Otherwise, make `FlashExecutor` generic over the transport type, or
use `#[cfg(test)]` conditional compilation to swap in the mock.

**Preferred approach**: Create a `FastBootTransport` trait in
`src/flash/transport.rs` that `NusbFastBoot` implements, then make
`FlashExecutor` generic over `T: FastBootTransport`. In tests, use
`FlashExecutor<MockFastBoot>`.

```rust
// src/flash/transport.rs — new file
#[cfg(not(test))]
pub(crate) type RealTransport = NusbFastBoot;

pub(crate) trait FastBootTransport {
    // ... methods FlashExecutor needs
}
```

**Verify**: `cargo test --lib` builds and all existing tests pass. The mock
doesn't need to be functional yet — just compiling.

### Step 2: Make `FlashExecutor` generic over transport

Change `FlashExecutor` from:
```rust
pub struct FlashExecutor {
    pub(crate) fb: NusbFastBoot,
    device_vars: HashMap<String, String>,
}
```
to:
```rust
pub struct FlashExecutor<T: crate::flash::transport::FastBootTransport> {
    pub(crate) fb: T,
    device_vars: HashMap<String, String>,
}
```

Update all methods that take `&mut self` — they're generic already since `self`
includes the type parameter. Update `connect()` to create a `NusbFastBoot` in
production builds and a `MockFastBoot` in test builds (or use a `connect_test()`
helper).

**Verify**: `cargo build` exits 0. This is a significant refactor — if it
touches too many files, fall back to the `#[cfg(test)]` conditional mock
approach instead.

### Step 3: Write integration tests for `FlashExecutor::execute_plan`

Create `src/flash/executor.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::test_util::MockFastBoot;

    #[tokio::test]
    async fn execute_plan_happy_path() {
        let mock = MockFastBoot::new()
            .with_response("getvar:max-download-size", "0x10000000")
            .with_download_accept(1024);
        let mut executor = FlashExecutor { fb: mock, device_vars: HashMap::new() };
        let plan = FlashPlan { /* minimal plan with one flash action */ };
        let result = executor.execute_plan(&plan, false, None).await;
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.failed, 0);
    }

    #[tokio::test]
    async fn execute_plan_handles_download_failure() { /* ... */ }

    #[tokio::test]
    async fn execute_plan_handles_flash_failure() { /* ... */ }

    #[tokio::test]
    async fn execute_plan_skips_missing_images() { /* ... */ }

    #[tokio::test]
    async fn execute_plan_dry_run_does_not_download() { /* ... */ }
}
```

Test cases to cover:
1. Happy path: single partition flash succeeds
2. Multi-partition: all succeed
3. Download failure: one partition fails, others continue
4. Flash failure: one partition fails, others continue
5. Missing image (resolved_path is None): skipped gracefully
6. Dry run: no download/flash commands sent
7. Progress bar callback is invoked (optional — use a test spy)

**Verify**: `cargo test flash::executor::tests` runs all new tests and passes.

### Step 4: Write integration tests for format-data

In `src/flash/format.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn format_data_skips_when_not_in_fastbootd() { /* ... */ }

    #[tokio::test]
    async fn format_data_cancels_snapshots() { /* ... */ }

    #[tokio::test]
    async fn format_data_clears_bcb() { /* ... */ }

    #[tokio::test]
    async fn format_data_wipes_partitions() { /* ... */ }

    #[tokio::test]
    async fn format_data_handles_missing_partition_type() { /* ... */ }

    #[tokio::test]
    async fn format_data_handles_extraction_failure() { /* ... */ }
}
```

Test cases:
1. Partition type query returns expected types
2. Erase succeeds
3. Filesystem generation fails (mock tool_dir with missing tools)
4. Sparse wrap succeeds
5. Footer offset applied for ext4 userdata, not for f2fs

**Verify**: `cargo test flash::format::tests` passes.

### Step 5: Write integration tests for sparse flash

In `src/flash/sparse.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn is_sparse_image_detects_sparse_magic() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sparse.img");
        // Write a minimal sparse image header
        let mut f = tokio::fs::File::create(&path).await.unwrap();
        f.write_all(&0xED26FF3Au32.to_le_bytes()).await.unwrap();
        drop(f);
        assert!(is_sparse_image(&path).await.unwrap());
    }

    #[tokio::test]
    async fn sparse_wrap_file_handles_zero_partition() { /* ... */ }

    #[tokio::test]
    async fn flash_sparse_image_sends_chunks() { /* ... */ }

    #[tokio::test]
    async fn flash_sparse_wrapped_handles_large_image() { /* ... */ }
}
```

**Verify**: `cargo test flash::sparse::tests` passes.

### Step 6: Write integration tests for GSI flash

In `src/gsi/flash.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gsi_flash_from_bootloader_mode() { /* ... */ }

    #[tokio::test]
    async fn gsi_flash_from_fastbootd_mode() { /* ... */ }

    #[tokio::test]
    async fn gsi_flash_with_overflow_creates_product_gsi() { /* ... */ }

    #[tokio::test]
    async fn gsi_flash_cancellation_stops_early() { /* ... */ }
}
```

These are more involved — they need a mock `FlashExecutor` that records its
method calls. Use `MockFlashExecutor` or make the GSI functions generic over
`FlashExecutor` methods.

**Verify**: `cargo test gsi::flash::tests` passes.

## Test plan

The new integration tests are the deliverable. Run:
- `cargo test` — all 83 existing + N new tests pass
- `cargo test flash::executor::tests` — new mock-based tests pass

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0; at least 15 new tests for the flash pipeline exist
      and pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] A mock fastboot transport exists (either via trait or `#[cfg(test)]` conditional)
- [ ] `FlashExecutor` can be instantiated in tests without a real USB device
- [ ] Test coverage includes: flash plan execution, format-data, sparse image,
      and GSI flash happy paths + error paths
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for 008 updated

## STOP conditions

Stop and report (do not improvise) if:

- The vendored `NusbFastBoot` struct has no trait and is concrete —
  making `FlashExecutor` generic requires changing its public API and all
  call sites. If this is too invasive, fall back to `#[cfg(test)]` conditional
  mock that replaces `fb` with an enum `enum Transport { Real(NusbFastBoot), Mock(MockFastBoot) }`.
- The vendored `fastboot-protocol` crate does not compile with test features
  or has conflicting dependencies.
- `cargo test` fails on existing tests after the transport abstraction — it
  means the abstraction leaked behavior.
- The `assert_cmd` dev-dependency was intended for CLI tests but the CLI has
  no real test infrastructure — that's out of scope for this plan.

## Maintenance notes

- After this plan, any new flash pipeline feature must include tests using the
  mock transport. Document the testing pattern in AGENTS.md.
- The mock transport should be maintained alongside `NusbFastBoot` — if the
  vendored crate adds new methods, the mock must be updated.
- If the vendored `fastboot-rs` upstream adds a public test mock, switch to it.
- The `#[cfg(test)]` conditional approach is simpler than a generic parameter
  but doesn't scale to multiple mock behaviors. If more sophistication is
  needed, migrate to a trait-based approach.
