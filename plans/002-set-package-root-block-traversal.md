# Plan 002: Set `package_root` and block path traversal in scatter image resolution

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 12bf062..HEAD -- src/cli/flash.rs src/cli/interactive.rs src/scatter_parser/path.rs`
> If these files changed since the planned-at commit, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: MEDIUM (may break users whose firmware images live outside the scatter directory; they would need `--firmware-dir`)
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `12bf062`, 2026-06-25

## Why this matters

A malicious scatter file can reference any file on the host filesystem via `..` traversal in its `file_name` fields (e.g. `file_name: "../../../etc/shadow"`). The image resolution code tracks `contains_parent_reference` but never acts on it. The `package_root` field in `FlashPlanOptions` is always set to `None` in the CLI handlers. This means a scatter file downloaded from a forum could exfiltrate sensitive files (SSH keys, `/etc/shadow`) to the device, or flash host system files as partition images, corrupting the device.

## Current state

Three relevant locations:

1. **`src/cli/flash.rs:160-173`** — `FlashPlanOptions` is constructed with `package_root: None`:

```rust
let options = sp::FlashPlanOptions {
    mode: cfg.mode,
    // ...
    package_root: None,       // <-- must be set to scatter dir
    // ...
};
```

2. **`src/cli/interactive.rs:82-93`** — Same pattern in the interactive flash flow:

```rust
let plan = sp::build_flash_plan(
    &parsed,
    sp::FlashPlanOptions {
        mode: sp::Mode::DirtyFlash,
        // ...
        ..Default::default()  // package_root defaults to None
    },
);
```

3. **`src/scatter_parser/path.rs:66-77`** — The `package_root` check only activates when `package_root` is `Some(...)`. With `None`, `outside_package_root` is also `None`:

```rust
let outside =
    package_root.as_ref().map(|root| !is_within(&candidate, root));
if outside == Some(true) {
    warning = Some(format!(
        "resolved image path is outside package_root: {}",
        candidate.display()
    ));
    continue;  // skip this candidate
}
```

When `package_root` is `None`, `outside` is `None`, `outside == Some(true)` is `false`, and all path candidates are accepted regardless of `..` traversal.

The `contains_parent_reference` field is computed at `path.rs:29-31` and stored in `ResolvedPath` but never checked to block resolution.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build | `cargo build` | exit 0 |
| Test | `cargo test` | all pass |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | no warnings |

## Scope

**In scope** (the only files you should modify):
- `src/cli/flash.rs` — change `package_root` from `None` to `scatter_path.parent()`
- `src/cli/interactive.rs` — same change in the interactive flow
- Optionally: `src/scatter_parser/path.rs` — add a hard error when `contains_parent_reference` is true and no `package_root` sandbox can be verified (defense in depth)

**Out of scope** (do NOT touch):
- `src/scatter_parser/plan/` submodules — the plan builder is fine; it just passes through options
- Any changes to the `ResolvedPath` struct or existing behavior for non-traversal paths
- Adding a `--allow-outside-package-root` CLI flag (deferred — can be added if users report breakage)

## Git workflow

- Branch: `advisor/002-set-package-root-block-traversal`
- Commit message: `fix: set package_root to scatter dir to block path traversal in image resolution`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Set `package_root` in `src/cli/flash.rs`

Change the `options` block at lines 160-173:

Before:
```rust
let options = sp::FlashPlanOptions {
    mode: cfg.mode,
    storage: cfg.storage,
    parts: cfg.parts.to_vec(),
    groups: cfg.groups.to_vec(),
    exclude: cfg.exclude.to_vec(),
    firmware_dir: cfg.firmware_dir.map(Path::to_path_buf),
    package_root: None,
    check_images: cfg.check_images,
    // ...
};
```

After:
```rust
let options = sp::FlashPlanOptions {
    mode: cfg.mode,
    storage: cfg.storage,
    parts: cfg.parts.to_vec(),
    groups: cfg.groups.to_vec(),
    exclude: cfg.exclude.to_vec(),
    firmware_dir: cfg.firmware_dir.map(Path::to_path_buf),
    package_root: Some(scatter_path.parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()),
    check_images: cfg.check_images,
    // ...
};
```

**Verify**: `cargo build` exits 0.

### Step 2: Set `package_root` in `src/cli/interactive.rs`

Find the `build_flash_plan` call at line 82-93. Add `package_root: Some(...)` to the options.

The `scatter_path` is available as the function parameter. Add:
```rust
package_root: Some(scatter_path.parent()
    .unwrap_or_else(|| std::path::Path::new("."))
    .to_path_buf()),
```

Inside the `FlashPlanOptions { ... }` block.

**Verify**: `cargo build` exits 0.

### Step 3: Add defense-in-depth check in `path.rs` (optional but recommended)

In `src/scatter_parser/path.rs`, after computing `contains_parent_reference` (line 29-31), add a block that upgrades the warning to a hard rejection when no `package_root` is provided:

At approximately line 65-68, after the candidates loop but before checking existence, add:

```rust
// Block parent-reference paths when there's no package_root sandbox.
if contains_parent && package_root.is_none() {
    return resolved_path_result(ResolvedPathParts {
        original,
        normalized: &normalized,
        resolved_path: None,
        resolved_via: None,
        exists: Some(false),
        is_absolute_input: absolute_input,
        input_style,
        contains_parent_reference: contains_parent,
        outside_package_root: None,
        warning: Some("path contains parent references (..) but no package_root is set; refusing to resolve".to_string()),
    });
}
```

Place this right before the loop at line 66 that iterates over `candidates`.

Note: `resolve_image_path` does not return `Result`, it returns `ResolvedPath` with an `exists: false` and a `warning`. The plan builder at `scatter_parser/plan/mod.rs:135-144` counts `exists == false` as missing images. So this defense-in-depth will cause the image to be reported as missing rather than silently resolved to a traversal target.

**Verify**: `cargo build` exits 0.

### Step 4: Run tests and clippy

**Verify**: `cargo test` exits 0, all pass.
**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings.

## Test plan

No new tests required — the change is a configuration fix (setting a previously-`None` field). The existing tests continue to pass.

The executor should verify that `cargo test` passes without regressions.

A manual verification: create a scatter file with `file_name: "../../../etc/passwd"` and confirm that `pawflash scatter plan test.txt --dry-run` reports the image as missing rather than resolving it to `/etc/passwd`.

## Done criteria

ALL must hold:

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0, all tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0, no warnings
- [ ] `package_root` is no longer `None` in either `cli/flash.rs` or `cli/interactive.rs`
- [ ] `plans/README.md` status row updated to DONE

## STOP conditions

Stop and report back (do not improvise) if:

- The code at the specified locations doesn't match the excerpts.
- `cargo build` or `cargo test` fails after the changes.
- You discover that `scatter_path.parent()` can return `None` for valid scatter paths (it returns `Some` for relative paths like `./scatter.txt` and absolute paths like `/home/user/scatter.txt`; only a root path `/` returns `None`).

## Maintenance notes

- If a user's firmware images are stored outside the scatter directory tree, they will get "missing image" errors after this fix. They should use `--firmware-dir <path>` to point to the external directory.
- The `--firmware-dir` option is the intended mechanism for adding external image directories. The `package_root` is a safety boundary, not a feature limitation.
- Future enhancement: add `--allow-outside-package-root` flag for advanced users who deliberately want to reference files outside the scatter directory.
