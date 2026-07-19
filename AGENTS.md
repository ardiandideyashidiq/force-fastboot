# pawflash — AGENTS.md

## Commands

```sh
cargo build -p pawflash-core            # core lib only
cargo build -p pawflash                 # CLI (debug)
cargo build --release -p pawflash       # matches CI
cargo build -p pawflash-gui             # Tauri (Rust side)
cargo test --workspace                  # all tests
cargo test -p pawflash-core <name>      # single test
cargo clippy --all-targets --all-features --locked -- -D warnings

pnpm lint                                # eslint
pnpm lint:tsc                            # tsc --noEmit
pnpm build                               # tsc && vite build (before tauri build)
pnpm tauri dev                           # Tauri dev server
```

**Order:** `pnpm build` before `cargo build -p pawflash-gui`.

## Project structure

```
pawflash/
├── Cargo.toml                        → workspace: core, pawflash, src-tauri
├── crates/pawflash-core/             → domain: flash/, force_fastboot/,
│                                        scatter_parser/, format/, output/
├── src-tauri/                        → Tauri v2 backend (lib.rs has commands, ProgressEvent)
├── src/                              → React 19 + Tailwind v4 frontend
│   ├── components/{console,layout,tabs,ui}/
│   ├── types/                        → api.ts, progress.ts
│   └── index.css                     → Tailwind v4 @theme tokens, copper palette
└── vendor/
    ├── fastboot-rs/                  → fork of boardswarm/fastboot-rs (+split.rs, +commands)
    └── format-tools/                 → prebuilt mke2fs + make_f2fs (Linux & Windows)
```

## Critical Tauri wiring

Commands accepting `on_event: Channel<ProgressEvent>` **must** get a frontend-created channel:

```ts
const channel = new Channel<ProgressEvent>();
channel.onmessage = (event) => addProgressEvent(event);
await invoke("force_fastboot", { onEvent: channel });
```

Affected commands: `force_fastboot`, `disable_vbmeta`, `format_data`, `execute_plan`, `flash_raw_image`. Omitting `on_event` causes silent runtime errors.

`ProgressEvent` uses `#[serde(tag = "event", content = "data")]` — TS discriminated union mirrored in `src/types/progress.ts`.

## Framework quirks

- **Tailwind v4** — tokens in `@theme` / `@theme inline` in index.css, **no** `tailwind.config.ts`.
- **shadcn/ui base-nova** — uses `@base-ui/react` (NOT Radix).
- **React 19 eslint strictness:**
  - `useRef(Date.now())` flagged — use `useState(() => Date.now())`.
  - Reading `ref.current` during render forbidden — do it inside callbacks only.
  - `react-refresh/only-export-components` — named hook exports in separate files.

## Rust conventions

- Zero `#[allow]`/`#[expect]` — fix the issue.
- Max ~400 lines per file; split into directory submodules.
- `tracing` with fields always: `info!(field = value, "msg")` — never format! in log calls.
- CLI prints via `output::status::*` helpers (`data()`, `ok()`/`warn()`/`fail()`, etc.) — never raw println/eprintln.
- Edition 2024, MSRV 1.85. Release: LTO (thin), panic=abort.
- Clippy: `all`+`pedantic`+`perf` = warn, `cast_lossless`/`missing_const_for_fn`/etc. = deny (see `Cargo.toml`).
- All tests in-module (`#[cfg(test)]`), no `tests/` integration dir.

## CI & release

Two 2-phase workflows (matrix build → single release):

| Workflow | Trigger | Build targets | Release tag |
|----------|---------|---------------|-------------|
| `release.yml` | push to main | `pawflash` linux + windows | `release-YYYYMMDD-HHMMSS` |
| `release-gui.yml` | changes to `src/`, `src-tauri/`, `package.json`, `pnpm-lock.yaml` | Tauri bundles linux + windows | `gui-release-YYYYMMDD-HHMMSS` |

Shared setup: `.github/actions/setup/`. Linux build deps: `libudev-dev` (CLI), `libwebkit2gtk-4.1-dev` + `patchelf` (GUI).

## Vendored dep notes

- `fastboot-rs` fork adds: `Flashing(s)`, `SetActive(s)`, `ResizeLogicalPartition`, `SnapshotUpdate`, `split.rs` (sparse image chunking). Bugfix: `Verify` formats as `"verify:"` not `"verity:"`.
- `format-tools/` — prebuilt mke2fs/make_f2fs per platform, embedded via `include_bytes!`.
- No generated code, no migrations, no codegen.
