# Tech Stack

> **TL;DR** — MaeclyPrint is built on **Tauri 2 + Rust + React (TypeScript)**. This combination gives us a 5-10 MB installer, sub-500 ms cold start, 30-50 MB idle RAM, a built-in signed auto-updater, and a 95% shared codebase across Windows and macOS — which is exactly what a context-menu-invoked utility needs.

## 1. Stack at a glance

| Layer                  | Choice                                                  |
| ---------------------- | ------------------------------------------------------- |
| Application shell      | **Tauri 2.x** (Rust host + system webview)              |
| Backend / core logic   | **Rust 1.78+** (stable, edition 2021)                   |
| Frontend / UI          | **React 18 + TypeScript 5 + Vite 5 + Tailwind CSS 3**   |
| Async runtime          | `tokio` (multi-threaded, with `rt-multi-thread`)        |
| Serialization          | `serde`, `serde_json`, `toml`                           |
| Logging / tracing      | `tracing`, `tracing-subscriber`, `tracing-appender`     |
| Windows API bindings   | `windows-rs` (official MS crate), `winreg`              |
| macOS API bindings     | `objc2`, `core-foundation`, `core-graphics` (M5+)       |
| Process orchestration  | `tokio::process`, `which` (locate external binaries)    |
| Shell extension (Win)  | **C++17** COM DLL implementing `IExplorerCommand`       |
| Shell extension (Mac)  | **Swift** Finder Sync Extension (M5+)                   |
| Installer (Win)        | **Inno Setup 6** + **Sparse MSIX** for shell identity   |
| Installer (Mac)        | `.dmg` + notarized `.app` bundle (M5+)                  |
| Auto-update            | `tauri-plugin-updater` (MIT, built-in)                  |
| CI/CD                  | GitHub Actions (Windows + macOS runners)                |
| Code signing           | DigiCert / Sectigo EV cert (Win), Apple Developer ID    |

## 2. Why Tauri 2

Tauri 2 was selected after benchmarking five competing frameworks. The full comparison appears in [`../02-research/`](../02-research/), but the headline numbers driving the choice are:

| Stack             | Installer  | RAM idle    | Cold start | Auto-update           | Cross-platform        | Score |
| ----------------- | ---------- | ----------- | ---------- | --------------------- | --------------------- | ----- |
| **Tauri 2**       | 5-10 MB    | 30-50 MB    | < 500 ms   | Built-in (MIT)        | ✓ Win/Mac/Linux       | 9/10  |
| Electron          | 80-150 MB  | 200-300 MB  | 1-2 s      | `autoUpdater` module  | ✓ but overkill        | 5/10  |
| .NET MAUI / Avalonia | ~50 MB  | 80-150 MB   | Fast       | Velopack / NetSparkle | ✓ Avalonia only       | 6/10  |
| Wails (Go)        | 5-15 MB    | 20-40 MB    | < 500 ms   | None built-in         | ✓ smaller community   | 6.5/10|
| Flutter Desktop   | 50-100 MB  | 100-150 MB  | Fast       | None built-in         | ✓ shell ext. awkward  | 4/10  |
| Qt / PySide6      | 100+ MB    | 80-150 MB   | Fast       | None built-in         | ✓ mature but verbose  | 5/10  |

### Why each metric matters for MaeclyPrint

- **Installer size.** MaeclyPrint is invoked by people who right-click a file and expect the dialog to appear instantly. Bundle bloat hurts the install experience and slows updates.
- **RAM idle.** The app may sit in the tray (or be re-launched on every right-click). Electron's 250 MB baseline is unacceptable for a utility.
- **Cold start.** Right-click latency is perceived directly by users. Anything over ~700 ms feels broken.
- **Auto-update.** A polished updater is non-negotiable for a desktop app shipped outside an app store. Tauri ships one as a first-party MIT plugin; competitors require third-party glue.
- **Cross-platform.** macOS is on the roadmap (M5). A 95% shared codebase means the Mac port is a port, not a rewrite.

### Concrete Tauri 2 advantages

1. **Updater plugin.** Generates delta diffs, verifies minisign signatures, supports either a static JSON manifest on S3/CDN or a dynamic server. Hosting on `updates.maecly.com` (S3 + CloudFront) means we are not coupled to GitHub Releases.
2. **System webview.** Uses WebView2 on Windows and WKWebView on macOS — no Chromium bundled. This is the source of the small installer and low RAM.
3. **Rust backend.** Memory-safe, zero garbage-collection pauses, fast startup, excellent FFI for the COM DLL boundary, and a strong ecosystem (`tokio`, `serde`, `tracing`).
4. **Type-safe IPC.** `tauri::command` macros plus generated TypeScript bindings (`tauri-specta` or hand-rolled) make the React ↔ Rust boundary safe.
5. **First-class Windows support.** Tauri 2 fixed several Windows pain points from v1 (better installer story, MSIX support, mobile, capabilities system).
6. **Capabilities security model.** Tauri 2's permission system lets us narrow the webview's reachable IPC commands to a minimum.

