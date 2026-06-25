# Plan 003: Consolidate `safety_class()` and `role_for_name()` into shared helper

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/scatter_parser/safety.rs`
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

`src/scatter_parser/safety.rs` has two functions — `safety_class()` (lines 77–136)
and `role_for_name()` (lines 140–162) — that each classify a partition name
against the same canonical constant lists but return different labels. The
first 55 lines of `safety_class()` are structurally identical to the entirety
of `role_for_name()`. They have already started to drift: `safety_class()` has
additional fallback logic (lines 95–134) not present in `role_for_name()`.
Any change to the canonical lists or classification logic must be mirrored
manually.

## Current state

Both functions in `src/scatter_parser/safety.rs` follow the same pattern:
check `IDENTITY_CANONICAL`, then `DANGEROUS_CANONICAL`, then
`BOOTLOADER_CANONICAL`, etc. Each returns a different string label.

```rust
// safety.rs:77 — safety_class starts with this block
pub fn safety_class(name: &str) -> String {
    let canonical = canonical_name(name);
    if IDENTITY_CANONICAL.contains(&canonical.as_str()) {
        "identity_or_calibration"
    // ... same pattern through REGIONAL_CANONICAL ...
    } else { "unknown" }.to_string()
}

// safety.rs:140 — role_for_name starts identically
pub fn role_for_name(name: &str) -> String {
    let canonical = canonical_name(name);
    if IDENTITY_CANONICAL.contains(&canonical.as_str()) {
        "identity_or_calibration"
    // ... same pattern, different labels ...
    } else { "unknown" }.to_string()
}
```

The repo's conventions (AGENTS.md) forbid `pub(crate)` helpers living in
type-definition files; helpers belong in their own module. This code already
lives in `safety.rs` which is fine, but the duplication should be
consolidated into an internal helper.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/scatter_parser/safety.rs`

**Out of scope**:
- `src/scatter_parser/types.rs` — do not touch
- Any test file
- `src/scatter_parser/plan/mod.rs` or any other consumer — the public API
  (`safety_class()` and `role_for_name()`) must keep their exact signatures
  and return values so no callers change.

## Git workflow

- Branch: `advisor/003-consolidate-safety`
- Single commit: `refactor: consolidate safety_class/role_for_name classification`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add an internal classification helper

Add a private helper function to `src/scatter_parser/safety.rs` that maps a
canonical name to a classification key without returning a user-facing label:

```rust
enum SafetyRole {
    IdentityOrCalibration,
    Dangerous,
    BootloaderCritical,
    BootChainOrAvb,
    ModemFirmware,
    McuFirmware,
    AndroidDynamicOrSystem,
    RegionalOrBranding,
    Unknown,
}

fn classify(canonical: &str) -> SafetyRole {
    if IDENTITY_CANONICAL.contains(&canonical) {
        SafetyRole::IdentityOrCalibration
    } else if DANGEROUS_CANONICAL.contains(&canonical) {
        SafetyRole::Dangerous
    } else if BOOTLOADER_CANONICAL.contains(&canonical) {
        SafetyRole::BootloaderCritical
    } else if BOOT_CHAIN_CANONICAL.contains(&canonical) {
        SafetyRole::BootChainOrAvb
    } else if MODEM_CANONICAL.contains(&canonical) {
        SafetyRole::ModemFirmware
    } else if MCU_FW_CANONICAL.contains(&canonical) {
        SafetyRole::McuFirmware
    } else if ANDROID_CANONICAL.contains(&canonical) {
        SafetyRole::AndroidDynamicOrSystem
    } else if REGIONAL_CANONICAL.contains(&canonical) {
        SafetyRole::RegionalOrBranding
    } else {
        SafetyRole::Unknown
    }
}
```

The `role_for_name()` function then becomes:
```rust
pub fn role_for_name(name: &str) -> String {
    let canonical = canonical_name(name);
    match classify(&canonical) {
        SafetyRole::IdentityOrCalibration => "identity_or_calibration",
        SafetyRole::Dangerous => "dangerous",
        SafetyRole::BootloaderCritical => "bootloader_critical",
        SafetyRole::BootChainOrAvb => "boot_chain_or_avb",
        SafetyRole::ModemFirmware => "modem_firmware",
        SafetyRole::McuFirmware => "mcu_firmware",
        SafetyRole::AndroidDynamicOrSystem => "android_dynamic_or_system",
        SafetyRole::RegionalOrBranding => "regional_or_branding",
        SafetyRole::Unknown => "unknown",
    }.to_string()
}
```

