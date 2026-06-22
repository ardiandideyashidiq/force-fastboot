# pawflash — AGENTS.md

## Commands

```sh
# Build (debug)
cargo build

# Build (release) — matches CI
cargo build --release

# Run all tests
cargo test

# Lint (aggressive — project may not pass cleanly)
cargo clippy --all-targets --all-features --locked -- -D warnings

# Single test
cargo test <test_name>  # e.g. cargo test parse_int_should_accept_decimal
```

## Project structure

- Binary entry: `src/main.rs` — clap CLI with subcommands
- Library entry: `src/lib.rs` — modules: `cli`, `force_fastboot`, `scatter_parser`, `flash`, `format`
- Vendored deps: `vendor/fastboot-rs/` — fork of `boardswarm/fastboot-rs` with extra commands + edition 2024
- Bundled format tools: `vendor/format-tools/` — prebuilt `mke2fs` + `make_f2fs` binaries (AOSP) for `format-data`, embedded via `include_bytes!`
- All tests are in-module `#[cfg(test)]`; no integration tests under `tests/`
- No generated code, no migrations, no codegen steps

## Code style

- **Modular code required.** Keep files focused and under ~400 lines. If a file grows beyond that, split it into a directory module with submodules — each submodule gets one clear responsibility.
- No `pub(crate)` helper functions living in type-definition files. Extract shared helpers into their own module (e.g. `scatter_parser/util.rs`).
- When splitting, use `sort` in the directory listing above to show submodules in order.
- **Structured logging required.** Always use `tracing` with fields (`info!(field = value, "msg")`), never `println!`/`eprintln!` or format strings in log calls. Pass values as fields, not in the message string.

## Notable config

- **Rust edition 2024**, MSRV 1.85 (pawflash + both vendored crates)
- **Async runtime**: tokio (full features) — all `main.rs` entry points, USB, serial
- **Release profile**: LTO (thin), `panic = "abort"`, `overflow-checks = false`, `debug = 0`
- **Clippy lints**: `all`+`pedantic`+`perf` = warn, several individual = deny (`cast_lossless`, `doc_markdown`, `large_enum_variant`, `missing_const_for_fn`, `needless_pass_by_value`, `redundant_clone`, `cargo_common_metadata`)
- **Linux build dep**: `libudev-dev` (for `nusb` USB enumeration)

### vendored fastboot-rs (fork-specific)

- `FastBootCommand::Flashing(S)` — formats as `"flashing {0}"` (lock, unlock, lock_critical, etc.)
- `FastBootCommand::SetActive(S)` — formats as `"set_active:{0}"` (a, b)
- `NusbFastBoot::flashing(cmd)` / `NusbFastBoot::set_active(slot)` — public methods
- Bugfix: `Verify` variant formats as `"verify:"` not `"verity:"`

## CLI usage

```
pawflash force-fastboot [-v]
pawflash scatter parse <scatter-path> [--full-json]
pawflash scatter plan <scatter-path> [--json] [--mode dry-run|selective|dirty-flash] [--storage auto|all|ufs|emmc] [--part ...] [--group ...] [--firmware-dir ...] [--check-images] [--include-preloader]
pawflash flash <scatter-path> [--mode selective|dirty-flash] [--storage auto|all|ufs|emmc] [--part ...] [--group ...] [--firmware-dir ...] [--check-images] [--include-preloader] [--dry-run] [-v]
pawflash format-data [-v] [--fs-options casefold,projid,compress]
pawflash device info
pawflash device reboot [system|bootloader|fastbootd|recovery]
pawflash device lock|unlock
pawflash device set-active <a|b>
pawflash device get-var <var-name>
```

## CI release

- Push to `main` triggers `.github/workflows/release.yml`
- Builds Linux (`x86_64-unknown-linux-gnu`) and Windows (`x86_64-pc-windows-msvc`)
- Creates a timestamped GitHub release (`release-YYYYMMDD-HHMMSS`) with changelog
- Release binary name: `force-fastboot-linux` / `force-fastboot-windows.exe`
