# Repository Structure

> **TL;DR.** The repository is split into a Rust backend (`src-tauri/`), a React frontend (`src/`), a C++ shell extension (`shell-ext-win/`), platform-specific installers (`installers/`), and the documentation tree (`docs/`). Each directory has a single responsibility and clear boundaries; this document explains why.

This document is the map of the repository. It describes every top-level directory and the rationale behind the split. If you are adding a new directory or a new module, check here first to confirm where it belongs.

## Top-level layout

```
maecly-print/
├── README.md                    # Landing page for github.com/<org>/maecly-print
├── LICENSE                      # GPLv3 (the canonical legal text)
├── CHANGELOG.md                 # Keep a Changelog format, updated atomically with releases
├── CONTRIBUTING.md              # Symlink or stub pointing to docs/08-development/contributing.md
├── CODE_OF_CONDUCT.md           # Contributor Covenant 2.1
├── SECURITY.md                  # Vulnerability disclosure policy
├── .gitignore                   # Tracks all build outputs to ignore
├── .gitattributes               # Line ending policy + binary file declarations
├── .editorconfig                # Cross-editor formatting baseline
├── .github/                     # GitHub-specific configuration
│   ├── workflows/
│   │   ├── ci.yml
│   │   ├── release.yml
│   │   └── nightly.yml
│   ├── ISSUE_TEMPLATE/
│   ├── PULL_REQUEST_TEMPLATE.md
│   ├── CODEOWNERS
│   └── dependabot.yml
├── docs/                        # ALL documentation
│   ├── README.md                # Doc tree index
│   ├── 01-vision/
│   ├── 02-research/
│   ├── 03-requirements/
│   ├── 04-architecture/
│   ├── 05-features/
│   ├── 06-roadmap/
│   ├── 07-build-and-release/
│   ├── 08-development/
│   ├── 09-legal/
│   └── adr/                     # Architecture Decision Records
├── src-tauri/                   # Rust backend (Tauri main process)
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── src/
│   ├── icons/
│   ├── resources/
│   └── tests/
├── src/                         # React + TypeScript frontend (Tauri webview content)
│   ├── App.tsx
│   ├── main.tsx
│   ├── index.html
│   ├── pages/
│   ├── components/
│   ├── hooks/
│   ├── i18n/
│   └── styles/
├── shell-ext-win/               # Windows shell extension (C++ COM DLL)
│   ├── ComServer.cpp
│   ├── dllmain.cpp
│   ├── MaeclyPrintExt.def
│   ├── ShellExt.h
│   └── project.vcxproj
├── shell-ext-mac/               # macOS Finder Sync Extension (Swift, M5+)
│   └── MaeclyPrintFinderExt/
│       ├── MaeclyPrintFinderExt.xcodeproj/
│       ├── FinderSync.swift
│       └── Info.plist
├── installers/
│   ├── windows/
│   │   ├── inno-setup.iss
│   │   ├── sparse-msix/
│   │   │   ├── AppxManifest.xml
│   │   │   └── Assets/
│   │   └── build.ps1
│   └── macos/
│       ├── pkgbuild.sh
│       ├── create-dmg.sh
│       ├── notarize.sh
│       ├── codesign.sh
│       ├── distribution.xml
│       └── resources/
├── scripts/
│   ├── dev.ps1                  # Local dev runner (Windows)
│   ├── dev.sh                   # Local dev runner (macOS)
│   ├── build-release.ps1
│   ├── build-mac-universal.sh
│   ├── update-third-party.ps1   # Update SumatraPDF / Pandoc / etc bundled binaries
│   ├── sign-windows-ev.ps1
│   ├── sign-nightly.ps1
│   ├── setup-apple-certs.sh
│   ├── upload-to-r2.sh
│   ├── update-manifest.sh
│   └── verify-windows-signatures.ps1
├── package.json                 # pnpm workspace root + frontend deps
├── pnpm-lock.yaml
├── pnpm-workspace.yaml
├── tsconfig.json                # Root TS config (extended by src/)
├── vite.config.ts
└── tailwind.config.ts
```

## `README.md`, `LICENSE`, `CHANGELOG.md`

Standard top-level metadata for any open source repo. GitHub renders `README.md` on the project page; `LICENSE` is detected by the licensing badge; `CHANGELOG.md` is the human-readable history (Keep a Changelog format).

`CONTRIBUTING.md` is a stub pointing readers to `docs/08-development/contributing.md`. We keep the stub at root because GitHub auto-detects it and links it from the "Contributing" sidebar on the repo page. The full content lives in `docs/` so it can be cross-linked with other developer docs.

