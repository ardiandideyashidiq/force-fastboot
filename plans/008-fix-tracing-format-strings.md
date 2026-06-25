# Plan 008: Fix `tracing` format strings to use structured fields

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/`
> If any source file changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

The AGENTS.md mandates: "Always use `tracing` with fields (`info!(field = value, "msg")`), never format strings in log calls. Pass values as fields, not in the message string." ~10 tracing calls violate this by embedding variable data in the message string (e.g., `info!("BCB cleared on {part}")` instead of `info!(partition = *part, "BCB cleared")`). This prevents structured log analysis systems (Splunk, Loki, JSON log shipping) from extracting structured fields.

## Current state

Violations found across the codebase (run `grep -rn 'tracing::\(info\|warn\|error\|debug\|trace\)!' src/ | grep -E '".*\{'` to find them):

1. `src/flash/format.rs:181`:
   ```rust
   info!(partition = *part, response = %resp, "BCB cleared on {part}")
   ```
   Fix: remove `{part}` from message string — partition is already a named field.

2. `src/flash/executor.rs:214`:
   ```rust
   info!(%partition, "Writing '{partition}' ...")
   ```
   Fix: `info!(%partition, "Writing partition ...")` — partition is already a field.

3. `src/flash/format.rs:243` (and similar nearby):
   ```rust
   info!(%partition, %partition_type, "defaulting to f2fs (reported as {partition_type})")
   ```
   Fix: move the reported type into a field or remove it from the message.

4. `src/flash/format.rs:248` (and similar nearby):
   ```rust
   info!(%partition, %partition_type, "defaulting to ext4 (reported as {partition_type})")
   ```
   Same fix.

5. `src/flash/format.rs:307`:
   ```rust
   info!(%partition, part_size, footer_size, "flashing empty filesystem via sparse wrap")
   ```
   This one is OK — message has no `{variable}` interpolation.

6. `src/flash/format.rs:133`:
   ```rust
   info!(wiped, erased_only, skipped, failed, "format-data complete")
   ```
   OK — fields only.

7. `src/output/status.rs` — multiple calls use `tracing::info!("{}", strip(out))` and `tracing::error!("{plain}")`. These are formatting the output for the tracing logger, which is acceptable since the structured data goes through the `output::status` helper interface rather than direct tracing calls.

The correct pattern (from AGENTS.md):
```rust
info!(%partition, "Writing partition");  // OK: partition is a field
info!("Writing '{partition}' ...");      // BAD: partition is in the message string
```

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/flash/format.rs` — fix the 4+ violations in tracing calls
- `src/flash/executor.rs` — fix the 1+ violations
- Any other file where the grep finds `"\{variable}"` in tracing macro message strings

**Out of scope** (do NOT touch):
- `src/output/status.rs` — the `tracing::info!("{}", strip(out))` pattern is by design (the structured output goes through the status helper, not the tracing message)
- The content of any log message — only the location of variables
- Any production logic changes

## Git workflow

- Branch: `advisor/008-fix-tracing-format-strings`
- Commit message: `fix: move interpolated variables to tracing fields in format.rs and executor.rs`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Find all violations

Run:
```
grep -rn 'tracing::\(info\|warn\|error\|debug\|trace\)!' src/ \
  | grep -E '".*\{[a-zA-Z_][a-zA-Z0-9_]*\}' \
  || echo "No violations found"
```

This finds all tracing macro calls where the format string contains `{variable_name}`. Document each hit.

### Step 2: Fix violations in `src/flash/format.rs`

For each violation found:

Before: `info!(%partition, "Writing '{partition}' ...")` (violation)
After: `info!(%partition, "Writing partition")` (the partition field already carries the value)

Before: `info!(%partition, %partition_type, "defaulting to f2fs (reported as {partition_type})")`
After: `info!(%partition, reported = %partition_type, "defaulting to f2fs")`

Before: `info!(partition = *part, response = %resp, "BCB cleared on {part}")`
After: `info!(partition = *part, response = %resp, "BCB cleared")`

The rule: if a value is already a named field (like `%partition` or `partition = *part`), remove it from the message string. If a value is only in the message string (like `{partition}` without a corresponding field), add it as a field.

**Verify**: `cargo build` exits 0.

### Step 3: Fix violations in `src/flash/executor.rs`

Same pattern. Each violation:

Before: `info!(%partition, "Writing '{partition}' ...")`
After: `info!(%partition, "Writing partition")`

**Verify**: `cargo build` exits 0.

### Step 4: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

### Step 5: Confirm no violations remain

Re-run the grep from step 1:

**Verify**: `grep -rn 'tracing::\(info\|warn\|error\|debug\|trace\)!' src/ | grep -E '".*\{[a-zA-Z_][a-zA-Z0-9_]*\}'` returns no output.

## Test plan

No new tests needed — this is a cosmetic change to logging macros. `cargo test` confirms no regressions.

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] No tracing call embeds `{variable}` in the message string when the variable is also passed as a field
- [ ] The grep from step 1 returns zero output
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the locations found by the grep in step 1 doesn't match the excerpts above.
- `cargo build` fails after the changes.
- `cargo test` reports any test failure.
- A tracing call uses `{variable}` in a way that IS intentional (e.g., the variable is NOT a named field and the message needs it for context) — in that case, add a named field instead.

## Maintenance notes

- After this fix, all tracing calls with variables use structured fields. Future contributors should follow the same pattern.
- AGENTS.md already documents this convention — this plan brings the code into compliance with it.
- This plan does NOT address the `output::status` module's tracing calls, which pass formatted strings through `tracing::info!("{}", strip(out))`. Those are intentional — `output::status` is the presentation layer, and the formatted string is the user-facing output that also echoes to tracing.
