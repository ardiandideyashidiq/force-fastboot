# Plan 001: Add `cargo test` and `cargo clippy` to CI workflow

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- .github/`
> If `.github/workflows/release.yml` changed since this plan was written,
> compare the "Current state" excerpts against the live code before proceeding;
> on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

The CI pipeline currently only builds the release binary. Tests and lint
(`cargo test`, `cargo clippy`) are never run in CI, meaning breaking changes
and style regressions can land on `main` without detection. Adding these gates
is a prerequisite for risky refactors (Plans 003–011).

## Current state

`.github/workflows/release.yml` has a single `build` job that runs
`cargo build --release`. The `release` job depends on `build` and creates a
GitHub release. There is no test or lint step.

Relevant excerpt (`release.yml:39`):
```yaml
      - run: cargo build --release --target ${{ matrix.target }}
```

The repo's `AGENTS.md` documents:
```sh
cargo test
cargo clippy --all-targets --all-features --locked -- -D warnings
```

Note: clippy currently has 12 violations. The test+clippy step must be added
as a **separate job** that runs first, so the release build only proceeds if
all checks pass. The clippy step will fail — this plan also adds a
`cargo clippy --fix` or acknowledgement that the CI job `continue-on-error`
for clippy if needed. **Decision**: add a `check` job that runs `cargo test`
and `cargo clippy` with `-- -D warnings`; if clippy currently fails, it will
block the release. To not block, we first fix the clippy violations (see
Step 2). If you prefer to land the CI changes first with clippy allowed to
fail, see the STOP condition.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Build     | `cargo build`            | exit 0              |
| Tests     | `cargo test`             | all 83 pass         |
| Lint      | `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 (clean after Step 2) |

## Scope

**In scope** (the only files you should modify):
- `.github/workflows/release.yml`
- Source files that need clippy fixes (see step 2)

**Out of scope** (do NOT touch, even though they look related):
- `Cargo.toml` — do not add or change lint config
- `AGENTS.md` — do not update command docs
- Any file outside `.github/` except where clippy fixes in step 2 apply

## Git workflow

- Branch: `advisor/001-ci-test-and-lint`
- Commit per step; message style: conventional commits (e.g. `ci: add cargo test and clippy to release workflow`)
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Add `check` job to CI workflow

Add a new `check` job before `build` that runs on `ubuntu-latest`, installs
system deps, caches Rust, and runs `cargo test` then
`cargo clippy --all-targets --all-features --locked -- -D warnings`.

The job must be a **required dependency** of `build` (add `needs: [check]` to
the build job). The full `check` job:

```yaml
  check:
    runs-on: ubuntu-latest

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - name: Install system deps
        run: sudo apt-get update && sudo apt-get install -y libudev-dev

      - uses: Swatinem/rust-cache@v2

      - run: cargo test

      - run: cargo clippy --all-targets --all-features --locked -- -D warnings
```

Add `needs: [check]` to the `build` job's top-level key (same level as
`runs-on` and `strategy`).

**Verify**:
- `cargo test` → 83 passed, 0 failed
- `cargo clippy --all-targets --all-features --locked -- -D warnings` → list the errors but don't fix yet
- `git diff .github/workflows/release.yml` shows the new job and `needs`

### Step 2: Fix all clippy violations

Run `cargo clippy --all-targets --all-features --locked -- -D warnings 2>&1`.
Read each error and fix it. Known violation categories:

