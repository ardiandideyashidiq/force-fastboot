# Plan 002: Update README CLI docs to match actual CLI

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 0411b16..HEAD -- README.md`
> If README.md changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: docs
- **Planned at**: commit `0411b16`, 2026-06-25

## Why this matters

The README shows a CLI that doesn't match the actual binary. New features
(GSI flash, `scatter parse`, `--exclude`, `--image-search`, etc.) are
invisible to users. Users who follow the README will get errors or miss
capabilities.

## Current state

The README's `## Usage` section (lines 16-23) shows:
```
pawflash force-fastboot [-v]
pawflash flash scatter <scatter> [--mode dry-run|selective|dirty-flash] [--storage auto|all|ufs|emmc] [--firmware-dir <dir>] [--check-images] [--dry-run] [--part <name>]... [--group <name>]... [-v]
pawflash flash <partition> <image> [--slot a|b] [--both]
pawflash disable-vbmeta [-v]
pawflash format-data [-v] [--fs-options casefold,projid,compress]
pawflash device info|reboot [target]|lock|unlock|set-active <a|b>|get-var <var>
```

The actual CLI (from `src/cli/args.rs` and `AGENTS.md`) includes:
- `pawflash flash gsi <image> [--clean-test]`
- `pawflash flash scatter` → the old `pawflash flash scatter` syntax is now
  `pawflash flash scatter <path>` with sub-flags `--show`, `--full-json`,
  `--exclude`, `--image-search`, `--allow-incomplete-slots`, `--clean`,
  `--no-format`, `--clean-test`
- The `scatter` subcommand is the default action for `pawflash flash` when
  called with a scatter path (see `main.rs:34-36`).

The "Flash modes" paragraph (line 25) is partially correct but doesn't
mention `selective` requires `--part`/`--group`, or that `dry-run` is a
`--dry-run` flag, not a mode in the same sense.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Build   | `cargo build` | exit 0 |
| Verify CLI | `cargo run -- --help` | prints help |
| Verify subcommand | `cargo run -- flash --help` | prints flash help |

## Scope

**In scope**:
- `README.md` — rewrite the Usage section

**Out of scope**:
- Any source file in `src/`
- `AGENTS.md` — already has the accurate CLI reference
- Any other `.md` file in the repo

## Git workflow

- Branch: `advisor/002-readme-cli-docs`
- Single commit: `docs: sync README CLI examples with actual CLI`
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Generate accurate help text

Run `cargo run -- --help` and `cargo run -- flash --help` to capture the
actual CLI. Take note of:
- The exact subcommand tree (`Commands` enum)
- All flags and their defaults
- `FlashAction::Scatter` and `FlashAction::Gsi` structure

**Verify**: Help text renders without errors.

### Step 2: Rewrite the README Usage section

Replace lines 16-25 with accurate content. Match the structure of the
AGENTS.md CLI section (lines 65-77) as the source of truth. The README should
show:

```
pawflash force-fastboot [-v]
pawflash flash scatter <scatter-path> [--mode dry-run|selective|dirty-flash] [--storage auto|all|ufs|emmc] [--part <name>]... [--group <name>]... [--firmware-dir <dir>] [--check-images] [--dry-run] [--json] [--exclude <name>]... [--image-search] [--allow-incomplete-slots] [--include-preloader] [--clean] [--no-format] [--clean-test] [-v]
pawflash flash gsi <image> [--clean-test]
pawflash flash <partition> <image> [--slot a|b] [--both]
pawflash disable-vbmeta [-v]
pawflash format-data [-v] [--fs-options casefold,projid,compress] [--fs-type ext4|f2fs]
pawflash device info
pawflash device reboot [system|bootloader|fastbootd|recovery]
pawflash device lock|unlock
pawflash device set-active <a|b>
pawflash device get-var <var-name>
```

Also add a second-line paragraph explaining the flash modes:

```
Flash modes: `dry-run` (preview via `--dry-run` flag), `selective` (explicit
`--part`/`--group`, the default), `dirty-flash` (safe firmware + Android).
Storage: `auto` (default, prefers UFS), `all`, `ufs`, `emmc`.
```

**Verify**:
- `cargo build` exits 0 (markdown changes don't affect compilation)
- `grep -c "pawflash" README.md` — all expected commands present

## Test plan

No code changes, so no new tests needed.

## Done criteria

- [ ] `cargo build` exits 0
- [ ] Every subcommand from `cargo run -- --help` has a corresponding line in the README Usage section
- [ ] `grep "flash gsi" README.md` returns a match
- [ ] `grep "flash scatter" README.md` returns a match
- [ ] `grep "device get-var" README.md` returns a match
- [ ] No files outside `README.md` are modified (`git status`)
- [ ] `plans/README.md` status row for 002 updated

## STOP conditions

- The help output from `cargo run -- --help` does not match the AGENTS.md
  reference (means the CLI has drifted further — report the actual output).
- The README has been restructured since this plan was written and lines
  don't match.

## Maintenance notes

- After this PR, every CLI feature addition should include a README update.
  Consider adding a README section to the PR checklist if one exists.
