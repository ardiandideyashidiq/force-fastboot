# Plan 009: Simplify GSI dual-path state machine

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/gsi/flash.rs`
> If this file changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: MEDIUM (restructures the GSI workflow; must preserve both mode paths)
- **Depends on**: Plan 005 (characterization tests for `detect_fastboot_mode` and `product_gsi_overflow_size` must land first)
- **Category**: tech-debt
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

The GSI flash state machine in `src/gsi/flash.rs:232-296` has two arms — one for `Bootloader` entry mode and one for `Fastbootd` entry mode — that perform the same operations in reverse order. Each arm is ~30 lines of sequential steps with duplicated timeline logic. Adding a new step (e.g., OTA snapshot cancellation, separate vbmeta re-enable, cancel token checks) requires editing both arms, risking missing one.

## Current state

`src/gsi/flash.rs:232-296`:

```rust
match mode {
    FastbootMode::Bootloader => {
        // Phase 1: bootloader-only operations
        report(GsiEvent::Step(GsiStep::PreparingVbmetaFlash));
        report(GsiEvent::Step(GsiStep::FlashingVbmeta));
        executor.flash_empty_vbmeta().await?;

        report(GsiEvent::Step(GsiStep::WipingUserdata));
        executor.format_data(0, clean_test, None).await;

        // Transition to fastbootd
        executor = transition_mode(executor, FastbootMode::Fastbootd, &mut report).await?;

        // Phase 2: fastbootd-only operations
        let (system_partition, system_size) = resolve_system_partition(&mut executor).await?;
        // ... flash system + product ...
    }
    FastbootMode::Fastbootd => {
        // Phase 1: fastbootd-only operations (opposite order)
        // ... resolve partitions ...
        // ... flash system + product ...

        // Phase 2: bootloader-only operations (opposite order)
        executor = transition_mode(executor, FastbootMode::Bootloader, &mut report).await?;
        report(GsiEvent::Step(GsiStep::FlashingVbmeta));
        executor.flash_empty_vbmeta().await?;
        executor.format_data(0, clean_test, None).await;
    }
}
```

Both arms share:
- `vbmeta disable` + `userdata wipe` (bootloader phase)
- `partition resolution` + `system/product flash` (fastbootd phase)

They differ only in the ORDER of phases and the `transition_mode` directions.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/gsi/flash.rs` — restructure the `execute_gsi_flash` function

**Out of scope** (do NOT touch):
- `src/gsi/types.rs` — the event types and step enums remain unchanged
- `src/flash/` — the executor API signatures remain unchanged
- The `transition_mode` function — it works correctly as-is

## Git workflow

- Branch: `advisor/009-simplify-gsi-dual-path-state-machine`
- Commit message: `refactor: extract GSI phase ordering into a stage list to eliminate duplicated branches`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Define a `Stage` enum for GSI workflow steps

Add inside `execute_gsi_flash` (or in `src/gsi/types.rs` for reuse):

```rust
enum GsiStage {
    FlashVbmeta,
    WipeUserdata,
    FlashSystem,
}
```

### Step 2: Determine stage order based on entry mode

Replace the `match mode { Bootloader => { ... }, Fastbootd => { ... } }` with a stage list:

```rust
// Define which stages to run in which order based on entry mode.
// In bootloader-first mode: vbmeta/wipe first, then transition, then flash.
// In fastbootd-first mode: flash first, then transition, then vbmeta/wipe.
let stages: Vec<(FastbootMode, Vec<GsiStage>)> = match mode {
    FastbootMode::Bootloader => vec![
        (FastbootMode::Bootloader, vec![GsiStage::FlashVbmeta, GsiStage::WipeUserdata]),
        (FastbootMode::Fastbootd, vec![GsiStage::FlashSystem]),
    ],
    FastbootMode::Fastbootd => vec![
        (FastbootMode::Fastbootd, vec![GsiStage::FlashSystem]),
        (FastbootMode::Bootloader, vec![GsiStage::FlashVbmeta, GsiStage::WipeUserdata]),
    ],
};

for (required_mode, stage_group) in stages {
    // Transition if not already in the required mode
    executor = transition_mode(executor, required_mode, &mut report).await?;

    for stage in &stage_group {
        match stage {
            GsiStage::FlashVbmeta => {
                report(GsiEvent::Step(GsiStep::PreparingVbmetaFlash));
                report(GsiEvent::Step(GsiStep::FlashingVbmeta));
                executor.flash_empty_vbmeta().await?;
            }
            GsiStage::WipeUserdata => {
                report(GsiEvent::Step(GsiStep::WipingUserdata));
                executor.format_data(0, clean_test, None).await;
            }
            GsiStage::FlashSystem => {
                let (system_partition, system_size) = resolve_system_partition(&mut executor).await?;
                // ... rest of flash_system_and_product ...
            }
        }
    }
}
```

### Step 3: Extract `flash_system_and_product` integration

The current `flash_system_and_product` helper (lines 331-388) takes all the parameters it needs. It is called exactly once and needs no change. Just move the call into the `GsiStage::FlashSystem` match arm.

**Verify**: `cargo build` exits 0.

### Step 4: Remove the `tools_dir` / `extract_tools()` duplicate

The current code extracts tools only once before the `match` block (line 229). Ensure the restructured code still extracts tools once, before the stage loop.

**Verify**: `cargo build` exits 0.

### Step 5: Run tests and clippy

**Verify**: `cargo test` exits 0, all tests pass (including Plan 005's characterization tests for `detect_fastboot_mode` and `product_gsi_overflow_size`).
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

The characterization tests from Plan 005 cover `detect_fastboot_mode` and `product_gsi_overflow_size`. The restructured code must preserve the same runtime behavior. Since there are no integration tests for the full GSI workflow, the executor should manually trace through the two paths and verify:

1. Bootloader entry → vbmeta → wipe → transition → flash (same as before)
2. Fastbootd entry → flash → transition → vbmeta → wipe (same as before)
3. `transition_mode` is called EXACTLY once between the two phase groups, never more

## Done criteria

ALL must hold:

- [ ] Plan 005 characterization tests are in place and passing
- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] The `match mode { Bootloader => { ... }, Fastbootd => { ... } }` structure is replaced by a single stage iteration
- [ ] Both mode paths produce the same sequence of operations as before
- [ ] No duplicate `transition_mode` calls: the stage list ensures exactly one transition per phase group
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` fails after the changes.
- Any test fails (pre-existing or induced).
- The `GsiStage` enum introduces a logical path that produces a different operation sequence than the original code for either entry mode.
- The `transition_mode` function is called more or fewer times than in the original code (original: Bootloader entry = 1 transition; Fastbootd entry = 1 transition).

## Maintenance notes

- Adding a new step to the GSI workflow now requires adding a variant to `GsiStage` and inserting it into the appropriate phase group in the stage list. Both mode orders are automatically handled.
- If a future mode (e.g., `Recovery`) needs a different ordering, add a new entry to the stage list.
- The `extract_tools()` call at the top must happen before the stage loop — ensure this invariant is preserved.