## `.github/`

GitHub-specific configuration. Treated as code: changes go through PR review.

- `workflows/`: CI pipelines (see [ci-cd.md](../07-build-and-release/ci-cd.md)).
- `ISSUE_TEMPLATE/`: structured templates for bug reports, feature requests, security issues.
- `PULL_REQUEST_TEMPLATE.md`: required checklist when opening PRs.
- `CODEOWNERS`: maps file paths to maintainers for auto-requested review.
- `dependabot.yml`: weekly dep update PRs for `cargo`, `npm`, and `github-actions` ecosystems.

## `docs/`

All long-form documentation. Numbered prefix folders give a stable reading order. Top-level `docs/README.md` is the index.

The numbering convention:

| Prefix | Content |
|---|---|
| `01-vision/` | What this project is and is not |
| `02-research/` | Background research and prior art |
| `03-requirements/` | Functional + non-functional requirements |
| `04-architecture/` | High-level + per-component architecture |
| `05-features/` | Feature specs and user-facing behavior |
| `06-roadmap/` | Milestones, timeline, release strategy |
| `07-build-and-release/` | Installer + signing + CI/CD |
| `08-development/` | Contributor-facing docs (this directory) |
| `09-legal/` | License, third-party, trademark, privacy |
| `adr/` | Architecture Decision Records (date-prefixed, immutable once accepted) |

We chose numbered prefixes instead of topical names because contributors read the docs in roughly this order, and a numeric scan is easier than alphabetical guessing.

## `src-tauri/` — Rust backend

The Tauri main process and all Rust logic live here.

```
src-tauri/
├── Cargo.toml             # Crate manifest
├── Cargo.lock             # Committed (binary crate)
├── tauri.conf.json        # Tauri runtime config (windows, plugins, updater)
├── build.rs               # Build script (e.g., embed icons, generate version)
├── src/
│   ├── main.rs            # Entry point (~10 lines, defers to lib)
│   ├── lib.rs             # Tauri app builder, IPC registration
│   ├── ipc.rs             # All Tauri IPC command handlers (the API surface to JS)
│   ├── cli.rs             # CLI parsing with clap
│   ├── queue.rs           # Print job queue (Tokio)
│   ├── profiles.rs        # Profile load/save/validate
│   ├── updater.rs         # Auto-update logic (uses tauri-plugin-updater)
│   ├── shell_integration.rs  # Named pipe server for shell-ext handoff
│   ├── engine/            # Format-specific print pipeline
│   │   ├── mod.rs
│   │   ├── router.rs      # Dispatch input file → adapter
│   │   ├── pdf.rs         # SumatraPDF adapter
│   │   ├── office.rs      # LibreOffice adapter
│   │   ├── image.rs       # ImageMagick / direct GDI+ adapter
│   │   ├── html.rs        # Chromium / WebView2 adapter
│   │   └── markdown.rs    # Pandoc adapter
│   └── printer/           # Printer abstraction (one impl per OS)
│       ├── mod.rs         # Trait + factory
│       ├── windows.rs     # SumatraPDF + GDI+ printing
│       └── macos.rs       # CUPS via lpr/lp
├── icons/                 # App icons (multiple resolutions, .ico, .icns)
├── resources/             # Bundled binaries (SumatraPDF.exe), user-facing files
└── tests/                 # Integration tests (separate binary per file)
    ├── pdf_print.rs
    ├── profile_roundtrip.rs
    └── queue_concurrency.rs
```

### Why `src-tauri/` is a separate directory

- **Tauri convention.** `tauri init` creates this layout; deviating breaks `pnpm tauri dev`.
- **Separation of concerns.** The Rust backend has its own `Cargo.toml`, its own `target/`, its own dependency graph. It builds independently of the frontend.
- **Editor experience.** Rust tooling (rust-analyzer) finds `Cargo.toml` here and indexes correctly.

### Module organization: by feature, not by layer

Inside `src/`, modules are organized by the feature they serve, not by architectural layer. Wrong:

```
src/
├── handlers/    # All IPC handlers
├── services/    # All business logic
├── repositories/  # All data access
└── models/      # All structs
```

Right (what we do):

```
src/
├── ipc.rs       # IPC handlers (one entry point)
├── queue.rs     # Print queue (handler + state + persistence)
├── profiles.rs  # Profiles (handler + state + persistence)
└── engine/      # Print engine (router + adapters)
```

