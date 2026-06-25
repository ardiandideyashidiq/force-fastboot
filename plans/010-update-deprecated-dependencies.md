# Plan 010: Update deprecated/abandoned dependencies

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- Cargo.toml src/`
> If these files changed since the planned-at commit, compare against the "Current state" sections below before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: LOW (dependency swaps are mechanical; the new crates have identical APIs)
- **Depends on**: none
- **Category**: migration
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

Three direct dependencies have upstream maintenance concerns:
1. **`colored`** (v3) — the crate is deprecated in favor of `anstyle`/`owo-colors`. Still functional but pulls in heavier transitive deps and may accumulate CVEs without active maintenance.
2. **`serde_yaml`** (v0.9) — the crate is largely unmaintained; the ecosystem has migrated to `serde_yml` (a maintained fork).
3. **`nusb`** (v0.2) — while functional, may have newer versions with security fixes and platform improvements.

This plan addresses `colored` and `serde_yaml` (low-risk swaps). `nusb` upgrade is deferred due to vendored fastboot-rs dependency.

## Current state

### `colored` → `owo-colors`

Cargo.toml line 34:
```toml
colored = "3"
```

Used in `src/output/theme.rs:1`:
```rust
use colored::Colorize as _;
```

The entire `theme.rs` file (25 lines) uses `colored`:
```rust
pub fn error(msg: impl AsRef<str>) -> String { msg.as_ref().red().bold().to_string() }
pub fn warn(msg: impl AsRef<str>) -> String { msg.as_ref().yellow().to_string() }
pub fn ok(msg: impl AsRef<str>) -> String { msg.as_ref().green().to_string() }
pub fn dim(msg: impl AsRef<str>) -> String { msg.as_ref().dimmed().to_string() }
pub fn heading(msg: impl AsRef<str>) -> String { msg.as_ref().white().bold().to_string() }
pub fn info(msg: impl AsRef<str>) -> String { msg.as_ref().bright_blue().to_string() }
```

`owo-colors` provides an equivalent API via the `OwoColorize` trait with methods like `.red()`, `.bold()`, `.yellow()`, `.green()`, `.dimmed()`, `.white()`, `.bright_blue()`.

### `serde_yaml` → `serde_yml`

Cargo.toml line 43:
```toml
serde_yaml = "0.9"
```

Used in `src/scatter_parser/parse/yaml.rs:6`:
```rust
use serde_yaml;
```

The import would change to `serde_yml`. The crate exposes identical function names (`from_reader`, `from_str`, `Value`).

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `Cargo.toml` — replace `colored` with `owo-colors`, replace `serde_yaml` with `serde_yml`
- `src/output/theme.rs` — change import from `colored::Colorize` to `owo_colors::OwoColorize`
- `src/scatter_parser/parse/yaml.rs` — change import from `serde_yaml` to `serde_yml`

**Out of scope** (do NOT touch):
- `nusb` (v0.2) — coordinated update needed with vendored `fastboot-rs`. Deferred.
- `vendor/fastboot-rs/` — not touched by this plan.
- Any other dependency or file.

## Git workflow

- Branch: `advisor/010-update-deprecated-dependencies`
- Commit message: `chore: replace colored with owo-colors, serde_yaml with serde_yml`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Update `Cargo.toml`

Change line 34:
```toml
colored = "3"
```
to:
```toml
owo-colors = "4"
```

Change line 43:
```toml
serde_yaml = "0.9"
```
to:
```toml
serde_yml = "0.10"
```

**Verify**: `cargo build` — if this fails, adjust version numbers. Check crates.io for the latest stable versions of `owo-colors` and `serde_yml`.

### Step 2: Update `src/output/theme.rs`

Change the import at line 1:
```rust
use colored::Colorize as _;
```
to:
```rust
use owo_colors::OwoColorize;
```

The rest of the file needs no changes — `owo-colors` provides the same `.red()`, `.bold()`, `.yellow()`, `.green()`, `.dimmed()`, `.white()`, `.bright_blue()` methods via the `OwoColorize` trait.

Note: `owo-colors` uses `OwoColorize` trait instead of `colored`'s `Colorize` trait. The method names are identical for basic color operations.

**Verify**: `cargo build` exits 0.

### Step 3: Update `src/scatter_parser/parse/yaml.rs`

Find the `serde_yaml` import. If it's used as a module path (e.g., `serde_yaml::from_str`), change to `serde_yml::from_str`. If it's a `use` statement, update accordingly.

The file at `src/scatter_parser/parse/yaml.rs` typically uses:
```rust
use serde_yaml;
```
This is a module-level import that allows `serde_yaml::from_reader(...)` in the code. Change to:
```rust
use serde_yml;
```

If the code uses `serde_yaml::from_str(...)` or `serde_yaml::from_reader(...)`, change to `serde_yml::from_str(...)` / `serde_yml::from_reader(...)`.

**Verify**: `cargo build` exits 0.

### Step 4: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

### Step 5: Remove `Cargo.lock` drift

If `cargo build` or `cargo test` modifies `Cargo.lock`, commit the lockfile changes.

## Test plan

The existing tests ensure that:
- `theme.rs` output helpers still compile (they're called from `output/tables.rs` and `output/status.rs`)
- YAML scatter parsing still works (tested by `parse_scatter_rejects_non_file` in `parse/mod.rs`)

Run `cargo test` to confirm both.

## Done criteria

ALL must hold:

- [ ] `colored` is removed from `Cargo.toml`
- [ ] `owo-colors` is in `Cargo.toml` dependencies
- [ ] `serde_yaml` is removed from `Cargo.toml`
- [ ] `serde_yml` is in `Cargo.toml` dependencies
- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `grep -rn 'colored' Cargo.toml src/` returns no matches
- [ ] `grep -rn 'serde_yaml' Cargo.toml src/` returns no matches
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- `cargo build` fails after the changes (the latest version of `owo-colors` or `serde_yml` may have different APIs than expected — check crates.io).
- A method used in `theme.rs` is not available in `owo-colors` v4 (check the `.red()`, `.bold()`, `.yellow()`, `.green()`, `.dimmed()`, `.white()`, `.bright_blue()` methods).
- `cargo test` reports any test failure related to YAML parsing (unlikely — `serde_yml` is API-compatible with `serde_yaml`).

## Maintenance notes

- `owo-colors` v4 uses the `OwoColorize` trait. If you get a "trait not in scope" error, add `use owo_colors::OwoColorize;` — the trait must be in scope for the method calls to work.
- `serde_yml` is a drop-in replacement for `serde_yaml`. The API surface is identical.
- `nusb` upgrade is deferred because it requires coordinated changes to the vendored `fastboot-rs/fastboot-protocol/Cargo.toml` where `nusb = "0.2.3"` is pinned.