## 3. Why Rust for the core

The Rust core handles file enumeration, format detection, conversion orchestration, queue management, IPC, the CLI, and the updater logic. It was chosen because:

- **Memory safety** in a process that ingests untrusted file paths and shells out to converters. A use-after-free in the print queue would be embarrassing.
- **Performance.** Enumerating thousands of files, dispatching conversions in parallel, and maintaining an IPC stream to the webview is throughput-bound. Rust + `tokio` excels here.
- **Single binary deployment.** Statically linked Rust binaries ship without a runtime — important for a utility that should never make the user "install Java" or ".NET Framework".
- **Ecosystem maturity.** All the crates we need exist and are mature (`serde`, `tokio`, `tracing`, `windows-rs`, `clap`, `notify`, `which`, `mime_guess`, `walkdir`, `rayon`).
- **FFI to C/C++.** The shell extension is a C++ COM DLL. Rust talks to it via well-defined IPC, but having the option of FFI is reassuring.

### Rust dependencies (key crates)

| Crate              | Purpose                                              |
| ------------------ | ---------------------------------------------------- |
| `tauri`            | App framework                                        |
| `tauri-plugin-updater` | Auto-update                                      |
| `tauri-plugin-cli` | CLI parsing integration                              |
| `tokio`            | Async runtime, process management                    |
| `serde`, `serde_json`, `toml` | Config / queue / IPC serialization        |
| `tracing`, `tracing-subscriber`, `tracing-appender` | Structured logging |
| `clap`             | CLI argument parsing (when running headless)         |
| `windows`          | Win32 API bindings (printers, shell, registry)       |
| `winreg`           | Higher-level registry helpers                        |
| `which`            | Locate `soffice`, `magick`, `pandoc`, `SumatraPDF`   |
| `mime_guess`       | Format detection from extension                      |
| `infer`            | Format detection from magic bytes (fallback)         |
| `walkdir`          | Directory enumeration                                |
| `rayon`            | Data-parallel format conversion                      |
| `thiserror`        | Library-level error types                            |
| `anyhow`           | Application-level error wrapping                     |
| `directories`      | XDG-conformant config / data / cache paths           |
| `notify`           | Filesystem watching (queue.json hot reload)          |
| `minisign-verify`  | Verify update manifest signatures                    |

## 4. Why React + TypeScript for the UI

The frontend is a small SPA: a print dialog, a profile editor, a queue viewer, and a settings page. We chose React + TypeScript because:

- **Productivity.** Building rich form UIs in React with hooks and component libraries is fast.
- **TypeScript** catches IPC mismatches at compile time when paired with generated bindings.
- **Ecosystem.** `react-hook-form` for the print dialog, `@tanstack/react-query` for IPC state, `zustand` for local UI state, `radix-ui` + `tailwind` for accessible primitives, `lucide-react` for icons.
- **Testability.** `vitest` + `@testing-library/react` is a well-trodden path.

### Frontend dependencies (key packages)

| Package                  | Purpose                                       |
| ------------------------ | --------------------------------------------- |
| `react`, `react-dom`     | UI runtime                                    |
| `vite`                   | Dev server + bundler                          |
| `typescript`             | Static typing                                 |
| `tailwindcss`            | Utility CSS                                   |
| `@radix-ui/react-*`      | Accessible primitives (Dialog, Select, Tabs) |
| `react-hook-form`        | Form state                                    |
| `@tanstack/react-query`  | Async IPC state                               |
| `zustand`                | Local UI state                                |
| `lucide-react`           | Icon set                                      |
| `clsx`, `tailwind-merge` | Conditional class names                       |
| `vitest`                 | Unit tests                                    |
| `@testing-library/react` | Component tests                               |
| `playwright`             | E2E (drives the WebView2 webview)             |

### Frontend conventions

- No `index.css` global file beyond Tailwind directives. All component styling is via Tailwind utilities or CSS modules.
- All IPC calls go through a typed wrapper module (`src/ipc/index.ts`). No raw `invoke()` calls in components.
- All component boundaries are pure presentational + a hook that talks to IPC.
- State that crosses route boundaries lives in `zustand`. Anything component-local stays in `useState`.

