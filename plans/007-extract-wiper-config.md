# Plan 007: Extract `WiperConfig` to eliminate 10-parameter `wipe_partition`

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/flash/format.rs`
> If this file changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

`wipe_partition` (src/flash/format.rs:248) takes 10 parameters — well above
the clippy default limit of 7. The function is called from a single site
(format_data, line 110). The repetition of `max_download`, `erase_blk`,
`logical_blk`, `tools_dir`, `clean_test`, `footer_size`, and
`fs_type_override` across the call and the function signature adds noise that
obscures what actually differs per partition (just the partition name and
`footer_size`).

## Current state

```rust
// format.rs:248
async fn wipe_partition(
    &mut self,
    partition: &str,
    fs_options: u32,
    max_download: u32,
    erase_blk: u32,
    logical_blk: u32,
    tools_dir: &Path,
    clean_test: bool,
    footer_size: u64,
    fs_type_override: Option<FsType>,
) -> FormatOutcome {
```

Called from `format_data`:
```rust
// format.rs:110-121
let outcome = self
    .wipe_partition(
        partition,
        fs_options,
        max_download,
        erase_blk,
        logical_blk,
        &tools_dir,
        clean_test,
        footer_size,
        fs_type_override,
    )
    .await;
```

Most of these parameters are fixed across all three partitions (userdata,
metadata, cache) — only `partition`, `footer_size` (depends on partition +
fs_type), and `fs_type_override` vary. The repetitive pass-through makes the
code fragile: transposing two `u32` arguments would compile but produce wrong
behavior.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/flash/format.rs` — refactor `wipe_partition` and `format_data`
- Optionally: `src/flash/mod.rs` — if `WiperConfig` type needs re-export

**Out of scope**:
- `src/flash/error.rs` — error types stay unchanged
- `src/flash/results.rs` — `FormatOutcome` stays unchanged
- `src/flash/sparse.rs` — not touched
- Any test file

## Git workflow

- Branch: `advisor/007-wiper-config`
- Single commit: `refactor: extract WiperConfig to reduce wipe_partition parameters`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Define `WiperConfig` struct

Add to the top of `src/flash/format.rs` (or a new `pub(super)` type in
`flash/types.rs` if one exists — check first; otherwise add to `format.rs`):

```rust
/// Shared configuration for wiping a single partition.
struct WiperConfig<'a> {
    fs_options: u32,
    max_download: u32,
    erase_blk: u32,
    logical_blk: u32,
    tools_dir: &'a Path,
    clean_test: bool,
    fs_type_override: Option<FsType>,
}
```

Place it just before the `impl FlashExecutor` block, outside the impl so it's
file-private. Use `pub(crate)` only if needed by tests.

**Verify**: `cargo build` exits 0.

### Step 2: Refactor `wipe_partition`

Change `wipe_partition` to take `cfg: &WiperConfig<'_>` instead of the 7
boilerplate parameters, keeping only the truly varying ones as direct args:

```rust
async fn wipe_partition(
    &mut self,
    partition: &str,
    footer_size: u64,
    cfg: &WiperConfig<'_>,
) -> FormatOutcome {
    debug!(%partition, "wipe_partition: querying partition type");
    let partition_type = match self.partition_type(partition).await {
        Ok(t) => t,
        Err(outcome) => return outcome,
    };

    info!(%partition, "erasing");
    if let Err(e) = self.fb.erase(partition).await {
        return FormatOutcome { /* ... */ };
    }

    let fs_type = match Self::determine_fs_type(partition, &partition_type, cfg.fs_type_override) {
        Ok(t) => t,
        Err(outcome) => return outcome,
    };

    if cfg.clean_test { /* ... */ }

    // ... rest of the function using cfg.fs_options, cfg.max_download, etc.
}
```

Replace every use of the old parameter names inside the function body:
- `fs_options` → `cfg.fs_options`
- `max_download` → `cfg.max_download`
- `erase_blk` → `cfg.erase_blk`
- `logical_blk` → `cfg.logical_blk`
- `tools_dir` → `cfg.tools_dir`
- `clean_test` → `cfg.clean_test`

**Verify**: `cargo build` exits 0.

### Step 3: Update the call site in `format_data`

In `format_data` (around line 110), construct a `WiperConfig` once before the
partition loop and use it for all three partitions:

```rust
let wiper_cfg = WiperConfig {
    fs_options,
    max_download,
    erase_blk,
    logical_blk,
    tools_dir: &tools_dir,
    clean_test,
    fs_type_override,
};

for partition in &partitions {
    let footer_size = match (*partition, fs_type_override.unwrap_or(FsType::F2fs)) {
        ("userdata", FsType::Ext4) => CRYPT_FOOTER_OFFSET,
        _ => 0,
    };

    let outcome = self
        .wipe_partition(partition, footer_size, &wiper_cfg)
        .await;
    // ...
}
```

**Verify**: `cargo build` exits 0.

### Step 4: Run tests and lint

**Verify**:
- `cargo test` — all 83 tests pass
- `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean
  (specifically, no `too_many_arguments` on `wipe_partition`)

## Test plan

No behavior changes — pure refactor. The only test in `flash/format.rs` is
`parse_getvar_hex_u64` tests (lines 346–379). Run `cargo test` to confirm.

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, 83 tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] `wipe_partition` has at most 4 parameters (`&mut self` excluded): the
      partition name, footer_size, and the `&WiperConfig<'_>` reference
- [ ] `grep -n "fn wipe_partition" src/flash/format.rs` shows ≤ 4 parameters (excluding self)
- [ ] No files outside `src/flash/format.rs` are modified (`git status`)
- [ ] `plans/README.md` status row for 007 updated

## STOP conditions

- If `WiperConfig<'a>` borrows from a temporary that doesn't live long enough
  — the config must outlive the `for` loop. Verify the struct is constructed
  before the loop.
- If `fs_type_override` is moved into the struct but also needed outside the
  loop — keep it in the struct and access via `cfg.fs_type_override` everywhere.
- If `cargo test` fails: clippy does not have `too_many_arguments` at deny
  level in `Cargo.toml`, so it shouldn't block compilation. But if any test
  fails, it's likely a logic error in the parameter extraction.

## Maintenance notes

- When adding a new parameter to `wipe_partition`, add it to `WiperConfig`
  rather than the function signature. If the parameter applies only to one
  partition, keep it as a direct function argument.
- The same pattern (`WiperConfig`) can be applied elsewhere if other functions
  have similar parameter clusters (e.g., `parse_max_download` usage).
- If the struct grows beyond 5 fields, consider grouping subsections (e.g.,
  `DeviceConfig { erase_blk, logical_blk, max_download }`).
