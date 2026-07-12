# Milestones

> **TL;DR.** MaeclyPrint ships in six milestones (M0–M5) over roughly five to six months of single-developer work, taking the project from an empty repository to a notarized macOS port. Each milestone has explicit scope, deliverables, exit criteria, and named risks.

This document is the source of truth for what each milestone contains. Anything not listed here is out of scope for that milestone and should be deferred or filed against a later one. See [timeline.md](./timeline.md) for duration estimates and [release-strategy.md](./release-strategy.md) for how these milestones map to public versions.

## Overview

| Milestone | Theme | Public version | Status (2026-07-12) |
|---|---|---|---|
| M0 | Foundation | `0.0.1` (repo only) | Done |
| M1 | Print Core (Win) | `0.1.0-alpha` (internal) | In progress |
| M2 | Shell Integration (Win) | `0.1.0-beta` (public) | Planned |
| M3 | Multi-format (Win) | `0.2.0` | Planned |
| M4 | Auto-update + Polish (Win) | `1.0.0` GA | Planned |
| M5 | macOS Port | `1.1.0` | Planned |

Milestones are strictly sequential. We do not start M(n+1) work until M(n) exit criteria are met. Spillover items roll forward and reduce the next milestone's headroom.

---

## M0 — Foundation

**Goal:** A browsable, lintable, buildable repository that any contributor can clone and extend.

### Scope

- Repository scaffold: `src-tauri/`, `src/`, `shell-ext-win/`, `installers/`, `scripts/`, `.github/workflows/`.
- Tauri 2 project initialized with React + TypeScript template.
- Documentation tree under `docs/` populated for all sections (this milestone is largely about the docs you are reading right now).
- Basic CI: `cargo test`, `cargo clippy`, `pnpm lint`, `pnpm test`, build (no signing).
- Licensing: `LICENSE` (GPLv3), `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SECURITY.md`, `CONTRIBUTING.md`.
- `README.md` with badges, install instructions placeholder, status banner.

### Deliverables

| Item | Owner | Form |
|---|---|---|
| `cargo build` succeeds on `windows-latest` | Maintainer | CI green |
| `pnpm tauri dev` boots an empty window | Maintainer | Manual verify |
| All 18 architecture, roadmap, build, and dev docs landed | Maintainer | PR merged |
| `0.0.1` git tag pushed | Maintainer | GitHub Release (notes only) |

### Exit criteria

- Every CI job is green on `main`.
- A new contributor can clone, run `pnpm install && pnpm tauri dev`, and see the app shell within 10 minutes.
- All cross-document links resolve (no 404s in `docs/`).
- DCO bot active on PRs.

### Risks

| Risk | Mitigation |
|---|---|
| Tauri 2 toolchain churn | Pin exact minor versions in `Cargo.toml` and `package.json`; document override path. |
| Windows SDK version drift on contributor machines | `local-setup.md` pins Visual Studio Build Tools 2022 with required components listed by ID. |
| Doc-set goes stale immediately | Each PR template requires a "Docs updated?" checkbox. |

### Dependencies

None. M0 is the bootstrap.

---

## M1 — Print Core (Win)

**Goal:** Printing works from a CLI on Windows for PDF inputs, with a profiles system and a minimal GUI.

### Status (2026-07-12) — in progress

**Delivered:**

- The GUI **print core**: real printer enumeration, capabilities, discovery, add-by-IP/discovery, set-default, and test page (`ipc::printers` + `platform/{cups,windows}`), plus the convert → print pipeline for PDFs (passthrough) and raster images (pure-Rust `image` + `lopdf`), spooled via CUPS `lp` / SumatraPDF.
- A **persistent print queue**: the single-writer dispatcher (concurrency, retries with backoff, pause/resume/cancel, crash recovery), `queue.json`, and typed `queue-updated` / `job-progress-event` events — wired end-to-end to the Print Job and Queue screens and Home's recent activity.
- A **persisted profiles backend** (`profiles.toml` for customs, built-ins in code, default in `config.toml`): full CRUD with an editor, clone, delete-with-undo, and set-default (`ipc::profiles`, FR-020/021/022/026/027). This closes the profiles work that was open earlier in M1.
- **App settings persistence** (`get_app_settings`/`set_app_settings`) with engine limits enforced for real — file-size at submit/inspect, concurrency + conversion timeout at the next app start — plus theme/density dual-write and a default-profile picker.
- Settings / printer-preferences persistence and an org-managed default via `policy.json`.
- An **About** surface with the read-only **update check** (GitHub `releases/latest`), honest component versions, filesystem paths + open-logs, and allow-listed external links.

**Still open in M1:**

