# ADR-0001: Stack — Tauri 2 + Rust + React (TypeScript)

## Status

Accepted

## Date

2026-05-03

## Context

MaeclyPrint is a Windows-first (and later macOS) desktop utility invoked from the Explorer/Finder context menu to batch-print files. The user-perceived workflow is:

1. Right-click N files (potentially hundreds).
2. Click "Print all with MaeclyPrint".
3. A small dialog appears almost instantly, lets the user pick a profile, and the queue starts.

The performance budget for step 2-3 is harsh — the dialog must appear in well under one second, and ideally in 200-400 ms. The app must also:

- Run on Windows 10 and 11 (x64 and ARM64), and later macOS 13+ (Apple Silicon and Intel).
- Ship with an auto-updater that supports signature verification and works without going through a store.
- Integrate cleanly with the OS shell (right-click extension, sparse MSIX on Windows, Quick Action / Finder Sync on Mac).
- Ship a CLI in the same binary for headless use.
- Be open source (AGPL-3.0 — see ADR-0010, which superseded ADR-0002's GPLv3), so we cannot rely on proprietary frameworks.
- Be developed by a small team (initially solo). Maintenance burden matters more than feature ceiling.

We need to choose the application framework and the language(s) the project lives in.

## Decision

We will build MaeclyPrint on **Tauri 2** with a **Rust** backend and a **React + TypeScript** frontend (built with Vite, styled with Tailwind CSS).

Concretely:

- The single Rust binary is the application — it hosts the webview, the print engine, the queue, and the CLI.
- The webview UI is a small React SPA bundled at build time and served from the Rust process.
- The Tauri Updater plugin (MIT) handles auto-updates with cryptographically signed manifests.
- A separate **C++17 COM DLL** (Win) and **Swift Finder Sync Extension** (Mac, post-M5) handle shell integration; these are platform-specific and minimal.

## Alternatives Considered

| Option                            | Why rejected                                                                                   |
| --------------------------------- | ---------------------------------------------------------------------------------------------- |
| **Electron** (Node.js + Chromium) | 80-150 MB installer, 200-300 MB idle RAM, 1-2 s cold start. Unacceptable for a context-menu utility. Auto-updater (`electron-updater`) is fine but third-party. |
| **.NET MAUI / Avalonia** (C#)     | Self-contained .NET runtime adds 30-50 MB. No first-party auto-updater (third-party Velopack is good but external). C# COM interop is doable but more friction than C++/Rust. macOS story (Avalonia) is less polished. |
| **Wails** (Go + system webview)   | Structurally similar to Tauri but no built-in updater, smaller community, fewer plugins, less battle-tested for shell integration. |
| **Flutter Desktop**               | Renders own widgets (Skia) — non-native look, larger bundle, no built-in updater, awkward FFI for COM DLL. |
| **Qt / PySide6**                  | LGPL friction, 100+ MB installer when bundled, verbose to develop, no built-in updater.        |
| **Rust + egui / Slint** (no webview) | Smallest possible binary and fastest startup, but custom-rendered UIs feel non-native, accessibility (NVDA) is weaker, and form-heavy UIs are tedious to build. |
| **Native Win32 + WinUI 3 (C++/WinRT)** | Maximum native fidelity. Rejected because cross-platform (Mac) would require a complete rewrite. The 95% codebase-share advantage of Tauri is decisive. |

A scored comparison from research:

| Stack                | Installer | RAM idle    | Cold start | Auto-update           | Cross-platform        | Score |
| -------------------- | --------- | ----------- | ---------- | --------------------- | --------------------- | ----- |
| **Tauri 2**          | 5-10 MB   | 30-50 MB    | < 500 ms   | Built-in (MIT)        | ✓ Win/Mac/Linux       | 9/10  |
| Electron             | 80-150 MB | 200-300 MB  | 1-2 s      | `autoUpdater` module  | ✓ but overkill        | 5/10  |
| .NET MAUI / Avalonia | ~50 MB    | 80-150 MB   | Fast       | Velopack / NetSparkle | ✓ Avalonia only       | 6/10  |
| Wails (Go)           | 5-15 MB   | 20-40 MB    | < 500 ms   | None built-in         | ✓ smaller community   | 6.5/10|
| Flutter Desktop      | 50-100 MB | 100-150 MB  | Fast       | None built-in         | ✓ shell ext awkward   | 4/10  |
| Qt / PySide6         | 100+ MB   | 80-150 MB   | Fast       | None built-in         | ✓ mature but verbose  | 5/10  |

## Consequences

### Positive

- **Tiny installer (5-10 MB) and small idle RAM (30-50 MB).** Both critical for a context-menu utility.
- **Cold start < 500 ms** because we use the system webview (WebView2 / WKWebView), not bundled Chromium.
- **Built-in signed auto-updater** via the Tauri Updater plugin — no third-party dependency to maintain, no store gatekeeping.
- **Memory-safe Rust core** with strong concurrency primitives (`tokio`) for the queue and converter orchestration.
- **95% shared codebase** between Windows and macOS — the macOS port is genuinely a port, not a rewrite. Only shell integration and a few `cfg(target_os=...)` paths differ.
- **Productive UI development** with React + TypeScript + Tailwind — large ecosystem (`react-hook-form`, `radix-ui`, `lucide-react`).
- **Type-safe IPC** when paired with `tauri-specta` (or hand-rolled bindings).
- **Capabilities security model** in Tauri 2 narrows the webview's reachable IPC surface.

### Negative

- **Three-language build environment.** Contributors need Rust, Node.js, and (for the shell ext) C++ build tooling. Higher onboarding cost than a single-language stack.
- **Smaller community than Electron.** Fewer Stack Overflow answers, fewer pre-built plugins. Mitigated by Tauri's active maintainer team and good docs.
- **WebView2 dependency on Win 10.** Auto-installed on Win 11; needs a bootstrapper on Win 10. Mitigated by Microsoft's own bootstrapper.
- **System webview means we don't fully control the rendering engine.** Edge updates can subtly change layout. Mitigated by avoiding bleeding-edge CSS and testing on a stable WebView2 channel in CI.

### Risks / Open Questions

- **Tauri 2 is relatively new (2024 GA).** API surface could still shift in minor ways. We pin the Tauri version and read the changelog before bumping.
- **Capability system maturity.** Tauri 2's permissions model is improving. We will keep the allowlist conservative and audit it each release.
- **Mobile (post-1.0).** Tauri 2 supports iOS/Android, but MaeclyPrint is desktop-first; mobile is not a goal.
- **WebView2 ARM64.** Currently shipping on Windows ARM64. We will validate this in the M2 nightly builds.

## Related

- ADR-0010 (license: AGPL-3.0-only, superseding ADR-0002) constrains some dependency choices.
- ADR-0004 (auto-updater) depends on this choice.
- [`../04-architecture/tech-stack.md`](../04-architecture/tech-stack.md) — full reference of the stack and dependencies.