Each module owns its IPC handlers, its state, its persistence. This keeps related code adjacent and reduces cross-module coupling.

## `src/` — React + TypeScript frontend

The Tauri webview content (what users see). Built with Vite, served by Tauri at runtime.

```
src/
├── App.tsx              # Top-level component (router setup)
├── main.tsx             # Vite entry: mounts <App /> into #root
├── index.html           # Single HTML page (SPA model)
├── pages/               # One file per top-level page
│   ├── Home.tsx
│   ├── PrintJob.tsx
│   ├── Profiles.tsx
│   ├── Queue.tsx
│   ├── Settings.tsx
│   └── About.tsx
├── components/          # Reusable UI primitives
│   ├── Button.tsx
│   ├── ProgressBar.tsx
│   ├── ProfileCard.tsx
│   └── ...
├── hooks/               # Custom React hooks
│   ├── useProfiles.ts
│   ├── usePrintQueue.ts
│   └── useUpdater.ts
├── i18n/                # Translation files
│   ├── en.json
│   ├── es.json
│   └── i18n.ts          # i18next setup
├── styles/
│   ├── globals.css      # Tailwind import + base styles
│   └── tailwind.css
└── lib/                 # Frontend-only utilities
    ├── ipc.ts           # Typed wrappers around tauri.invoke()
    └── format.ts
```

### Why `src/` is a separate directory from `src-tauri/`

- **Different tooling.** TS/React uses Vite, ESLint, Prettier. Rust uses cargo, clippy, rustfmt. Mixing them confuses tooling and inflates IDE indexing time.
- **Bundle contract.** Tauri's `tauri.conf.json` declares `src/` (more precisely: the Vite output of `src/`) as the webview content. This is the canonical separation Tauri expects.
- **Mental model.** Frontend developers can reason about `src/` without knowing Rust; backend developers can reason about `src-tauri/` without knowing React.

## `shell-ext-win/` — Windows shell extension

A C++ COM DLL implementing `IExplorerCommand` (Win 11 modern shell) and `IShellExtInit` + `IContextMenu` (Win 10 fallback).

```
shell-ext-win/
├── ComServer.cpp           # COM class registration boilerplate
├── dllmain.cpp             # DllMain entry, DllGetClassObject, DllCanUnloadNow
├── MaeclyPrintExt.def      # Module definition (exports)
├── ShellExt.h              # Header for the extension classes
├── ShellExt.cpp            # IExplorerCommand + IContextMenu implementations
├── project.vcxproj         # Visual Studio project file
├── project.vcxproj.filters
└── README.md               # How to build this DLL specifically
```

### Why `shell-ext-win/` is at the top level (not under `src-tauri/`)

- **Different toolchain.** It builds with MSVC + Visual Studio project files, not cargo. Putting it under `src-tauri/` would imply it's part of the Cargo workspace, which it isn't.
- **Different lifecycle.** The shell extension changes infrequently after M2; segregating it makes the build script cleaner.
- **Future macOS counterpart.** `shell-ext-mac/` is a Swift Xcode project, also outside `src-tauri/`. The naming and location are symmetric.

### Why C++, not Rust

We considered using `windows-rs` (Rust crate for Windows APIs) for the shell extension. Rejected because:

- COM in-proc servers have strict requirements about thread affinity, DLL apartment models, and `DllMain` reentrancy that Rust's safety model fights.
- The C++ ecosystem has decades of working `IShellExtInit` / `IExplorerCommand` examples; the Rust ecosystem has very few.
- The DLL is small (~500 LOC), changes rarely, and is a stable interface boundary.

If a community contributor proves a clean Rust implementation, we'd happily switch. As of M2, C++ is the pragmatic choice.

## `shell-ext-mac/` — macOS Finder Sync Extension (M5+)

The Swift counterpart to `shell-ext-win/`. Lives at the top level for the same symmetry reasons.

```
shell-ext-mac/
└── MaeclyPrintFinderExt/
    ├── MaeclyPrintFinderExt.xcodeproj/
    ├── FinderSync.swift          # Main extension class
    ├── Info.plist
    ├── Assets.xcassets/
    └── Base.lproj/
```

Built with `xcodebuild`. Output bundled into `MaeclyPrint.app/Contents/PlugIns/MaeclyPrintFinderExt.appex/`.

## `installers/`

Installer build artifacts and scripts, separated by platform.