## 5. Stacks we considered and rejected

### 5.1 Electron

**Pros:** ubiquitous, huge community, mature auto-updater (`electron-updater`), batteries-included.
**Cons:**
- Bundles Chromium → 80-150 MB installer.
- 200-300 MB RAM idle.
- Cold start in the 1-2 s range; unacceptable for a context-menu utility.
- Node.js in main process: extra GC pauses, weaker isolation story than Rust.
- Repeated security advisories for popular Electron apps suggest the surface area is large.

**Verdict:** rejected on installer size, RAM, and cold start.

### 5.2 .NET MAUI / Avalonia (C#)

**Pros:** strong tooling (Visual Studio, Rider), mature .NET BCL, native Win32 interop, decent cross-platform via Avalonia.
**Cons:**
- Self-contained .NET runtime adds 30-50 MB even after trimming.
- No first-party auto-updater. Velopack is good, but third-party.
- COM interop is doable but more friction than C++/Rust.
- macOS story (Avalonia) is less polished than Tauri's WKWebView path.

**Verdict:** rejected because no built-in updater and a bigger footprint than Tauri.

### 5.3 Wails (Go)

**Pros:** very small binaries, fast cold start, simple Go syntax, similar webview model to Tauri.
**Cons:**
- No built-in updater.
- Smaller community; fewer plugins, less battle-tested for shell extension integration.
- Go's GC is fine but adds latency variability that Rust avoids.

**Verdict:** rejected. Tauri is structurally similar but more featureful (updater, capabilities, mobile).

### 5.4 Flutter Desktop

**Pros:** beautiful UI, fast iteration, single codebase across mobile + desktop.
**Cons:**
- Renders its own widgets (Skia) → larger bundle, no native look-and-feel.
- No built-in auto-updater.
- Shell extension integration on Windows is awkward (no clean FFI to a C++ COM DLL).
- Dart ecosystem outside Google is thin for desktop concerns.

**Verdict:** rejected. Wrong fit for a "small native utility" target.

### 5.5 Qt / PySide6

**Pros:** mature, professional, exhaustive widgets, cross-platform.
**Cons:**
- LGPL dynamic linking adds compliance complexity.
- 100+ MB installer when bundled.
- Verbose to develop in compared to React.
- No built-in updater.

**Verdict:** rejected on installer size, LGPL friction, and verbosity.

### 5.6 Rust + egui / Slint (no webview)

**Pros:** smallest possible binary, fastest startup, no webview dependency.
**Cons:**
- Custom-rendered UIs feel non-native on Windows; accessibility (NVDA, narrator) is weaker.
- The print profile editor and dialog need rich form controls; building them in egui is a slog.
- Theming and dark-mode tracking is more work.

**Verdict:** rejected. Webview UX wins for form-heavy desktop utilities.

## 6. Toolchain and supported versions

| Tool          | Required version    |
| ------------- | ------------------- |
| Rust          | 1.78 stable+        |
| Node.js       | 20 LTS+             |
| pnpm          | 9.x (preferred over npm/yarn) |
| Tauri CLI     | 2.x                 |
| Visual Studio Build Tools | 2022 (Win)         |
| Windows SDK   | 10.0.22621.0+ (for WebView2 + COM)  |
| WebView2 Runtime | Evergreen (auto-installed by Win 11; bootstrapper for Win 10) |
| Xcode         | 15+ (Mac, M5+)      |
| Inno Setup    | 6.2+                |

The `rust-toolchain.toml` and `package.json` in the repo pin exact versions. CI (`.github/workflows/ci.yml`) verifies all of the above on every PR.

## 7. Where this decision is recorded

- ADR [`0001-stack-tauri-rust-react`](../adr/0001-stack-tauri-rust-react.md) — the canonical record of this decision, with full alternatives and consequences.
- ADR [`0002-license-gplv3`](../adr/0002-license-gplv3.md) — license choice that constrains some dependencies.
- ADR [`0004-auto-updater-tauri-builtin`](../adr/0004-auto-updater-tauri-builtin.md) — updater choice tied to Tauri.

## 8. Related documents

- [`system-architecture.md`](./system-architecture.md) — how the layers fit together.
- [`shell-extensions.md`](./shell-extensions.md) — the C++/Swift glue at the OS boundary.
- [`auto-updater.md`](./auto-updater.md) — Tauri updater configuration in detail.
- [`security-model.md`](./security-model.md) — what we sign, what we sandbox, and why.
