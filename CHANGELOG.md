# Changelog

All notable changes to MaeclyPrint will be documented in this file.

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

### Fixed

- Printers: **Remove** is hidden for virtual printers (e.g. Microsoft Print to PDF) so they can't be soft-disabled by mistake; **Restore** stays reachable.
- Docs: printers documentation no longer understates support — **Set default** and **Test page** have been live spooler actions, not local-only stubs.

### Notes

- M1 "Print Core" is now in progress: the desktop GUI enumerates real printers and prints PDFs and images end-to-end through the queue. Still open in M1 — the CLI (specified, not implemented), SumatraPDF bundling (an existing binary is located; missing → job paused), and a persisted profiles backend (profiles are frontend-local presets today).
- Local prerequisites for running `pnpm tauri dev`: Rust stable 1.78+, Node 20 LTS, pnpm 9, Visual Studio Build Tools 2022 with C++ workload + Windows 11 SDK + ATL. See `docs/08-development/local-setup.md`.

## [0.0.1] - 2026-04-26

### Added

- Initial commit with `LICENSE` (GPLv3) and the documentation tree under `docs/`.
- Claude Design handoff bundle under `claude design/maeclyprint/`.