- The **CLI** (`maeclyprint print|queue|…`) is specified ([ipc-and-cli.md § 3](../04-architecture/ipc-and-cli.md#3-cli)) but not implemented — the binary runs the GUI only.
- **SumatraPDF bundling** is deferred: the app locates an existing `SumatraPDF.exe`; a missing one is a `DependencyMissing` that pauses the job.
- The update flow is **check-only**: it detects and links to a newer release but does not download or install — the in-app auto-updater is **M4**. Channel selection (beta/nightly) is persisted but not yet wired to separate feeds.
- Note: several scope lines below are framed Windows-first, but the enumeration/print backends are cross-platform and the CUPS path is exercised in CI.

Shell integration (the right-click menu) is **M2** and not yet started.

### Scope

- Rust backend modules: `engine::router`, `engine::pdf`, `printer::windows`, `queue`, `profiles`, `cli`.
- SumatraPDF integration: bundled binary in `src-tauri/resources/`, invoked via `Command` with structured args.
- CLI: `maeclyprint print <files…> --printer <name> --profile <id>` parsed with `clap`.
- TOML profiles: load from `%APPDATA%\MaeclyPrint\profiles\*.toml`, schema documented and versioned.
- GUI: bare-bones React pages for Profiles list, Queue view, Settings (printer picker).
- Print queue: Tokio task that processes jobs sequentially, reports progress over Tauri IPC.
- Logging: `tracing` with file rotation under `%LOCALAPPDATA%\MaeclyPrint\logs\`.

### Deliverables

| Item | Acceptance |
|---|---|
| Print 50 PDFs in one batch | Wall-clock < 5 min on Microsoft Print to PDF; zero crashes. |
| Profile switching from GUI | Selected profile persists across restart and is honored by next print job. |
| Log file output | One log per app session, JSON lines format. |
| `0.1.0-alpha` internal build | Unsigned, distributed via GitHub Releases marked as pre-release. |

### Exit criteria

- Integration test prints a known-good PDF to "Microsoft Print to PDF" and the output matches a fixture (byte-equal or visual diff < 0.5%).
- All public Rust APIs in `engine` and `printer` documented with rustdoc.
- No `panic!` reachable from any user-driven code path (verified by `cargo clippy -W clippy::panic`).

### Risks

| Risk | Mitigation |
|---|---|
| SumatraPDF CLI flag changes between versions | Pin exact SumatraPDF version in `scripts/update-third-party.ps1`; add smoke test that asserts known flags exist. |
| Win32 print spooler quirks (per-printer driver bugs) | Limit M1 to "Microsoft Print to PDF" and one physical printer model in CI; track other printers as known-issues for M3. |
| Tokio + Tauri IPC backpressure | Bound channel capacity; reject new jobs with a clear error rather than buffering unbounded. |

### Dependencies

- M0 complete.
- SumatraPDF binary licensing review (covered in [third-party-licenses.md](../09-legal/third-party-licenses.md)).

---

## M2 — Shell Integration (Win)

**Goal:** Right-click in File Explorer on a selection of files surfaces a "Print all with MaeclyPrint" submenu that hands the selection to the running app (or starts it).

### Scope

- C++ COM DLL in `shell-ext-win/` implementing `IExplorerCommand` (Win 11) and `IShellExtInit` + `IContextMenu` (Win 10 fallback).
- Sparse MSIX package in `installers/windows/sparse-msix/` to grant DLL identity for `IExplorerCommand` registration on Win 11.
- Inno Setup installer (`installers/windows/inno-setup.iss`) that:
  - Installs to `%LOCALAPPDATA%\Programs\MaeclyPrint`.
  - Registers `HKCU\Software\Classes\` entries for the Win 10 fallback path.
  - Calls `Add-AppxPackage` to deploy the sparse MSIX during install, `Remove-AppxPackage` on uninstall.
- IPC handoff: shell extension serializes selection to JSON and hands off to running app via named pipe (or starts app with `--enqueue` CLI arg).
- Submenu items: "Print all", "Print all with profile…", "Open MaeclyPrint…".

### Deliverables

| Item | Acceptance |
|---|---|
| Right-click on 30 mixed files, click "Print all" | All 30 PDFs print successfully; non-PDFs are listed as skipped (multi-format is M3). |
| Submenu appears in Win 11 File Explorer (modern UI) | Verified on clean Win 11 23H2 VM. |
| Submenu appears in Win 10 File Explorer | Verified on clean Win 10 22H2 VM. |
| Installer + uninstaller round-trip | After install → use → uninstall, no MaeclyPrint files or registry entries remain (audited with Sysinternals Process Monitor). |
| `0.1.0-beta` public release | Signed with self-signed cert (warnings expected); SmartScreen reputation N/A yet. |

### Exit criteria

- Shell extension passes Microsoft's [Shell Extension Best Practices](https://learn.microsoft.com/windows/win32/shell/shell-extension-handlers) audit (no UI on background threads, no `MessageBox` from handler, no blocking I/O > 50 ms).
- Installer is reproducible: building twice from the same commit produces byte-identical `.exe`.
- Uninstaller deregisters MSIX, removes all registry entries, deletes install directory and `%LOCALAPPDATA%\MaeclyPrint`.

### Risks

| Risk | Mitigation |
|---|---|
| Sparse MSIX requires identity that conflicts with non-MSIX install on some Win 10 builds | Detect Win version at install time; only deploy sparse MSIX on Win 11. |
| File Explorer caches shell extension DLLs | Installer kills `explorer.exe` and restarts it (with user consent) or instructs user to sign out. |
| Selection size limit (~15 files) re-imposed by Explorer | Verified workaround via verb-based registration that bypasses `IDataObject` row limits. |

### Dependencies

- M1 complete (the app must actually print before adding a way to invoke it).

---

## M3 — Multi-format (Win)

**Goal:** Office documents, web pages, Markdown, and images print transparently through format-specific adapters.

### Scope

- `engine::router` extended with format detection (extension + magic bytes).
- Adapters:
  - `engine::office` — LibreOffice headless invocation (`soffice --headless --convert-to pdf`).
  - `engine::html` — Chromium headless via `chrome --headless --print-to-pdf` (or Edge WebView2 fallback).
  - `engine::markdown` — Pandoc subprocess.
  - `engine::image` — ImageMagick `convert` for resampling, then Windows GDI+ direct print (no PDF round-trip for single images).
- Optional preprocessing: `qpdf` for PDF normalization (linearize, decrypt with empty password if applicable).
- Dependency detection at install/runtime: "LibreOffice not found, Office files will be skipped" warning.
- Profile field: per-format adapter selection (e.g. force Pandoc instead of LibreOffice for `.docx`).

### Deliverables

| Item | Acceptance |
|---|---|
| Print mixed batch: PDF, DOCX, XLSX, PNG, MD, HTML | Each prints with correct page count and orientation. |
| Missing dependencies report | UI shows green/yellow/red status per adapter on Settings page. |
| `0.2.0` public release | Signed with OV cert (SmartScreen warnings persist until reputation accumulates). |

### Exit criteria

- Integration test matrix covers PDF, DOCX, XLSX, PPTX, ODT, PNG, JPG, MD, HTML, TXT — all printing successfully to "Microsoft Print to PDF" in CI.
- Adapter timeouts enforced (default 60s/file) with clean cancellation.
- No adapter spawns more than one subprocess concurrently per file (resource discipline).

### Risks

| Risk | Mitigation |
|---|---|
| LibreOffice headless instability when invoked rapidly | Serialize Office conversions; use single LO instance with `--accept` socket. |
| Chromium version drift breaking `--print-to-pdf` flags | Pin to system Edge WebView2 (preinstalled on Win 11) as primary; Chrome as fallback only. |
| Pandoc engine variance (LaTeX vs wkhtmltopdf) | Default to `--pdf-engine=wkhtmltopdf`; document switch in profiles. |

### Dependencies

- M2 complete (multi-format on the CLI is unhelpful without the right-click entry point).

---

## M4 — Auto-update + Polish (Win)

**Goal:** A signed, reputation-bearing 1.0.0 that updates itself silently, supports English and Spanish, and is ready for general public consumption.

### Scope

- Tauri updater wired up: 24h periodic check, on-demand "Check for updates" in Settings.
- Update manifest server: static JSON at `updates.maecly.com/maeclyprint/{channel}.json` (S3 or Cloudflare R2 origin, CloudFront/CF in front).
- EV code signing certificate procured and integrated into CI (key in HSM-backed GitHub Actions runner or Azure Key Vault).
- SmartScreen reputation seed: submit installer to Microsoft Defender SmartScreen analytics endpoint via Partner Center.
- i18n: full English + Spanish translations using `react-i18next` for frontend and `fluent-rs` for backend strings.
- Polish: empty states, error toasts, progress animations, accessibility audit (keyboard nav, screen reader labels, color contrast WCAG AA).
- Auto-updater install path completing the About screen's **check** (shipped in M1): download + verify + relaunch, and beta/nightly channel feeds. The About page itself (version, license, credits, update *check*) already landed in M1; M4 adds the installer and i18n/polish.

### Deliverables

| Item | Acceptance |
|---|---|
| In-app update from `0.9.x` → `1.0.0` succeeds | No user prompts beyond initial UAC for the new binary; old binary cleaned up. |
| EV signature on all installer + updater binaries | `signtool verify /pa /v setup.exe` passes; SmartScreen shows no warning on download. |
| Spanish UI verified end-to-end | Native Spanish reviewer signs off; no truncated strings; date/number formats locale-aware. |
| `1.0.0` GA release | Public announcement, GitHub Release with binaries, update manifest live. |

### Exit criteria

- Auto-update e2e test passes in CI (download → verify signature → replace binary → restart → version check).
- Translation completeness: 100% of user-facing strings in both `en.json` and `es.json`, validated by `i18next-parser`.
- Accessibility: zero `axe-core` violations on every page; keyboard reachability tested manually for all primary flows.

### Risks

| Risk | Mitigation |
|---|---|
| EV cert procurement delay (often 2-4 weeks for vetting) | Order cert at start of M3; have OV fallback in hand to keep cadence. |
| SmartScreen still warns despite EV (rare but happens) | Submit installer to MS for manual review via Partner Center; can take 1-3 weeks. |
| Translation review gates the release | Recruit Spanish reviewer at M3 start; budget 1 week for review + revision cycle. |

### Dependencies

- M3 complete.
- EV certificate purchased and provisioned (~$300/year, 1-3 week procurement).
- DNS for `updates.maecly.com` live with TLS certificate (Let's Encrypt or CDN-managed).

---

## M5 — macOS Port

**Goal:** A notarized, signed macOS application with a Finder Sync Extension or Quick Action that mirrors the Windows shell integration UX.

### Scope

- New crate target: `printer::macos` using `lpr` / `lp` (CUPS) via `Command`.
- Adapter parity: LibreOffice (cask install), `cups-pdf` for HTML print, Pandoc, ImageMagick — all via Homebrew or bundled.
- `shell-ext-mac/` Swift project: Finder Sync Extension targeting macOS 13+ (Apple Silicon and Intel via Universal binary).
- Alternative path evaluated: macOS Quick Action (Automator-based) as fallback if Finder Sync Extension proves too constrained.
- Installer: `pkgbuild` + `productbuild` for `.pkg`, `create-dmg` for `.dmg` with drag-to-Applications layout.
- Codesign with Apple Developer ID Application certificate.
- Notarization with `xcrun notarytool` and stapling with `xcrun stapler`.
- Universal binary built with `lipo` from `aarch64-apple-darwin` and `x86_64-apple-darwin` slices.
- Auto-updater extended: Tauri updater already supports darwin-aarch64 and darwin-x86_64 platform keys.

### Deliverables

| Item | Acceptance |
|---|---|
| `MaeclyPrint.app` runs on macOS 13 Ventura, 14 Sonoma, 15 Sequoia | First-launch Gatekeeper allows execution without right-click bypass. |
| Right-click in Finder → "Print all with MaeclyPrint" | Selection passed to app, prints via CUPS. |
| `.pkg` and `.dmg` both notarized and stapled | `spctl --assess --verbose=4` reports "accepted source=Notarized Developer ID". |
| `1.1.0` macOS GA release | GitHub Release includes Win + Mac assets; update manifest covers both. |

### Exit criteria

- Notarization passes on every release build (no warnings, all entitlements minimal).
- Finder Sync Extension survives Finder restart and macOS update without re-registration.
- All Rust + frontend tests pass on `macos-14` and `macos-13` runners.

### Risks

| Risk | Mitigation |
|---|---|
| Apple notarization failures (entitlement issues, hardened runtime exceptions) | Prototype the codesign + notarize pipeline at M5 week 1 with a stub binary; do not wait until release. |
| Finder Sync Extension developer experience is poor and undocumented in places | Spike Quick Action approach in parallel; pick the better one by week 3 of M5. |
| CUPS subtleties (printer-specific PPDs) | Limit M5 launch to "Generic PostScript" and "Generic PCL" PPD families; document known-good printers; defer exotic models. |
| Universal binary inflates download size | Acceptable for v1.1; investigate per-arch downloads for v1.2. |

### Dependencies

- M4 complete (auto-updater infrastructure must exist before macOS uses it).
- Apple Developer Program membership active ($99/year).
- Access to one Apple Silicon and one Intel Mac for local QA (CI handles both).

---

## What is explicitly out of scope (pre-1.0)

These are deferred to post-1.0 to keep the milestone path tight:

- Linux port (CUPS exists; tracked for v2.0).
- Cloud printing / IPP-over-HTTPS (would expand attack surface; not justified yet).
- Plugin/extension API for third-party adapters.
- Per-page settings within a single document (we only do per-job and per-profile).
- OCR of scanned PDFs.
- Batch resizing/rotating beyond what ImageMagick adapter does for image inputs.
- Cloud sync of profiles (privacy posture is "no telemetry"; opt-in cloud sync would be M6+).

If a feature request matches one of the above, label it `out-of-scope/pre-1.0` and link to this section.
