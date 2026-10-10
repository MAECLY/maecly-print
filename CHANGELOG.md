# Changelog

All notable changes to Pluryprint (called MaeclyPrint until October 2026) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial repository scaffold: `package.json`, `pnpm-workspace.yaml`, `tsconfig.json`, `vite.config.ts`, `tailwind.config.ts`, ESLint 9 flat config, Prettier, Husky + lint-staged + commitlint.
- Tauri 2 backend skeleton in `src-tauri/` with utility modules (`paths`, `error`, `logging`, `settings`) and a stub IPC command (`app_info`) wired through tauri-specta.
- React + TypeScript frontend scaffold in `src/`: design tokens (CSS variables), `Brand`, `Icon`, and 15 UI primitives ported 1:1 from `claude design/maeclyprint/project/`.
- Six main screens (Home, PrintJob, Queue, Profiles, Settings, About) and onboarding flow rendered with mock data.
- GitHub Actions CI for `windows-latest`: lint, typecheck, vitest, cargo test, cargo clippy, tauri build (unsigned).
- Community files: `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, issue/PR templates, CODEOWNERS, dependabot.
- Printers: add a printer by network discovery or by IP address, installing a real spooler queue (IPP Everywhere via `lpadmin` on macOS/Linux; Microsoft IPP Class Driver on Windows, IPP/IPPS only).
- Printers: **From system** tab to choose which OS spooler queues MaeclyPrint shows, as a soft-disable preference that never modifies the underlying queue.
- Printers: missing-printer reconciliation — reassign, forget, or keep-paused references (routes, tray/paper overrides, default) to a printer the spooler no longer reports.
- Printers: organization-managed default printer via `policy.json`, which locks **Set default** in the UI and takes precedence over user preferences.
- Print Job screen: assemble a batch via the OS file picker or native drag & drop, with per-file inspection (type, size, page count), inline validation (copies, page ranges), and an honesty "resolved command" preview of the exact `lp` / SumatraPDF invocation; submit straight into the print queue.
- Persistent print queue with a single-writer dispatcher: the convert → print pipeline (PDF passthrough + image conversion), up to two concurrent conversions and one print at a time, automatic retry with backoff, per-job and whole-queue pause/resume/cancel, crash recovery, and a 200-job finished-history cap in `queue.json`.
- Queue screen wired to live `queue-updated` / `job-progress-event` events (List, Board, and Timeline views); Home "Recent activity" surfaces the last finished jobs.
- Print engine: image → PDF conversion for `jpg`/`jpeg`/`png`/`tif`/`tiff`/`bmp`/`webp` (pure-Rust `image` + `lopdf`) and PDF passthrough, spooled via CUPS `lp` (macOS/Linux) or SumatraPDF (Windows), with a duplex capability check and a shared page-range parser.
- Profiles: a persisted profile backend (`profiles.toml` for customs, three immutable built-ins in code, default id in `config.toml`) with a master-detail Profiles screen — create, clone, edit with inline validation, mark default, and delete with a 5-second **Undo** that restores the same id (NFR-032). Atomic writes; corrupt-file reads degrade to built-ins only.
- Settings: a persisted app-settings backend (`get_app_settings`/`set_app_settings`) and a Settings screen — appearance (3-way theme incl. System, density), default profile, engine limits (concurrent conversions, conversion timeout, max file size), update channel + check-on-startup, and consent-only telemetry switches. Engine limits are enforced for real: max file size at file inspection/submission; concurrency and timeouts reach the running queue on save, with no restart.
- About: an on-demand update **check** against the GitHub latest release (semver compare), honest MaeclyPrint/React/Tauri component versions with OS · arch, live filesystem paths with **Open logs folder**, and allow-listed external links (release notes, license, docs, report an issue, repository).

### Added — page-completeness campaign

Every screen was audited against its spec and completed end to end; the items below summarise what that campaign shipped.

- **Headless CLI** in the same binary (`maeclyprint print | printers | profiles | queue | update`, `--json`, frozen exit codes, `--invoke=context-menu --files-from`), driving the same cores as the GUI, with a console front end so Windows shells wait for the exit code (FR-008…FR-010).
- **Right-click integration**: a Windows Explorer `IExplorerCommand` handler (`shell-ext-win`, flat submenu with the default profile, up to eight profiles, Configure and Add to queue, labels from a localized `shell-menu.json` cache the app rewrites) and macOS Finder Services. A selection cut to the handoff cap says so (FR-005). The Windows installer does not register the handler yet.
- **Single instance**: a second launch forwards its selection into the open window; selections are drained one after another, never twice (NFR-014).
- **Folders**: dropping or right-clicking a folder asks whether to recurse, with honest counts and walk progress; skipped files are reported with a reason (FR-004/FR-005/FR-059).
- **More formats**: Office (DOCX, XLSX, PPTX, DOC, XLS, PPT), OpenDocument, TXT and RTF via LibreOffice; HTML via Edge/Chrome/Chromium with no network access; Markdown via Pandoc then the browser, with raw HTML and remote images disabled; HEIC/HEIF via the OS decoder. Converters run bounded, killable and without a shell, and page counts and page ranges apply to converted documents (FR-052…FR-057).
- **Dependency probe** (FR-058): missing SumatraPDF, LibreOffice, browser or Pandoc pauses the job with a message naming the program and how to install it; a SumatraPDF fetch script for the Windows bundle that pins the version and refuses to run until its SHA-256 is filled in.
- **Printing fidelity**: any paper size the printer reports (or **Printer default**), collate, portrait/landscape to CUPS, RGB colour only where the printer lists it, per-printer tray and paper defaults on real jobs, a warning when the printer cannot do the chosen sides or colour, and document-bounds checks on page ranges in the GUI and the CLI.
- **Queue**: `done` now means printed — each job is followed in the OS queue (CUPS, Windows spooler) until it leaves it; a separate **print timeout**; **Cancel all**; batches submitted held for review; job errors carry stable, translated reasons; a resumed job re-resolves a printer that is gone.
- **Profiles**: import/export as TOML (fresh ids, de-duplicated names, 1 MiB cap); profiles resolved by id or name everywhere (FR-024).
- **Updates**: a launch-time check 30 s after start, at most daily, with **Remind me later** and **Skip this version**; signed per-user install from About and the CLI, with a second confirmation for a major version (FR-080…FR-087).
- **Notifications**: native OS notifications for finished, failed and stuck batches while the window is in the background, with a switch in Settings (FR-072).
- **Settings**: Language (English/Español, FR-094), Start minimized, Native notifications, and a banner when `config.toml` cannot be read (saves are refused until it is fixed, FR-101).
- **i18n and accessibility**: the whole UI in English and Spanish with a typed key-parity test; IPC errors translated from their code and reason; accessible names, error wiring and real disabled states on form controls; keyboard shortcuts (NFR-034).
- **Windows**: mDNS discovery of IPP/IPPS printers through the DNS Client service; localized paper names resolved through driver paper ids.
- **Logging**: daily JSONL files with 10 MB rolls and 14-day / 7-file retention, panics and frontend errors forwarded to the log, and absolute paths kept out of the default log level (FR-113, NFR-081, NFR-082).
- **Brand**: the Abanico app icon, drawn from a generator, and the same mark inside the app.
- **Tests**: a browser E2E harness with a fake Tauri host (macOS), seven WebdriverIO specs, launch smokes in CI, opt-in real-spooler suites for a CUPS hold queue and a paused Windows queue, and a gate that every Tauri command is registered.

### Changed

- **Relicensed under AGPL-3.0-only; contributions now under the CLA.** Pluryprint moves from GPL-3.0-only to the GNU Affero General Public License v3.0, the model Versorium uses: contributions come in under [`CLA.md`](CLA.md) (the DCO sign-off check is gone), `TRADEMARKS.md` keeps the name and mark out of the licence under AGPL section 7(e), and [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) ships inside the app bundle with `LICENSE` and the licence texts in `LICENSES/`. About links the licence, the CLA and the notices. See [ADR-0010](docs/adr/0010-relicense-to-agpl-3.0.md).
- **Renamed to Pluryprint.** The product, binaries (`pluryprint`, `pluryprint-cli`), bundle identifier (`com.maecly.pluryprint`), data folders, log files, Finder service and Explorer verb ("Print with Pluryprint") and environment variables (`PLURYPRINT_*`) all carry the new name. An existing MaeclyPrint install upgrades in place: its settings, profiles, queue and logs move to the new folders on first launch (never overwriting), admin policies and `MAECLYPRINT_*` variables are still honoured as a fallback, and the Windows installer removes the old registration. See [ADR-0009](docs/adr/0009-rename-to-pluryprint.md). The repository URL is unchanged.
- Appearance (theme + density) now persists to `config.toml` as the source of truth, with a localStorage instant-boot cache; on divergence the backend value wins.
- The default print profile is now **Quick** (`builtin-quick`) rather than the earlier `Quick BW Duplex`; the default is stored as a profile id in `config.toml`'s `[engine].default_profile`.

### Changed — page-completeness campaign

- Windows ships only the per-user NSIS installer; the per-machine MSI is no longer built (ADR-0008).
- The repository identity is `github.com/MAECLY/maecly-print` everywhere (About links, update check, issue reports).
- `set_app_settings` never writes `engine.default_profile`, so a stale window cannot revert a default the CLI changed.

### Fixed

- Printers: **Remove** is hidden for virtual printers (e.g. Microsoft Print to PDF) so they can't be soft-disabled by mistake; **Restore** stays reachable.
- Docs: printers documentation no longer understates support — **Set default** and **Test page** have been live spooler actions, not local-only stubs.
- Context menu: **Print with “<profile>”** on a profile created after the window opened (for example with `maeclyprint profiles create`) printed with the default profile on the OS default printer. The app now re-reads profiles before resolving a selection, and a profile that matches nothing prints nothing: an error names it and the files open on the Print Job screen (FR-001).
- Security hardening from the campaign's review: CUPS option values that `lp` would split are refused, PowerShell arguments for Add-Printer travel as environment data, `open_external` parses URLs with a WHATWG parser before the allow-list, `policy.json` ownership is checked on the handle that is read, and the command preview is quoted so pasting it cannot run a file name.

### Notes

- M1 "Print Core": the desktop GUI enumerates real printers, prints every supported format end to end through the queue, and persists profiles and app settings; the CLI, SumatraPDF bundling and the signed per-user update install have since landed. Still open: no installer registers the Windows Explorer handler yet, no release workflow publishes a signed `latest.json`, and per-channel (beta/nightly) update feeds do not exist.
- Local prerequisites for running `pnpm tauri dev`: Rust stable 1.78+, Node 20 LTS, pnpm 9, Visual Studio Build Tools 2022 with C++ workload + Windows 11 SDK + ATL. See `docs/08-development/local-setup.md`.

## [0.0.1] - 2026-04-26

### Added

- Initial commit with `LICENSE` (GPLv3) and the documentation tree under `docs/`.
- Claude Design handoff bundle under `claude design/maeclyprint/`.
