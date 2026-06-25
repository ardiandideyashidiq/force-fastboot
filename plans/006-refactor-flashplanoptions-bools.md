# Plan 006: Refactor `FlashPlanOptions` bools into logical enums

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/scatter_parser/types.rs src/scatter_parser/plan/`
> If these files changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: tech-debt
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

`FlashPlanOptions` (src/scatter_parser/types.rs:193) has 8 boolean fields:
`show`, `full_json`, `dry_run`, `json`, `check_images`, `image_search`,
`include_preloader`, `allow_incomplete_slots`, `clean`, `no_format`,
`clean_test`. This triggers clippy's `struct_excessive_bools` lint. More
importantly, the bools encode invalid states — e.g., `show` and `dry_run`
both true is a nonsense combination, `clean` and `no_format` simultaneously
is contradictory. Replacing groups of bools with enums makes invalid states
unrepresentable and improves readability.

## Current state

```rust
// types.rs:193
pub struct FlashPlanOptions {
    pub mode: Mode,
    pub storage: StorageSelect,
    pub parts: Vec<String>,
    pub groups: Vec<String>,
    pub exclude: Vec<String>,
    pub firmware_dir: Option<PathBuf>,
    pub package_root: Option<PathBuf>,
    pub check_images: bool,
    pub image_search: bool,
    pub include_preloader: bool,
    pub allow_incomplete_slots: bool,
    pub clean: bool,
}
```

Callers construct `FlashPlanOptions` in three places:
- `cli/flash.rs:159-174` (ScatterConfig → run_scatter)
- `cli/flash.rs:160` (`check_images` from `cfg`)
- `cli/interactive.rs:86-97` (uses `..Default::default()`)

The `Mode` enum already exists and handles `DryRun`, `Selective`, `DirtyFlash`
correctly — that's the pattern to follow.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/scatter_parser/types.rs` — `FlashPlanOptions` struct, add new enums
- `src/scatter_parser/plan/mod.rs` or `src/scatter_parser/plan/mode.rs` (if
  Plan 005 merged first) — update `build_flash_plan` and helpers
- `src/cli/flash.rs` or `src/cli/flash/scatter.rs` (if Plan 004 merged first)
  — update `ScatterConfig` construction
- `src/cli/interactive.rs` — update `run()` where `..Default::default()` is
  used
- `src/cli/args.rs` — the CLI arg definitions (e.g. `--check-images` maps to
  a bool flag)

**Out of scope**:
- Any test file — existing tests should pass without changes
- `FlashPlan` struct — no change to serialization
- `Mode` or `StorageSelect` enums — keep as-is

## Git workflow

- Branch: `advisor/006-enum-flashplanoptions`
- Commit per step; message style: conventional commits
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Define enums in `types.rs`

Add the following enums above `FlashPlanOptions` in `src/scatter_parser/types.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageCheckMode {
    #[default]
    No,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageSearchMode {
    #[default]
    No,
    Yes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreloaderMode {
    #[default]
    Exclude,
    Include,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlotIncompletePolicy {
    #[default]
    Error,
    Allow,
}
```

Replace the bool fields in `FlashPlanOptions`:

```rust
pub struct FlashPlanOptions {
    pub mode: Mode,
    pub storage: StorageSelect,
    pub parts: Vec<String>,
    pub groups: Vec<String>,
    pub exclude: Vec<String>,
    pub firmware_dir: Option<PathBuf>,
    pub package_root: Option<PathBuf>,
    pub check_images: ImageCheckMode,
    pub image_search: ImageSearchMode,
    pub include_preloader: PreloaderMode,
    pub allow_incomplete_slots: SlotIncompletePolicy,
    pub clean: bool,
}
```

Keep `clean` as bool — it's a single concern and the remaining bool count
(just `clean`) is well within the clippy limit.

Also add re-exports in `src/scatter_parser/mod.rs` if they need to be public:
```rust
pub use types::{ImageCheckMode, ImageSearchMode, PreloaderMode, SlotIncompletePolicy};
```

**Verify**: `cargo build` exits 0.

### Step 2: Update all call sites

Find every reference to the replaced fields with:
```
git grep -n "check_images\|image_search\|include_preloader\|allow_incomplete_slots" src/
```

Expected call sites:
1. `src/cli/args.rs` — the `--check-images`, `--image-search`,
   `--include-preloader`, `--allow-incomplete-slots` CLI args currently produce
   `bool`. Keep the CLI args as `bool` and convert at the boundary:
   ```rust
   let options = FlashPlanOptions {
       check_images: if check_images { ImageCheckMode::Yes } else { ImageCheckMode::No },
       ...
   };
   ```

2. `src/cli/flash.rs` (or `flash/scatter.rs`) — `ScatterConfig` construction,
   pass enum values from CLI args:
   ```rust
   check_images: if cfg.check_images { ImageCheckMode::Yes } else { ImageCheckMode::No },
   ```

3. `src/cli/interactive.rs` — uses `..Default::default()` so no change needed
   (enums derive `Default`).

4. `src/scatter_parser/plan/mod.rs` (or submodules after Plan 005) — in
   `mode_allows_partition`: change `include_preloader: bool` parameter to
   `PreloaderMode`. In `build_flash_plan`: change comparisons:
   ```rust
   if options.image_search == ImageSearchMode::Yes { ... }
   if options.check_images == ImageCheckMode::Yes { ... }
   ```

5. `apply_exclude_filter` and `check_incomplete_slots` — update parameter
   type from `bool` to `SlotIncompletePolicy`.

**Verify**: `cargo build` exits 0 after each call site update.

### Step 3: Run tests and lint

**Verify**:
- `cargo test` — all 83 tests pass
- `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean
  (specifically, no more `struct_excessive_bools` on `FlashPlanOptions`)

## Test plan

No behavioral changes — this is a type-level refactor. The existing plan
builder tests (6 tests) cover the observable behavior. Run `cargo test`.

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, 83 tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] `FlashPlanOptions` has at most 1 boolean field (the `clean` field)
- [ ] `grep -r "struct_excessive_bools" src/` returns no matches (unless the
  lint was disabled upstream)
- [ ] `grep -r "ImageCheckMode" src/scatter_parser/types.rs` finds the enum
- [ ] `grep -r "PreloaderMode" src/scatter_parser/types.rs` finds the enum
- [ ] `grep -r "SlotIncompletePolicy" src/scatter_parser/types.rs` finds the enum
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for 006 updated

## STOP conditions

- If the `FlashPlanOptions` is serialized somewhere (e.g., into `FlashPlan.options`
  as JSON), the enum values must serialize correctly. Check `build_flash_plan`:
  the `options` field is constructed with `json!({ ... })` — the bool values
  fed into `json!` must produce the same JSON as before. Use `Value::Bool`
  explicitly when building the JSON, or add serialization support to the enums.
- If `FlashPlanOptions` derives `Serialize`/`Deserialize`, add `serde` derives
  to the new enums.

## Maintenance notes

- When adding new boolean options to `FlashPlanOptions`, prefer a two-variant
  enum over a bool. The existing enums (`ImageCheckMode`, `PreloaderMode`,
  etc.) serve as templates.
- The `clean: bool` remaining in the struct is intentionally kept as bool
  because it's a single, well-understood concern. If another boolean needs to
  be added, consider grouping related booleans into an enum.
- The CLI flags in `args.rs` still produce `bool` — that's correct for CLI UX.
  The conversion happens at the boundary between CLI args and `FlashPlanOptions`.
