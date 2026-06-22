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

- Binary entry: `src/main.rs` — clap CLI with subcommands `force-fastboot` and `scatter`
- Library entry: `src/lib.rs` — three modules: `cli`, `force_fastboot`, `scatter_parser`
- All tests are in-module `#[cfg(test)]`; no integration tests under `tests/`
- No generated code, no migrations, no codegen steps

## Code style

- **Modular code required.** Keep files focused and under ~400 lines. If a file grows beyond that, split it into a directory module with submodules — each submodule gets one clear responsibility.
- No `pub(crate)` helper functions living in type-definition files. Extract shared helpers into their own module (e.g. `scatter_parser/util.rs`).
- When splitting, use `sort` in the directory listing above to show submodules in order.
- **Structured logging required.** Always use `tracing` with fields (`info!(field = value, "msg")`), never `println!`/`eprintln!` or format strings in log calls. Pass values as fields, not in the message string.

## Notable config

- **Rust edition 2024**, MSRV 1.85
- **Release profile**: LTO (thin), `panic = "abort"`, `overflow-checks = false`, `debug = 0`
- **Clippy lints**: `all`+`pedantic`+`perf` = warn, several individual = deny (`cast_lossless`, `doc_markdown`, `large_enum_variant`, `missing_const_for_fn`, `needless_pass_by_value`, `redundant_clone`, `cargo_common_metadata`)
- **Linux build dep**: `libudev-dev` (for `nusb` USB enumeration)

## CLI usage

```
pawflash force-fastboot [-v]
pawflash scatter parse <scatter-path> [--full-json]
pawflash scatter plan <scatter-path> [--json] [--mode dry-run|selective|dirty-flash] [--storage auto|all|ufs|emmc] [--part ...] [--group ...] [--firmware-dir ...] [--check-images] [--include-preloader]
```

## CI release

- Push to `main` triggers `.github/workflows/release.yml`
- Builds Linux (`x86_64-unknown-linux-gnu`) and Windows (`x86_64-pc-windows-msvc`)
- Creates a timestamped GitHub release (`release-YYYYMMDD-HHMMSS`) with changelog
- Release binary name: `force-fastboot-linux` / `force-fastboot-windows.exe`