```
installers/
├── windows/
│   ├── inno-setup.iss              # Inno Setup script
│   ├── sparse-msix/                # Sparse MSIX manifest + assets
│   │   ├── AppxManifest.xml
│   │   └── Assets/
│   └── build.ps1                   # Builds setup.exe (calls iscc + MakeAppx)
└── macos/
    ├── pkgbuild.sh                 # Builds .pkg
    ├── create-dmg.sh               # Builds .dmg
    ├── notarize.sh                 # Submits to Apple notarization
    ├── codesign.sh                 # Codesigns .app and .appex
    ├── distribution.xml            # productbuild distribution definition
    └── resources/
        ├── welcome.html
        ├── conclusion.html
        ├── LICENSE.html
        ├── entitlements.plist
        ├── finder-ext-entitlements.plist
        ├── volume.icns
        └── dmg-bg.png
```

### Why `installers/` at top level (not in `src-tauri/`)

- **Multiple platforms.** Tauri's bundler covers basic cases but we need custom Inno Setup / pkgbuild flows. Having a dedicated `installers/` directory makes the customization explicit.
- **Independent versioning.** Installer scripts can be tweaked without touching the app code (e.g. updating the welcome page).
- **CI clarity.** The release pipeline reads from `installers/` directly; no path translation through Tauri's output structure.

## `scripts/`

Single-purpose shell / PowerShell scripts. Each script does one thing; the file name describes it.

| Script | Purpose |
|---|---|
| `dev.ps1` / `dev.sh` | Run `pnpm tauri dev` with the right env vars (e.g. `RUST_LOG=info`). |
| `build-release.ps1` | Wraps `pnpm tauri build` for Windows release. |
| `build-mac-universal.sh` | Builds both Mac arches and lipos them. |
| `update-third-party.ps1` | Re-downloads bundled binaries (SumatraPDF, etc.) and updates checksums. |
| `sign-windows-ev.ps1` | Signs all Windows binaries with EV cert. |
| `sign-nightly.ps1` | Signs with self-signed nightly cert. |
| `setup-apple-certs.sh` | Imports Apple certs into a temp keychain (CI). |
| `upload-to-r2.sh` | Pushes signed artifacts to Cloudflare R2. |
| `update-manifest.sh` | Updates a channel's `.json` manifest. |
| `verify-windows-signatures.ps1` | Asserts every shipped binary is signed. |

These scripts are checked into the repo because the build is part of the codebase. They are not "developer convenience" — they are how release builds happen.

## `package.json`, `pnpm-workspace.yaml`

Standard Node project files. We use pnpm (not npm or yarn) because:

- Faster installs.
- Stricter dependency resolution (no phantom dependencies).
- Workspace support (we have a single workspace today, but may split later).

The root `package.json` declares the frontend dependencies and scripts (`pnpm tauri dev`, `pnpm tauri build`, `pnpm test`, `pnpm lint`).

## `tsconfig.json`, `vite.config.ts`, `tailwind.config.ts`

Frontend tool configs at the root because Vite expects them there. These cover the `src/` directory only; they have nothing to do with Rust.

## `.gitignore`

Tracks all build outputs (`target/`, `dist/`, `node_modules/`, `.venv/`, `.DS_Store`, etc.). The convention: if `git status` would normally show it after a build, it belongs in `.gitignore`.

## `.gitattributes`

- Forces LF line endings for code files (`*.rs`, `*.ts`, `*.tsx`, `*.json`).
- Forces CRLF for Windows-specific files (`*.ps1`, `*.bat`, `*.iss` because Inno Setup expects CRLF).
- Marks binary files explicitly (`*.png`, `*.icns`, `*.pfx`).

## `.editorconfig`

Sets indent size (2 spaces for TS/JSON/YAML, 4 for Rust, tabs for Makefiles), trim trailing whitespace, ensure final newline. Covers what `.gitattributes` doesn't.

## What does **not** belong in this repo

- Compiled binaries (use Releases or CDN).
- Secrets (use GitHub Secrets or a secrets manager).
- Personal IDE configs (`.idea/`, `.vscode/settings.json`) — `.gitignore` excludes them. Shared IDE configs (e.g. `.vscode/extensions.json` recommending extensions) are OK.
- Generated docs (cargo doc output, TypeDoc output) — built on demand, not committed.
- Test fixtures larger than 1 MB — keep in a separate Git LFS-tracked path or pull from a CDN at test time.

## See also

- [coding-standards.md](./coding-standards.md)
- [local-setup.md](./local-setup.md)
- [contributing.md](./contributing.md)
- [../04-architecture/](../04-architecture/) — for the conceptual architecture that this repo structure implements
