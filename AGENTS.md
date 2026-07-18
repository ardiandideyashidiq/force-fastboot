# pawflash — AGENTS.md

## Commands

```sh
# Rust
cargo build -p pawflash-core          # core lib only
cargo build -p pawflash-cli           # CLI binary (debug)
cargo build --release -p pawflash-cli # matches CI
cargo build -p pawflash-tauri         # Tauri app (Rust side)
cargo test --workspace                # all tests
cargo test -p pawflash-core <name>    # single test, e.g. parse_int_should_accept_decimal
cargo clippy --all-targets --all-features --locked -- -D warnings  # aggressive

# Frontend (pnpm, never npm)
pnpm lint                             # eslint
pnpm lint:tsc                         # tsc --noEmit
pnpm build                            # tsc && vite build (runs BEFORE tauri build)
pnpm tauri dev                        # Tauri dev server (starts Vite + Rust)
```

**Build order matters:** `pnpm build` must pass before `cargo build -p pawflash-tauri` succeeds.

## Project structure

```
pawflash/
├── Cargo.toml                  → workspace: pawflash-core, pawflash-cli, src-tauri
├── crates/
│   └── pawflash-core/          → domain logic: flash/, force_fastboot/, scatter_parser/,
│                                  format/, gsi/, output/
├── src-tauri/                  → Tauri v2 Rust backend
│   ├── src/lib.rs              → commands wrapping pawflash-core, ProgressEvent enum
│   └── tauri.conf.json         → devUrl localhost:1420, frontendDist ../dist
├── src/                        → React 19 + Tailwind v4 frontend
│   ├── components/
│   │   ├── console/            → ConsolePanel + ConsoleContext (live progress output)
│   │   ├── layout/             → AppLayout (sidebar nav + console grid)
│   │   ├── tabs/               → MainTab, ToolsTab, SettingsTab (lazy-loaded)
│   │   └── ui/                 → shadcn/base-ui components (button, select, dialog, etc.)
│   ├── hooks/                  → useConsole, useTauriEvent
│   ├── types/                  → api.ts (DeviceInfo, ScatterFile), progress.ts (ProgressEvent)
│   └── index.css              → Tailwind v4 + CSS custom properties (copper palette)
└── vendor/
    ├── fastboot-rs/            → fork of boardswarm/fastboot-rs, edition 2024
    └── format-tools/           → prebuilt mke2fs + make_f2fs (AOSP), embedded via include_bytes!
```

## Framework & styling

- **Tailwind v4** — uses `@theme` / `@theme inline` for tokens, NOT `tailwind.config.ts`
  - Static colors in `@theme`, theme-aware ones in `@theme inline` mapping CSS vars
  - Custom fonts via `@font-face` in index.css (Inter Variable, DM Mono, JetBrains Mono)
- **shadcn/ui style `base-nova`** — components use `@base-ui/react` (NOT Radix), styled via Tailwind + CVA
- **Class merging:** `cn()` from `@/lib/utils` (clsx + tailwind-merge) — always wrap className with it
- **Icons:** `lucide-react`
- **Toasts:** `sonner` (`<Toaster richColors />` in App.tsx)
- **Theming:** `.dark` class on `<html>`, CSS variables in `:root`/`.dark`, toggle via `localStorage("app-theme")`

## Critical Tauri wiring (easy to miss)

Rust commands that accept `on_event: Channel<ProgressEvent>` MUST receive a frontend-created channel:

```typescript
import { Channel } from "@tauri-apps/api/core";
const channel = new Channel<ProgressEvent>();
channel.onmessage = (event) => addProgressEvent(event);
await invoke("force_fastboot", { onEvent: channel });
```

Commands that need channels: `force_fastboot`, `disable_vbmeta`, `format_data`, `execute_plan`, `flash_gsi`, `flash_raw_image`. Omitting them causes silent runtime errors.

The Rust `ProgressEvent` enum uses `#[serde(tag = "event", content = "data")]`, so the TS type is a discriminated union: `{ event: "Phase", data: { phase, message } }` etc. Mirrored in `src/types/progress.ts`.

## React 19 quirks

- Hooks purity is strictly enforced by eslint (`react-hooks/purity`, `react-hooks/refs`).
  - `useRef(Date.now())` is flagged — use `useState(() => Date.now())` instead.
  - Reading/writing `ref.current` during render is forbidden — do it inside callbacks only.
- `react-refresh/only-export-components` — move named exports of hooks into separate files.
- `React.lazy()` + `<Suspense>` for tab components (MainTab, ToolsTab, SettingsTab).

## Code style (Rust)

- Zero `#[allow]`/`#[expect]` — fix the underlying issue.
- Max ~400 lines per file; split into directory modules with submodules.
- No `pub(crate)` helpers in type-definition files — extract to own module (e.g. `scatter_parser/util.rs`).
- Structured `tracing` with fields, never format strings in log calls: `info!(field = value, "msg")`.
- CLI output via `output::status::*` helpers (`data()`, `ok()`/`warn()`/`fail()`, `dim()`, `heading()`, `blank()`, `stderr()`), never raw `println!`/`eprintln!`.
- Rust edition 2024, MSRV 1.85. Tokio (full features). Release: LTO (thin), panic=abort.
- Clippy: `all`+`pedantic`+`perf` = warn, several = deny (see Cargo.toml `[workspace.lints.clippy]`).

## CI & release

- Push to `main` → `.github/workflows/release.yml` (check + build + release).
- Linux: `x86_64-unknown-linux-gnu`, Windows: `x86_64-pc-windows-msvc`.
- Timestamped release tag `release-YYYYMMDD-HHMMSS`, changelog from git log.
- Binary name: `force-fastboot-linux` / `force-fastboot-windows.exe`.
- Linux build dep: `libudev-dev` (for nusb USB enumeration).

## Vendored deps

- `vendor/fastboot-rs/` — fork-specific: `FastBootCommand::Flashing(s)` formats as `"flashing {0}"`, `SetActive(s)` as `"set_active:{0}"`. Bugfix: `Verify` formats as `"verify:"` not `"verity:"`.
- `vendor/format-tools/` — prebuilt mke2fs + make_f2fs, embedded at compile time.
- No generated code, no migrations, no codegen steps.
- All tests in-module (`#[cfg(test)]`), no `tests/` integration test directory.