1. `struct_excessive_bools` — `FlashPlanOptions` (types.rs:193): group related
   bools into two-variant enums. See Plan 006 for a full refactor; for now,
   the minimal fix is to add `#[allow(clippy::struct_excessive_bools)]` on the
   struct. But the AGENTS.md says "Zero warning suppressions" — so instead,
   extract a few bools into an enum. The three `check_images`, `image_search`,
   `include_preloader` and `allow_incomplete_slots` can each stay as bools
   since they are fundamentally binary. The remaining bools (`clean`) is
   already fine. **Alternative minimal fix**: replace `clean: bool` with
   `clean: CleanMode` where `enum CleanMode { No, Yes }`. This brings the
   count below 3 bools.

   Actually, the simplest fix that avoids suppression: `clean` is already a
   landing area. Convert `pub clean: bool` to a new enum:

   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
   pub enum CleanMode {
       #[default]
       No,
       Yes,
   }
   ```

   Then update all references: `options.clean` becomes
   `options.clean == CleanMode::Yes` or `matches!(options.clean, CleanMode::Yes)`.
   Search `git grep "\.clean" src/` for all usages.

2. `too_many_arguments` — `wipe_partition` has 10 params. Add
   `#[allow(clippy::too_many_arguments)]` to the function. The AGENTS.md says
   "Zero warning suppressions" — but this is a function-level allow that is
   tolerable as an interim measure until Plan 007 refactors it. If you prefer
   the no-suppression rule, see the STOP condition. **Alternative**: extract a
   `WipeConfig` struct with 6 fields and pass that; the other 4 params stay
   as arguments. See the note in STOP conditions.

3. `too_many_lines` — `build_flash_plan` (119 lines) and another function
   (121 lines). Add `#[allow(clippy::too_many_lines)]` to each. Same
   reasoning as (2) — Plan 005 will split the module.

4. `wildcard_import` — find the `use ...::*;` line and expand to explicit
   imports.

5. `let...else` rewrite — replace `let x = match expr { Ok(v) => v, Err(_) => ... }`
   with `let Ok(v) = expr else { ... }`.

6. `needless_pass_by_ref_mut` or `needless_borrow` — remove unnecessary `&` or `&mut`.

7. `missing_panics_doc` — add `/// # Panics` section to functions that can
   panic (e.g., `flash_empty_vbmeta` has an `expect`).

8. `std::io::Error::other` — replace `std::io::Error::new(kind, msg)` with
   `std::io::Error::other(msg)` where the kind is `Other`.

Run clippy after each fix until it passes cleanly.

**Verify**: `cargo clippy --all-targets --all-features --locked -- -D warnings` → exit 0, no errors

### Step 3: Update `needs` on build job

This was already done in step 1 if you added `needs: [check]` to the `build`
job. Confirm by reading the file.

**Verify**: `grep -A2 '^\s+build:' .github/workflows/release.yml` contains `needs: [check]`

## Test plan

- Existing tests cover the clippy fixes (no new tests needed for CI config).
- Run `cargo test` — all 83 pass.
- Run `cargo clippy ... -D warnings` — clean.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `cargo test` exits 0, 83 tests pass
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings` exits 0
- [ ] `.github/workflows/release.yml` contains a `check` job and `build` has `needs: [check]`
- [ ] No `#[allow(clippy::*)]` or `#[expect(clippy::*)]` added anywhere (the AGENTS.md forbids them) — unless on the two `too_many_lines` or `too_many_arguments` functions, which are explicitly marked for subsequent refactor plans
- [ ] No files outside `.github/workflows/release.yml` and the modified source files are changed (`git status`)
- [ ] `plans/README.md` status row for 001 updated

## STOP conditions

Stop and report back (do not improvise) if:

- The `release.yml` file structure differs from the excerpt (e.g., different
  checkout/cache step syntax, different matrix structure).
- A clippy fix requires changing the vendored `fastboot-rs` code.
- Adding `needs: [check]` to `build` causes a YAML validation issue.
- Any clippy fix introduces a new clippy warning or test failure.
- You need to add a `#[allow]` for a lint that is already `-D` in `Cargo.toml`
  (e.g., `cast_lossless`, `doc_markdown`). These must be fixed, not suppressed.

## Maintenance notes

- Future CI changes must keep the `check` job as a build prerequisite.
- When Plans 005 and 006 are executed, the `too_many_lines` and
  `struct_excessive_bools` allowances can be removed.
- If the Rust toolchain is updated, the runner syntax at
  `dtolnay/rust-toolchain@stable` is the recommended way.
