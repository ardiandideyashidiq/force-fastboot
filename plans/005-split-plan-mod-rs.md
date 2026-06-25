# Plan 005: Split `plan/mod.rs` into submodules

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/scatter_parser/plan/`
> If any file in this directory changed since this plan was written, compare
> the "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: MED
- **Depends on**: none (but note: Plan 003 modifies `safety.rs` which this
  module imports; coordinate merge order if both are executed)
- **Category**: tech-debt
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

`src/scatter_parser/plan/mod.rs` is 989 lines — 2.5× the 400-line convention
in AGENTS.md. It's the largest file in the codebase and everything depends on
it (`FlashPlan`, `build_flash_plan`). The file contains 21 private helper
functions alongside the public `build_flash_plan`. Navigating and maintaining
it is harder than necessary.

## Current state

`src/scatter_parser/plan/mod.rs` contains these distinct groups of functions:

| Lines | Group | Description |
|-------|-------|-------------|
| 22–72 | Layout selection | `selected_partitions`, `selected_layouts`, `selected_layout_names` |
| 76–174 | Mode filtering | `mode_str`, `storage_str`, `select_partition_for_mode`, `mode_allows_partition` |
| 178–254 | Group management | `part_matches_group`, `group_names`, `group_members`, `record_unknown_groups`, `warn_for_missing_selective_requests` |
| 258–449 | Slot handling | `inherited_image_source_for_slot_b`, `inherited_action_reason`, `expand_requested_names`, `synthesize_slot_actions_if_needed`, `synthesize_non_download_slot_actions`, `slot_synthesized_action`, `check_incomplete_slots` |
| 453–620 | Action & image | `flash_action`, `skipped_partition`, `finalize_plan_summary`, `resolve_images_for_plan`, `checked_image_status`, `recheck_synthesized_image` |
| 624–828 | Plan builder + verify | `compute_image_counts`, `apply_exclude_filter`, `build_flash_plan` |
| 830–989 | Tests | ~160 lines of inline tests |

The repo convention (AGENTS.md): "Keep files focused and under ~400 lines. If
a file grows beyond that, split it into a directory module with submodules —
each submodule gets one clear responsibility."

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/scatter_parser/plan/mod.rs` — the file to split
- `src/scatter_parser/plan/` — new submodule directory

**Out of scope**:
- `src/scatter_parser/mod.rs` — already re-exports `plan::build_flash_plan`
  and `plan::FlashPlan`, no change needed
- `src/scatter_parser/types.rs` — the `FlashAction`, `FlashPlan`, etc. types
  stay in `types.rs`
- Any other file outside `src/scatter_parser/plan/`

## Git workflow

- Branch: `advisor/005-split-plan-mod`
- Single commit: `refactor: split plan/mod.rs into submodules`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Create submodule files

Create the following files under `src/scatter_parser/plan/`:

1. **`src/scatter_parser/plan/layout.rs`** — layout selection functions:
   `selected_partitions`, `selected_layouts`, `selected_layout_names`

2. **`src/scatter_parser/plan/mode.rs`** — mode/group filtering:
   `mode_str`, `storage_str`, `select_partition_for_mode`,
   `mode_allows_partition`, `part_matches_group`, `group_names`,
   `group_members`, `record_unknown_groups`,
   `warn_for_missing_selective_requests`

3. **`src/scatter_parser/plan/slot.rs`** — slot handling:
   `inherited_image_source_for_slot_b`, `inherited_action_reason`,
   `expand_requested_names`, `synthesize_slot_actions_if_needed`,
   `synthesize_non_download_slot_actions`, `slot_synthesized_action`,
   `check_incomplete_slots`

4. **`src/scatter_parser/plan/image.rs`** — image resolution and status:
   `resolve_images_for_plan`, `checked_image_status`,
   `recheck_synthesized_image`

5. **`src/scatter_parser/plan/action.rs`** — action/skipped builders:
   `flash_action`, `skipped_partition`, `finalize_plan_summary`,
   `compute_image_counts`, `apply_exclude_filter`

Each submodule file should contain only the listed functions. Imports should
use `crate::` paths for external references and `super::` for intra-module
references within the `plan` module tree.

**Convention**: Functions that were private but are now cross-submodule should
be `pub(super)` — not `pub(crate)` or `pub`. Only `build_flash_plan` stays
`pub` in `mod.rs`.

**Verify**: `cargo build` exits 0 after each file is created (or batch-create
all and test once).

### Step 2: Rewrite `mod.rs` as a thin orchestrator

The `mod.rs` should contain:
- Module declarations for each submodule
- The public `build_flash_plan()` function (lines 689–828 of the original)
- Any private helpers used only by `build_flash_plan` that don't fit a
  submodule theme
- The `#[cfg(test)] mod tests { ... }` block (lines 830–989)

The test block stays in `mod.rs` and can access `super::`
imports from submodules since tests are `use super::*;`.

**Verify**:
- `cargo build` exits 0
- `wc -l src/scatter_parser/plan/mod.rs` — under 400 lines (should be ~200
  accounting for the test block)

### Step 3: Run tests and lint

**Verify**:
- `cargo test` — all 83 tests pass (including the plan builder tests)
- `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean
- `wc -l src/scatter_parser/plan/*.rs` — each file under 400 lines

## Test plan

No behavior changes — pure structural refactor. The existing 6 plan builder
tests (lines 895–989) cover the public `build_flash_plan` function. Run
`cargo test` to confirm. The `#[cfg(test)]` block stays in `mod.rs` and
accesses submodule functions via `use super::*`.

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, 83 tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] `wc -l src/scatter_parser/plan/mod.rs` ≤ 400
- [ ] `wc -l src/scatter_parser/plan/layout.rs` ≤ 400
- [ ] `wc -l src/scatter_parser/plan/mode.rs` ≤ 400
- [ ] `wc -l src/scatter_parser/plan/slot.rs` ≤ 400
- [ ] `wc -l src/scatter_parser/plan/image.rs` ≤ 400
- [ ] `wc -l src/scatter_parser/plan/action.rs` ≤ 400
- [ ] All submodule functions use `pub(super)` visibility, not `pub(crate)` or `pub`
- [ ] No files outside `src/scatter_parser/plan/` are modified (`git status`)
- [ ] `plans/README.md` status row for 005 updated

## STOP conditions

- Circular dependencies between submodules (e.g., `slot.rs` imports from
  `action.rs` which imports from `slot.rs`). Resolve by moving the shared
  dependency to `mod.rs` or a shared utility submodule.
- Any `pub(super)` function that needs broader visibility — if a function
  is used outside the `plan` module, it gets `pub(crate)`.
- The `build_flash_plan` function (140 lines) itself approaches the 400-line
  limit — if it exceeds it after the split, stop and report; it may need
  sub-function extraction as a second step.
- Test compilation fails because `#[cfg(test)]` in `mod.rs` can't access
  submodule symbols — make the submodule functions `pub(super)` and use
  `use super::*;` in the test block.

## Maintenance notes

- New flash-planning features go into one of the submodule files or a new
  submodule if they introduce a new concern (e.g., `compress.rs` for
  compressed image support).
- After this split, the `mod.rs` file should be reviewed periodically to
  ensure `build_flash_plan` itself doesn't grow out of hand.
- If Plan 003 (safety helper consolidation) is merged first, verify the
  `mode.rs` submodule's imports still resolve correctly.
