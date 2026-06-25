# Plan 004: Split `cli/flash.rs` into submodule

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/cli/flash.rs src/cli/mod.rs`
> If these files changed since this plan was written, compare the "Current
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

`src/cli/flash.rs` is 390 lines — just 10 lines shy of the 400-line convention
in AGENTS.md. It dispatches three very different workflows (scatter show/plan/execute,
GSI flash, raw image flash) through one `run()` function. Adding any new flash
subcommand will push it over the limit. Splitting now keeps the codebase
consistent and makes each path independently testable.

## Current state

`src/cli/flash.rs` contains:
- `run()` (lines 48–130) — top-level dispatcher matching `FlashAction::Scatter`,
  `FlashAction::Gsi`, and `None` (raw image)
- `run_scatter()` (lines 134–249) — scatter show/plan/execute orchestration
- `show_scatter_metadata()` (lines 253–286) — displays scatter metadata
- `print_plan()` (lines 290–319) — prints a flash plan
- `run_raw_image()` (lines 324–390) — raw partition flash
- `ScatterConfig` struct (lines 13–31)

The convention from AGENTS.md: "Keep files focused and under ~400 lines. If a
file grows beyond that, split it into a directory module with submodules."

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/cli/flash.rs` — split into submodules
- `src/cli/mod.rs` — update to export new submodule

**Out of scope**:
- Any other `src/cli/*.rs` file — do not modify
- `src/flash/executor.rs` or any flash/ module
- `src/scatter_parser/` modules

## Git workflow

- Branch: `advisor/004-split-cli-flash`
- Single commit: `refactor: split cli/flash.rs into scatter/raw/gsi submodules`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Create directory submodule

Replace `src/cli/flash.rs` with a directory `src/cli/flash/` containing:

```
src/cli/flash/
  mod.rs      ← public API re-exports + `run()` dispatcher
  scatter.rs  ← ScatterConfig, run_scatter, show_scatter_metadata, print_plan
  raw.rs      ← run_raw_image
```

The new `mod.rs` should contain:
- The `run()` function (lines 48–130 of the original)
- The `ScatterConfig` struct (lines 13–31)
- `pub(crate) use` re-exports from submodules so that external callers
  (`src/main.rs` calls `pawflash::cli::flash::run`) don't change

The submodule files (`scatter.rs`, `raw.rs`) keep their contents as-is,
adjusting imports to use `super::` for `ScatterConfig`.

**Important**: The `ScatterConfig` struct must stay in `mod.rs` because
`run_scatter()` in `scatter.rs` takes `&ScatterConfig<'_>` — use
`pub(super)` visibility. The `print_flash_help()` helper stays in `mod.rs`
since it's only used by `run()`.

**Verify**: `cargo build` exits 0.

### Step 2: Verify modules compile and tests pass

**Verify**:
- `cargo test` — all 83 tests pass
- `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean
- `grep -r "mod flash" src/cli/` shows the module declaration

### Step 3: Verify line count

**Verify**: `wc -l src/cli/flash/mod.rs src/cli/flash/scatter.rs src/cli/flash/raw.rs`
— each file under 400 lines.

## Test plan

No behavior changes — this is a pure structural refactor. Run `cargo test` to
confirm all existing tests pass unchanged.

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, 83 tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] `src/cli/flash.rs` no longer exists (replaced by `src/cli/flash/` directory)
- [ ] `src/cli/flash/mod.rs` is under 400 lines
- [ ] `src/cli/flash/scatter.rs` is under 400 lines
- [ ] `src/cli/flash/raw.rs` is under 400 lines
- [ ] `cargo run -- flash --help` still prints the same help text as before
- [ ] No files outside `src/cli/flash.rs` and `src/cli/mod.rs` are modified (`git status`)
- [ ] `plans/README.md` status row for 004 updated

## STOP conditions

- `cargo run -- flash --help` output changes compared to `git stash` version
- Any test that was passing before the split fails
- Cyclic module dependencies introduced (verify with `cargo build`)
- The `ScatterConfig` struct needs to be visible to both `mod.rs` and
  `scatter.rs` — if `pub(super)` doesn't work, use `pub(crate)` instead

## Maintenance notes

- Future flash subcommands (e.g. `flash factory`, `flash super`) get new
  files in `src/cli/flash/` rather than growing `mod.rs`.
- The `run()` dispatch function in `mod.rs` is the natural place for new
  subcommand routing.
- Reviewers should verify that no `pub` visibility was upgraded beyond
  `pub(crate)` unless actually needed by external code.
