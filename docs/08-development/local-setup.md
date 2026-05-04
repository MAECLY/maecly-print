# Local Setup

> **TL;DR.** Install Rust stable, Node 20 LTS + pnpm, the platform's native build tools (Visual Studio Build Tools 2022 on Windows; Xcode CLT on macOS), then `pnpm install && pnpm tauri dev`. The app boots with hot reload in under 30 seconds on a typical dev machine.

This document is the developer onboarding checklist. Follow it once on a clean machine; afterward, daily work is `git pull && pnpm install && pnpm tauri dev`.

## Prerequisites

### Windows 10/11

| Tool | Version | Install via |
|---|---|---|
| Rust | stable (1.80+) | [rustup.rs](https://rustup.rs/) — accepts default options |
| Node.js | 20 LTS | [nodejs.org](https://nodejs.org/) or `winget install OpenJS.NodeJS.LTS` |
| pnpm | latest | `npm install -g pnpm` |
| Visual Studio Build Tools 2022 | latest | [VS Build Tools download](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022) |
| Windows SDK | 10.0.22621 or newer | Bundled with VS Build Tools |
| WebView2 Runtime | latest | Preinstalled on Win 11; for Win 10, [bootstrap installer](https://developer.microsoft.com/microsoft-edge/webview2/) |
| Git | latest | [git-scm.com](https://git-scm.com/) or `winget install Git.Git` |
| Inno Setup 6 | 6.x | [innosetup.com](https://jrsoftware.org/isinfo.php) — only needed for installer builds |

#### Visual Studio Build Tools components

When installing VS Build Tools, select these workloads in the installer:

- **Desktop development with C++** (the workload)
- Within that workload, ensure these individual components are checked:
  - MSVC v143 - VS 2022 C++ x64/x86 build tools
  - Windows 11 SDK (10.0.22621 or newer)
  - C++ ATL for latest v143 build tools (x86 & x64)
  - C++ ATL for latest v143 build tools with Spectre Mitigations (x86 & x64) — required for shell extension build

Without ATL, the `shell-ext-win/` build will fail with cryptic linker errors.

### macOS 13+ (M5+ work only)

| Tool | Version | Install via |
|---|---|---|
| Rust | stable | [rustup.rs](https://rustup.rs/) |
| Node.js | 20 LTS | [nodejs.org](https://nodejs.org/) or `brew install node@20` |
| pnpm | latest | `npm install -g pnpm` |
| Xcode | 15+ | [Mac App Store](https://apps.apple.com/us/app/xcode/id497799835) |
| Xcode Command Line Tools | latest | `xcode-select --install` |
| Homebrew | latest | [brew.sh](https://brew.sh/) |
| create-dmg | latest | `brew install create-dmg` |
| LibreOffice | latest | `brew install --cask libreoffice` (optional, for end-to-end Office tests) |

### Optional but recommended (both platforms)

- **VS Code** with extensions: `rust-lang.rust-analyzer`, `tamasfe.even-better-toml`, `dbaeumer.vscode-eslint`, `esbenp.prettier-vscode`, `bradlc.vscode-tailwindcss`.
- **JetBrains RustRover** as an alternative IDE if you prefer.
- **DBeaver** or similar for inspecting SQLite (we may add a local SQLite store post-1.0).

## Initial setup

```powershell
# Clone
git clone https://github.com/<org>/maecly-print.git
cd maecly-print

# Install dependencies (frontend + Tauri CLI via package.json)
pnpm install

# First-time Rust setup: this will fetch and compile dependencies
# (~5 minutes the first time, cached thereafter)
cd src-tauri
cargo build
cd ..
```

On macOS the first two commands are identical. The third uses bash (`cd src-tauri && cargo build && cd ..`), which `pnpm install` doesn't trigger.

After `pnpm install`, you should have:

- `node_modules/` populated.
- `src-tauri/target/debug/maeclyprint(.exe)` built.
- No errors from either step.

If errors appear, see [Troubleshooting](#troubleshooting) below.

## Daily dev workflow

### Hot-reload dev server

```powershell
pnpm tauri dev
```

This:

1. Starts Vite dev server (frontend hot reload on port 1420).
2. Builds the Rust backend in debug mode.
3. Launches the app, pointing the webview at `http://localhost:1420`.
4. Watches `src/` for frontend changes (instant) and `src-tauri/src/` for backend changes (full rebuild + relaunch).

Backend changes take 5-30 seconds to take effect (cargo incremental compile + relaunch). Frontend changes are sub-second.

### Running tests

```powershell
# Rust tests (unit + integration)
cd src-tauri
cargo test
cd ..

# Frontend tests (Vitest)
pnpm test

# Both, in one shot, with formatting + linting:
pnpm check-all
```

The `pnpm check-all` script chains: `pnpm lint`, `pnpm prettier --check`, `pnpm typecheck`, `pnpm test`, `(cd src-tauri && cargo fmt --check && cargo clippy && cargo test)`.

### Production build (local)

```powershell
pnpm tauri build
```

Output:

- Windows: `src-tauri/target/release/bundle/nsis/maeclyprint_<version>_x64-setup.nsis.zip` (Tauri's native bundler) and the binary at `src-tauri/target/release/maeclyprint.exe`.
- macOS: `src-tauri/target/release/bundle/macos/MaeclyPrint.app` and `.app.tar.gz`.

The Inno Setup installer (`.exe`) requires the separate `installers/windows/build.ps1` script (see [installer-windows.md](../07-build-and-release/installer-windows.md)). The Apple-signed `.pkg` and `.dmg` require `installers/macos/` scripts.

### CLI usage during dev

The Rust binary can also be invoked as a CLI:

```powershell
# Print a single PDF to a specific printer
src-tauri\target\debug\maeclyprint.exe print sample.pdf --printer "Microsoft Print to PDF"

# Print multiple files using a profile
src-tauri\target\debug\maeclyprint.exe print *.pdf --profile draft
```

This bypasses the GUI and is useful for backend debugging.

## Debugging

### Rust backend

**VS Code with CodeLLDB:**

`launch.json`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "name": "Debug Tauri (Rust backend)",
      "type": "lldb",
      "request": "launch",
      "cargo": {
        "args": ["build", "--manifest-path=src-tauri/Cargo.toml", "--no-default-features"],
        "filter": { "name": "maeclyprint", "kind": "bin" }
      },
      "args": [],
      "cwd": "${workspaceFolder}",
      "env": {
        "RUST_LOG": "maeclyprint=debug,tauri=info"
      }
    }
  ]
}
```

Set breakpoints in `src-tauri/src/`, hit F5. The debugger attaches to the Rust process; the webview runs detached.

**Logging-based debug:**

Set `RUST_LOG`:

```powershell
$env:RUST_LOG = "maeclyprint=debug,tauri=info,reqwest=warn"
pnpm tauri dev
```

Module-specific filters: `maeclyprint::engine=trace` for very detailed output from the engine module.

### TypeScript frontend

**Browser DevTools:**

Right-click anywhere in the app webview → "Inspect Element" (or press F12). This opens Chromium DevTools (Tauri uses WebView2 on Windows, WKWebView on macOS — DevTools work on both).

In production builds, DevTools are disabled. Set `devtools = true` in `tauri.conf.json` only for dev or rebuild a debug binary.

**VS Code with Edge Tools or Chrome Debugger:**

Less reliable than browser DevTools for Tauri because the webview's HTTP server lives behind Tauri's IPC. Browser DevTools are the standard.

### Mixed Rust + TS debugging

When an IPC call fails, check both sides:

1. JS side: in browser DevTools console, look for the `invoke()` rejection.
2. Rust side: in the terminal running `pnpm tauri dev`, look for `tracing` output around the same timestamp.

The IPC layer adds a `command` field to log lines; grep for the command name.

## Connecting a real printer

For end-to-end testing, you need at least one printer. Three options:

### Microsoft Print to PDF (Windows)

Preinstalled on Win 10/11. Always available, never offline, outputs `.pdf` files. **Use this for CI and the default dev loop.**

```powershell
# Verify it's available
Get-Printer | Where-Object { $_.Name -eq "Microsoft Print to PDF" }
```

### CUPS-PDF (macOS)

Install via `brew install cups-pdf`. Outputs `.pdf` to `~/Desktop` by default. macOS equivalent of Microsoft Print to PDF.

### A real physical printer

Install the manufacturer's driver. Test with the OS's standard print dialog first to confirm the driver works; then test through MaeclyPrint.

For driver-quirky printers (HP, Brother), be prepared for issues that aren't MaeclyPrint's fault. We track known-good printers in a separate compatibility matrix doc.

## Verifying the install

Run this smoke test sequence after first setup:

```powershell
# 1. Run unit tests
cd src-tauri
cargo test
# Expect: tests pass, ~5-15 seconds.
cd ..

# 2. Lint everything
pnpm check-all
# Expect: all checks green.

# 3. Boot the dev app
pnpm tauri dev
# Expect: app window opens within ~30 seconds.

# 4. From the GUI: navigate to Settings, see your printer list.
# Expect: at least one printer listed (e.g. "Microsoft Print to PDF").

# 5. Print a test PDF.
# Expect: PDF prints; queue page shows the job; output appears (or paper comes out).
```

If any step fails, see Troubleshooting.

## Troubleshooting

### Windows: `link.exe not found`

VS Build Tools incomplete or not on PATH. Reinstall, ensure "Desktop development with C++" workload is selected, and **restart your shell** (build tools modify PATH on install but existing shells don't pick it up).

### Windows: cargo build fails with `windows.h not found`

Windows SDK missing. In VS Build Tools installer, add "Windows 11 SDK (10.0.22621.0)" or newer.

### Windows: `error: linking with link.exe failed: exit code: 1120` mentioning `RoGetActivationFactory` or COM symbols

ATL not installed. In VS Build Tools installer, add "C++ ATL for latest v143 build tools (x86 & x64)".

### Windows: `pnpm tauri dev` says WebView2 not found

Install WebView2 Runtime from [https://developer.microsoft.com/microsoft-edge/webview2/](https://developer.microsoft.com/microsoft-edge/webview2/). On Win 11 this is preinstalled and shouldn't happen.

### macOS: `xcode-select --install` opens a dialog that says "already installed"

You're fine. Proceed.

### macOS: cargo build fails with `ld: library not found for -lSystem`

Run `xcode-select --install` and accept the license: `sudo xcodebuild -license accept`.

### macOS: build slow on M-series

Ensure you have the right Rust target: `rustup target add aarch64-apple-darwin` (should be the default on M-series; verify with `rustc --print target-list | grep apple-darwin`).

### Both: `pnpm tauri dev` is "stuck" with no window

First-time builds can take 5-15 minutes. Watch for cargo progress in the terminal. If it's stuck for >20 minutes, kill it (Ctrl+C) and check `cargo build --verbose` from `src-tauri/` for actual errors.

### Both: `cargo test` fails with permission errors on a temp file

Some tests use temp files; antivirus on Windows or `tmpfs` quirks on macOS can interfere. Try setting `TMPDIR`:

```powershell
$env:TMPDIR = "$env:TEMP\maeclyprint-test"
mkdir $env:TMPDIR -Force
cargo test
```

### Both: rust-analyzer is slow / unresponsive

The Rust language server indexes `target/`. Make sure `target/` is excluded from VS Code's `files.watcherExclude`:

```json
{
  "files.watcherExclude": {
    "**/target/**": true,
    "**/node_modules/**": true
  }
}
```

### Both: `pnpm install` fails on `husky` postinstall

Husky may not be set up if you're cloning a fork. Skip with `pnpm install --ignore-scripts`, or run `pnpm prepare` separately.

### Windows: shell extension DLL doesn't appear in Explorer

The DLL needs to be registered. The dev workflow uses Inno Setup for full registration, which is heavy for daily dev. For testing the shell extension specifically, see [installer-windows.md](../07-build-and-release/installer-windows.md) for the manual `regsvr32` flow.

You'll likely need to log out + log in (or restart `explorer.exe`) for changes to take effect, because Explorer caches shell extension DLLs.

## Performance tips

- **Use a fast SSD.** Cargo and pnpm both touch many small files; HDD or slow SSD will cause minute-scale builds.
- **Increase Rust parallelism:** `export CARGO_BUILD_JOBS=8` (or whatever your CPU count). Default is "number of cores", which is usually right.
- **Use `cargo nextest`** for faster test runs: `cargo install cargo-nextest --locked && cargo nextest run`. We don't require it but it's significantly faster than `cargo test` for our suite.
- **Disable antivirus realtime scan on `target/`** if your AV is aggressive (Windows Defender by default, Symantec, etc.). On Defender: `Add-MpPreference -ExclusionPath "C:\path\to\maecly-print\src-tauri\target"`.

## Updating local environment

Periodically:

```powershell
# Update Rust
rustup update stable

# Update Node deps (review the diff before committing)
pnpm update --interactive

# Update Cargo deps
cd src-tauri
cargo update
cd ..
```

These changes go through PR review like any code change.

## See also

- [coding-standards.md](./coding-standards.md)
- [contributing.md](./contributing.md)
- [testing-strategy.md](./testing-strategy.md)
- [repo-structure.md](./repo-structure.md)
- [../07-build-and-release/installer-windows.md](../07-build-and-release/installer-windows.md)
