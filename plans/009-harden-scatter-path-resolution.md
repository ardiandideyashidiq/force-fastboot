# Plan 009: Harden scatter path resolution: block outside-package_root paths

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- src/scatter_parser/path.rs src/scatter_parser/plan/`
> If these files changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

When processing a scatter file from an untrusted source, the image path
resolution (`resolve_image_path` in `src/scatter_parser/path.rs`) currently
allows paths that resolve outside `package_root` — it records them as
warnings but doesn't block them. A malicious scatter file with crafted
`file_name` entries could reference arbitrary files on the host filesystem
(e.g., `/etc/shadow`, a private SSH key). If the user runs `pawflash flash`,
the tool would attempt to flash these files to device partitions, exfiltrating
them over USB. The `outside_package_root` field exists but is advisory only.

## Current state

In `src/scatter_parser/path.rs`, the `check_existing_candidates` function
(lines 163–192) notes `outside_package_root` but continues to use the path:

```rust
// path.rs:169-188
for &(via, ref candidate) in candidates {
    let candidate = absolutize(candidate);
    let outside = package_root.as_ref().map(|root| !is_within(&candidate, root));
    if outside == Some(true) {
        *warning = Some(format!("resolved image path is outside package_root: ..."));
        continue;  // ← skips this candidate but falls through to next
    }
    if candidate.exists() {
        return Some(resolved_path_result(ResolvedPathParts { ... }));
    }
}
```

The `continue` skips this *candidate path* but the function will return the
first *next* candidate (line 88) or the `first_allowed` fallback (line 194)
which also uses a similar pattern — `outside != Some(true)` means "allow if
not definitively outside." If `package_root` is `None` (default: scatter
parent dir), no blocking occurs at all.

Also, `resolve_images_for_plan` in `plan/mod.rs` (lines 519–548) calls
`resolve_image_path` and stores the result, but the plan builder never checks
`status.checked` or blocks on `outside_package_root` — it just records
warnings.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all pass            |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |

## Scope

**In scope**:
- `src/scatter_parser/path.rs` — add blocking logic for outside-package paths
- `src/scatter_parser/plan/mod.rs` (or submodules after Plan 005) — validate
  resolved paths against `package_root` and add plan errors instead of warnings
- `src/scatter_parser/types.rs` — may need a `ResolvedPath` field change

**Out of scope**:
- `src/cli/flash.rs` — no CLI changes; the behavior change is silent (what
  was a warning becomes a plan error)
- `src/flash/executor.rs` — no change; the blocker is at the plan level
- Any test file — existing tests should pass; add new tests for the blocking

## Git workflow

- Branch: `advisor/009-harden-path-resolution`
- Commit per step; message style: conventional commits
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add a blocking flag to `resolve_image_path`

Add a parameter `block_outside_package: bool` to `resolve_image_path` (path.rs:12).
When `true` and the resolved path is outside `package_root`, return
`ResolvedPath { resolved_path: None, warning: Some("path outside package_root blocked"), ... }`
instead of the outside path.

Update all callers of `resolve_image_path`:
1. `plan/mod.rs:resolve_images_for_plan` (line 519) — pass `true`
2. Any other call site (grep `resolve_image_path` in `src/`)

**Verify**: `cargo build` exits 0.

### Step 2: Convert outside-package warnings to plan errors

In `plan/mod.rs` (or submodules), in `build_flash_plan` or the image
resolution path, check the resolved path's `outside_package_root` field.
If `true`, add an error to `plan.errors` instead of (or in addition to) a
warning. This makes the flash plan execution fail early with a clear message.

Specifically, in `resolve_images_for_plan` (plan/mod.rs:519), after calling
`resolve_image_path`, check:

```rust
if resolved.outside_package_root == Some(true) && options.mode != Mode::DryRun {
    warnings.push(format!(
        "image path outside package_root: {}",
        resolved.warning.as_deref().unwrap_or("unknown")
    ));
}
```

And in `checked_image_status` (plan/mod.rs:550), when `exists` is `None` or
`false` due to blocking, the subsequent plan validation in `compute_image_counts`
will count it as missing — which `build_flash_plan` already errors on when
`check_images` is true (lines 778-779).

Thus the minimal change is: ensure that blocked paths result in
`resolved_path: None` so they're treated as missing images, and the existing
error machinery in `build_flash_plan` handles them.

**Verify**: `cargo build` exits 0.

### Step 3: Add the `block_outside_package` default for `package_root = None`

When `package_root` is `None`, `resolve_image_path` currently has no reference
point to check against — it sets `outside_package_root: None`. In this case,
use the scatter file's parent directory as the effective package root. This is
already the default behavior in `cli/flash.rs` and `cli/interactive.rs` where
`package_root` is set to `scatter_path.parent()`.

The code in `path.rs:resolve_image_path` should compute an effective root:
```rust
let effective_root = package_root.or_else(|| scatter_dir);
```

Then use `effective_root` for the `is_within` check. If both are `None`, the
path is treated as allowed (no reference point to check against — this is the
same as current behavior when run from an interactive context with no scatter
directory).

**Verify**: `cargo build` exits 0.

### Step 4: Update tests

Add test cases to `src/scatter_parser/path.rs` (or wherever path tests live):
1. Outside-package path is blocked when `block_outside_package = true`
2. Outside-package path is warned (not blocked) when `block_outside_package = false`
3. Normal path inside package_root is unaffected
4. No `package_root` with `scatter_dir` uses scatter_dir as effective root
5. No root at all (both None) — path is allowed

Follow the existing test pattern in the file (no test module currently exists
in `path.rs` — add `#[cfg(test)] mod tests { ... }`).

**Verify**: `cargo test scatter_parser::path::tests` passes.

## Test plan

- 5 new test cases in `src/scatter_parser/path.rs` (see Step 4)
- Existing plan builder tests (6 tests) should still pass — they use
  `package_root = None` which falls back to scatter dir
- Run `cargo test` — all pass

## Done criteria

- [ ] `cargo build` exits 0
- [ ] `cargo test` exits 0; at least 5 new path resolution tests exist and pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] When `block_outside_package = true`, a path outside `package_root` returns
      `resolved_path: None` (not the path)
- [ ] Existing flash plan behavior is unchanged when paths are inside package_root
- [ ] `grep -r "block_outside_package" src/scatter_parser/path.rs` — parameter exists
- [ ] No files outside the in-scope list are modified (`git status`)
- [ ] `plans/README.md` status row for 009 updated

## STOP conditions

- If `package_root` is `None` and no `scatter_dir` is available (e.g., called
  from a context that doesn't have a scatter file), don't block — return the
  path as-is with `outside_package_root: None`.
- If blocking breaks existing valid flash workflows (e.g. firmware images
  stored in `/usr/share/` referenced by relative path), the tool should error
  with a clear message suggesting `--firmware-dir` as an alternative.
- `image_search` (recursive basename search) may legitimately find files
  outside `package_root`. The existing `outside_package_root` warning for
  image_search results (path.rs:58-65) should remain as a warning, not become
  a blocker — image_search is an opt-in convenience feature.

## Maintenance notes

- The blocking behavior is transparent to users who only flash their own
  scatter files — everything inside the firmware directory works as before.
- Users who intentionally reference images outside the package tree will see
  a plan error with the filename and the expected location; they can use
  `--firmware-dir` to add an allowed search path.
- If in the future the tool supports downloading firmware from a remote source,
  this same `package_root` check should apply to the download destination.
