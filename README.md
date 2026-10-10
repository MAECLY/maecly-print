# Pluryprint

> **Open source batch file printing for Windows and macOS.** Right-click → print everything, with no 15-file limit, no Adobe required, and no $149 license fee.

[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](./docs/06-roadmap/milestones.md)
[![License](https://img.shields.io/badge/license-GPLv3-blue)](./LICENSE)
[![Stack](https://img.shields.io/badge/stack-Tauri%202%20%2B%20Rust%20%2B%20React-success)](./docs/04-architecture/tech-stack.md)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20%E2%86%92%20macOS-lightgrey)](./docs/06-roadmap/milestones.md)

---

## The problem

Windows hides the **Print** verb from the right-click menu when you select more than ~15 files. Adobe Acrobat doesn't help: it has the same effective limit and is heavyweight. The commercial alternatives that fix this — Print Conductor ($149), 2Printer ($149), Batch & Print Pro ($79.95) — are closed-source and pricey. The few open-source options are abandoned (last release in 2015), depend on Adobe Reader, or are virtual printers, not real batch printers.

So most people end up printing in tedious batches of 15-20 files, manually, again and again.

## What Pluryprint does

Pluryprint adds a native entry to the **modern Windows 11 right-click menu**:

```
Right-click on any number of files → Pluryprint ▸
                                       ├─ Print 12 files          (default profile)
                                       ├─ ──────────
                                       ├─ Print with “Duplex BW”  (one row per profile †)
                                       ├─ Print with “Photo Color”
                                       ├─ ──────────
                                       ├─ Configure print job…
                                       └─ Add to print queue
```

† The per-profile rows and the translated labels come from a `shell-menu.json` cache that the app is supposed to write. It does not write it yet, so today the handler falls back to built-in English with no profile rows.

- Select **5, 50, or 500** files. They print.
- Use **profiles** (built-in or custom) to lock in your favorite settings: duplex BW, photo color, A3 simplex, whatever.
- All **local**: no cloud, no telemetry, no account.

Two of the headline promises are still on the roadmap and are called out as such below: routing **Word, Excel, HTML and Markdown** to the right converter is M3 (today PDFs and raster images print, everything else is reported as unsupported), and **updating in place** is M4 (today the app tells you a new version exists and links to it).

## Status

**Pre-alpha · M1 (Print Core) largely delivered.** It prints. What works today:

- **Printing**, end to end: PDFs pass through untouched, raster images (JPG/JPEG, PNG, TIFF, BMP, WebP) are wrapped into a one-page PDF by a pure-Rust converter, and both are spooled through SumatraPDF on Windows or `lp` (CUPS) on macOS.
- **A persistent print queue** (`queue.json`): a single-writer dispatcher with bounded concurrency, automatic retries with backoff, per-job and whole-queue pause/resume/cancel, and crash recovery.
- **Profiles and settings on disk**: `profiles.toml` and `config.toml`, written atomically with a `schema_version`, plus profile import/export.
- **Seven GUI screens** (Home, Print Job, Queue, Printers, Profiles, Settings, About) wired to the real backend over 40 typed IPC commands — no mock data in the desktop app — with keyboard shortcuts and an English/Spanish UI.
- **A headless CLI**: `pluryprint print|printers|profiles|queue|update`, `--help` everywhere, `--json` output, and documented exit codes.
- **Context-menu handlers**: an `IExplorerCommand` COM DLL for Windows Explorer (`shell-ext-win/`, written in Rust) and an `NSServices` provider for the macOS Finder.
- **An update check** on startup and from About, with defer / skip-this-version.

**What is NOT here yet:**

- **No installer.** `installers/` is still empty, so nothing registers the Sparse MSIX, the shell extension, or a Start-menu shortcut. The Explorer DLL builds and is tested in CI, but you have to place and register it yourself.
- **No download-and-install updater.** The app detects a newer release and links to it; verifying the signature and applying the update is M4. Channel selection is persisted but every channel reads the same feed.
- **No multi-format adapters.** Office, ODF, HTML, Markdown, TXT/RTF and HEIC are rejected with an explicit reason, not converted. That is M3.
- **No bundled SumatraPDF.** The app locates an existing `SumatraPDF.exe`; a missing one pauses the job with a dependency error.
- **No code signing**, so expect SmartScreen and Gatekeeper warnings on anything you build.

See the [roadmap](./docs/06-roadmap/milestones.md) for the full milestone path from `M0 — Foundation` through `M5 — macOS Port`, and the [functional requirements](./docs/03-requirements/functional-requirements.md) for per-requirement status.

## Quick start (developers)

```powershell
# Prerequisites (one-time):
#   1. Node 20 LTS + pnpm 9    -> https://nodejs.org, then `npm i -g pnpm`
#   2. Rust stable 1.78+       -> https://rustup.rs
#   3. Visual Studio Build Tools 2022 with the "Desktop development with C++"
#      workload (incl. Windows 11 SDK + ATL + Spectre Mitigations)
#   4. WebView2 Runtime (preinstalled on Windows 11)
# Detailed checklist in docs/08-development/local-setup.md.

git clone https://github.com/<org>/maecly-print.git
cd maecly-print
pnpm install

# Web preview only (no Tauri host, no IPC):
pnpm dev

# Full Tauri dev loop (requires Rust toolchain):
pnpm tauri dev
```

The web preview renders every screen without the Rust toolchain, but it has no Tauri host: IPC calls fail, so the screens show their empty and error states rather than real printers, jobs or profiles. Use it for layout and styling work only. `pnpm tauri dev` boots the real app against the real backend.

The Explorer context-menu DLL is a separate crate with its own target directory — build and test it with `cargo build --manifest-path shell-ext-win/Cargo.toml` (Windows only; see [`shell-ext-win/README.md`](./shell-ext-win/README.md) for registering it by hand until the installer lands).

## Documentation

All documentation lives under [`docs/`](./docs/README.md). Recommended starting points:

- [Product Vision](./docs/01-vision/product-vision.md)
- [Value Proposition](./docs/01-vision/value-proposition.md)
- [System Architecture](./docs/04-architecture/system-architecture.md)
- [MVP Features](./docs/05-features/mvp-features.md)
- [Roadmap & Milestones](./docs/06-roadmap/milestones.md)
- [Architecture Decision Records](./docs/adr/README.md)

## Stack

| Layer                  | Technology                                                  | State                    |
| ---------------------- | ----------------------------------------------------------- | ------------------------ |
| Application framework  | [Tauri 2](https://tauri.app)                                | In use                   |
| Backend                | Rust                                                        | In use                   |
| Frontend               | React + TypeScript + Vite + Tailwind CSS                    | In use                   |
| Print engine (Win)     | [SumatraPDF](https://www.sumatrapdfreader.org/) (CLI)       | In use                   |
| Print engine (macOS)   | CUPS (`lp`)                                                 | In use                   |
| Image → PDF            | Rust `image` + `lopdf` (no external binary)                 | In use                   |
| Shell extension (Win)  | Rust COM DLL implementing `IExplorerCommand` + Sparse MSIX  | DLL in use; MSIX planned |
| Finder integration     | `NSServices` provider inside the app (no separate `.appex`) | In use                   |
| Update check           | `ureq` against the GitHub releases API                      | In use                   |
| Office / ODF / RTF     | [LibreOffice](https://www.libreoffice.org/) (headless)      | M3                       |
| HTML conversion        | Chromium (headless)                                         | M3                       |
| Markdown conversion    | [Pandoc](https://pandoc.org/)                               | M3                       |
| Image resampling       | [ImageMagick](https://imagemagick.org/)                     | M3                       |
| Installer (Win)        | [Inno Setup](https://jrsoftware.org/isinfo.php)             | Planned                  |
| Auto-updater (install) | Tauri Updater plugin                                        | M4                       |

Rows marked M3/M4/Planned are decided but not yet wired; the app already detects LibreOffice, Chromium and Pandoc on the machine and reports them, but no adapter calls them yet. Justification in [tech-stack.md](./docs/04-architecture/tech-stack.md) and the [ADRs](./docs/adr/README.md).

## License

[GPLv3](./LICENSE). Pluryprint is and will remain free software. See [docs/09-legal/license.md](./docs/09-legal/license.md) for what that means in practice.

## Contributing

Contributions are welcome — see [CONTRIBUTING](./docs/08-development/contributing.md). Before opening a PR for non-trivial changes, please file an issue or start a discussion.

If you want to contribute a translation, see the [internationalization section](./docs/08-development/contributing.md#translations).

## Project name and trademark

"Pluryprint" is a trademark of the project. Forks must rebrand. See [trademark policy](./docs/09-legal/trademark.md).

## Privacy

No telemetry by default. No cloud. Your files never leave your machine. See [privacy](./docs/09-legal/privacy.md).

## Acknowledgements

Pluryprint stands on the shoulders of these excellent open-source projects:

- [Tauri](https://tauri.app), [Rust](https://www.rust-lang.org/), [React](https://react.dev/)
- [SumatraPDF](https://www.sumatrapdfreader.org/) for fast, reliable PDF printing on Windows
- [LibreOffice](https://www.libreoffice.org/) for Office and ODF conversion
- [Chromium](https://www.chromium.org/), [Pandoc](https://pandoc.org/), [ImageMagick](https://imagemagick.org/), [qpdf](https://github.com/qpdf/qpdf)

Full list and licenses in [third-party-licenses.md](./docs/09-legal/third-party-licenses.md).