**Verify**: `cargo build` exits 0.

### Step 2: Rewrite `safety_class()` to use the helper

The `safety_class()` function currently has the same initial block as
`role_for_name()` (lines 77–93 matching `IDENTITY_CANONICAL` through
`REGIONAL_CANONICAL`) plus extra fallback logic (lines 95–134) for edge cases
like `super`, `system_ext`, `cache`, `metadata`, `vendor_dlkm`, etc. The new
implementation should keep the fallback logic but use the helper for the
first check:

```rust
pub fn safety_class(name: &str) -> String {
    let canonical = canonical_name(name);
    match classify(&canonical) {
        SafetyRole::Unknown => {
            // Fallback: extended heuristic patterns
            if matches!(
                canonical.as_str(),
                "super" | "system_ext" | "vendor_dlkm" | "odm_dlkm"
                    | "my_product" | "my_region" | "product" | "vendor"
                    | "odm" | "cache" | "metadata"
            ) || canonical.starts_with("system")
                || canonical.starts_with("product")
                || canonical.starts_with("vendor")
                || canonical.starts_with("odm")
            {
                "android_system"
            } else if canonical.contains("vbmeta")
                || canonical.contains("boot")
                || canonical.contains("dtbo")
                || canonical.contains("recovery")
                || canonical.contains("init_boot")
            {
                "boot_critical"
            } else if canonical.contains("logo")
                || canonical.contains("splash")
                || canonical.contains("cust")
            {
                "regional"
            } else if canonical.contains("modem")
                || canonical.contains("radio")
                || canonical.contains("dsp")
                || canonical.ends_with("_fw")
            {
                "firmware"
            } else {
                "unknown"
            }
        }
        SafetyRole::BootloaderCritical => "bootloader_critical",
        SafetyRole::BootChainOrAvb => "boot_critical",
        SafetyRole::ModemFirmware | SafetyRole::McuFirmware => "firmware",
        SafetyRole::AndroidDynamicOrSystem => "android_system",
        SafetyRole::RegionalOrBranding => "regional",
        SafetyRole::IdentityOrCalibration => "identity_or_calibration",
        SafetyRole::Dangerous => "dangerous",
    }.to_string()
}
```

Note how `role_for_name` and `safety_class` return *different labels* for the
same classification — that's intentional. The helper just removes the
duplicated constant-list checking.

**Verify**: `cargo build` exits 0.

### Step 3: Run tests

**Verify**: `cargo test` — all tests pass (the safety tests in the same file
cover every return value of both functions).

**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` — clean.

## Test plan

Existing unit tests in `src/scatter_parser/safety.rs` (lines 183–305) cover
every branch of both functions. No new tests needed — the public API surface
(`safety_class()` and `role_for_name()`) is unchanged. Run `cargo test` and
all 83 tests must pass.

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, 83 tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] `safety_class("boot")` and `role_for_name("boot")` return the same
      strings as before the change (verify by reading: `safety_class("boot")`
      → `"boot_critical"`, `role_for_name("boot")` → `"boot_chain_or_avb"`)
- [ ] No files outside `src/scatter_parser/safety.rs` are modified (`git status`)
- [ ] `plans/README.md` status row for 003 updated

## STOP conditions

- The `classify` function introduces ambiguity between `safety_class` and
  `role_for_name` labels (they are intentionally different — verify the test
  expectations match).
- Any test fails after the refactor (stop and report the exact test name and
  error).
- The `safety_class()` fallback logic (lines 95–134) cannot be cleanly mapped
  to the `classify` helper's branches.

## Maintenance notes

- When new partition types need classification, update the canonical constant
  lists in `safety.rs` (the `BOOTLOADER_CANONICAL`, etc. slices) and add a
  new variant to `SafetyRole` if needed. Both `safety_class` and
  `role_for_name` automatically pick up the change via `classify()`.
- The fallback logic in `safety_class()` (the `match SafetyRole::Unknown`
  arm) is the part that will drift from `role_for_name` over time — that's
  intentional, as the two functions serve different labeling needs.
