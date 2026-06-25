# Plan 012: Add `--json` output for flash results

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/flash/ src/cli/`
> If these files changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

The scatter plan preview already supports `--json` output (`cli/flash.rs:284-314`), but the actual flash execution results can only be rendered as human-readable terminal output via `output::tables::flash_result()`. CI systems (GitHub Actions, GitLab CI) that automate device flashing cannot programmatically consume the results — they'd need to scrape colored terminal output. Adding `Serializable` derives to the result types and a `--json` flag enables CI integration.

## Current state

`src/flash/results.rs:7-24` — result types:
```rust
pub struct FlashOutcome {
    pub partition: String,
    pub success: bool,
    pub response: Option<String>,
    pub duration: Duration,  // Duration doesn't impl Serialize
    pub error: Option<FlashError>,  // FlashError has no Serialize
}

pub struct FlashResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub outcomes: Vec<FlashOutcome>,
}

pub struct FormatOutcome {
    pub partition: String,
    pub status: FormatStatus,
}

pub enum FormatStatus {
    Wiped,
    ErasedOnly(String),
    Skipped(String),
    Failed(FlashError),
}

pub struct FormatDataResult {
    pub outcomes: Vec<FormatOutcome>,
}
```

None of these derive `Serialize`.

`src/cli/flash.rs:223-232` — the execution path currently just prints human-readable output:
```rust
output::status::stderr(output::tables::flash_result(&result));
```

The plan preview path at `cli/flash.rs:284-314` already supports `--json`:
```rust
fn print_plan(plan: &sp::FlashPlan, json: bool) -> Result<()> {
    if json {
        let output = serde_json::to_string_pretty(plan)?;
        output::status::data(&output);
    } else {
        // human-readable table output
    }
}
```

The `Duration` type does not implement `Serialize`. For JSON output, serialize it as seconds (f64).

`FlashError` derives `Error` and `Display` but not `Serialize`. For JSON output, serialize the error message as a string.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/flash/results.rs` — add `Serialize` to result types, custom serialization for `Duration` and `FlashError`
- `src/flash/error.rs` — add `Serialize` to `FlashError` (serializes as its display string)
- `src/cli/flash.rs` — add `--json` flag to scatter-flash execution path, conditionally output JSON
- `src/cli/format_data.rs` — optionally add `--json` for format-data results

**Out of scope** (do NOT touch):
- The human-readable display code in `output/tables.rs` — keep it as default output
- Any other CLI subcommand (device, force-fastboot)
- Changes to the scatter plan builder

## Git workflow

- Branch: `advisor/012-add-json-output-for-flash-results`
- Commit message: `feat: add Serialize to flash result types and --json output for flash execution`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add `Serialize` to `FlashError` in `src/flash/error.rs`

Add `use serde::Serialize;` to the imports. Add `Serialize` to the derive macro:

```rust
#[derive(Error, Debug, Serialize)]
pub enum FlashError {
    // ...
}
```

For serialization, we want `FlashError` to serialize as its display string. The simplest approach: add `#[serde(untagged)]` or better, implement custom serialization.

Actually, the simplest way: add `#[serde(untagged)]` and each variant serializes as its display string. Or better, derive `Serialize` directly (it serializes the enum variant name + fields, which is fine for JSON output).

**Verify**: `cargo build` exits 0.

### Step 2: Add `Serialize` to result types in `src/flash/results.rs`

Add `use serde::Serialize;`. Then add `Serialize` to the derive macros.

For `FlashOutcome`:
```rust
#[derive(Debug, Serialize)]
pub struct FlashOutcome {
    pub partition: String,
    pub success: bool,
    pub response: Option<String>,
    #[serde(serialize_with = "serialize_duration")]
    pub duration: Duration,
    pub error: Option<FlashError>,
}
```

Add a helper function for serializing `Duration`:
```rust
fn serialize_duration<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64())
}
```

For `FlashResult`:
```rust
#[derive(Debug, Serialize)]
pub struct FlashResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub outcomes: Vec<FlashOutcome>,
}
```

For `FormatOutcome`:
```rust
#[derive(Debug, Serialize)]
pub struct FormatOutcome {
    pub partition: String,
    pub status: FormatStatus,
}
```

For `FormatStatus`:
```rust
#[derive(Debug, Serialize)]
pub enum FormatStatus {
    Wiped,
    ErasedOnly(String),
    Skipped(String),
    Failed(FlashError),
}
```

For `FormatDataResult`:
```rust
#[derive(Debug, Serialize)]
pub struct FormatDataResult {
    pub outcomes: Vec<FormatOutcome>,
}
```

**Verify**: `cargo build` exits 0.

### Step 3: Add `--json` flag to the scatter flash execution path

In `src/cli/args.rs`, the `FlashAction::Scatter` variant already has a `json` field for plan preview (line 76: `json: bool`). However, this is used for plan preview. For execution results, we need a separate flag or reuse it.

The simplest approach: reuse the existing `json` flag. If `--dry-run` is specified, `json` controls plan preview output. If `--dry-run` is not specified, `json` controls execution result output.

In `src/cli/flash.rs`, after the plan execution at line 223-232, replace:

```rust
output::status::stderr(output::tables::flash_result(&result));
```

with:

```rust
if cfg.json {
    let json_output = serde_json::to_string_pretty(&result)?;
    output::status::data(&json_output);
} else {
    output::status::stderr(output::tables::flash_result(&result));
}
```

**Verify**: `cargo build` exits 0.

### Step 4: Add `--json` to `format_data` CLI (bonus, matching the pattern)

In `src/cli/format_data.rs`, at line 44, replace:

```rust
let failed = output::format_display::print_format_results(&result);
```

with conditional JSON output. However, the `format_data` CLI doesn't have a `--json` flag yet. For simplicity, defer this — focus on the scatter flash results.

**Verify**: `cargo build` exits 0.

### Step 5: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

No new tests needed — the change adds serialization derives and a conditional output path. The existing tests confirm no regressions.

Manual verification:
```
pawflash flash scatter <file> --dry-run          # human-readable plan (unchanged)
pawflash flash scatter <file> --dry-run --json    # JSON plan (unchanged)
pawflash flash scatter <file>                     # human-readable result (unchanged)
pawflash flash scatter <file> --json              # JSON result (new)
```

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `FlashError` derives `Serialize`
- [ ] `FlashResult`, `FlashOutcome`, `FormatDataResult`, `FormatOutcome`, `FormatStatus` all derive `Serialize`
- [ ] `Duration` in `FlashOutcome` serializes as f64 seconds
- [ ] `FlashError` in result types serializes as a string
- [ ] `pawflash flash scatter <file> --json` outputs JSON-formatted results
- [ ] `pawflash flash scatter <file>` (no `--json`) still outputs human-readable results
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` fails after the changes (especially the `#[serde(serialize_with = ...)]` custom serialization — if the syntax is wrong, simplify by converting `Duration` to `f64` before serialization).
- `cargo test` reports any test failure.
- Adding `Serialize` to `FlashError` conflicts with its `Error` derive or creates ambiguity.

## Maintenance notes

- The `--json` flag is shared between plan preview and execution results: `--dry-run --json` = JSON plan, `--json` (no dry-run) = JSON results. This is intuitive but should be documented in README.
- Future work: add `--json` to `format-data` results, `device info`, and other subcommands for complete CI integration.
- The `Duration` → `f64` seconds serialization is lossy (nanosecond precision). For a flash tool, sub-second precision is not meaningful.
