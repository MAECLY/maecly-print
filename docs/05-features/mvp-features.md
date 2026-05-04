# MVP Features (v0.1.0)

> **TL;DR** — The minimum cut of MaeclyPrint that is genuinely useful: right-click → print PDFs and common images, with a small set of profiles, on Windows 10/11. No Office, no cloud, no fancy automation.

The MVP exists to **prove the core thesis**: that a free, open-source tool can integrate cleanly into the Windows context menu and print N files in batch without the friction of Adobe Reader or the price of Print Conductor. Everything else can wait.

## In scope

### 1. Native Windows 11 context menu integration

- **Submenu** appears in the **modern** context menu (not relegated to "Show more options").
- Items:
  - **Print with default profile** — silent, fastest path
  - **Configure print job…** — opens GUI with the selection pre-loaded
  - **Open MaeclyPrint** — launches the main GUI window
- Works for selections of any size (no 15-file limit).
- Works for files, folders (with recursion prompt), and mixed selections.
- Filters unsupported file types automatically and lists what was skipped.

Implementation: Sparse MSIX package + COM DLL implementing `IExplorerCommand`. Details in [shell-extensions.md](../04-architecture/shell-extensions.md) and [ADR-0005](../adr/0005-shell-extension-sparse-msix.md).

### 2. Format support

| Format | Engine | Notes |
|---|---|---|
| **PDF** (`.pdf`) | SumatraPDF direct | Native, no conversion |
| **JPG/JPEG, PNG** | ImageMagick → PDF → SumatraPDF | Standard image batch |
| **TIFF, BMP, WebP, HEIC** | ImageMagick → PDF → SumatraPDF | Less common but cheap to include |

That's it for MVP. Office and HTML wait for v0.2.

### 3. Print settings

The user can configure (per profile or per job override):

- **Printer** (system default or any installed printer)
- **Duplex**: none / long-edge / short-edge
- **Color mode**: color / monochrome
- **Copies**: 1–999
- **Page ranges**: e.g., `1-5,10` (PDFs only; images are single-page)
- **Paper size**: A4 / Letter / Legal / A3
- **Orientation**: portrait / landscape / auto
- **Scaling**: fit / shrink / none

Settings not exposed in MVP: tray, n-up, collate, watermark, separator pages, custom paper, margins, quality. These come in v0.2 / v1.

### 4. Profiles

- 3 built-in (immutable, can be cloned):
  - **Quick** — system default printer, color, simplex, fit, A4, portrait
  - **Duplex BW** — system default printer, monochrome, long-edge, fit, A4, portrait
  - **Photo Color** — system default printer, color, simplex, none scaling, A4, portrait, high quality
- User can create unlimited custom profiles.
- One profile is marked **Default** for context menu invocations.
- Profiles persist in `%APPDATA%\MaeclyPrint\profiles.toml` (atomic writes).

### 5. GUI

A single window built with React + Tailwind, embedded via Tauri. Pages:

- **Home** — drag-drop area, button to start a job, recent activity
- **Print Job** — pre-loaded with the selection, settings panel on the right, "Start printing" button
- **Profiles** — CRUD list of profiles
- **Queue** — current and recent jobs with status (pending / converting / spooling / printed / failed), per-job pause/resume/cancel, retry
- **About** — version, license, "Check for updates"

GUI MUST start in <500 ms cold from a context menu invocation.

### 6. CLI

The same binary, headless mode:

```
maeclyprint print --files <list> [--profile <name>] [--printer <name>] [...overrides]
maeclyprint printers list
maeclyprint profiles list
maeclyprint queue status
maeclyprint update check
```

`--help` for every subcommand. Stable exit codes (documented in [ipc-and-cli.md](../04-architecture/ipc-and-cli.md)).

### 7. Queue & lifecycle

- Persistent queue (`queue.json`) survives crashes and restarts.
- Concurrent execution: up to 2 jobs at a time (not configurable in MVP).
- Automatic retry: 3 attempts with exponential backoff (1s, 5s, 25s) before marking failed.
- Structured JSONL logs in `%LOCALAPPDATA%\MaeclyPrint\logs\`.

### 8. Notifications

- Native Windows toast on job complete and on first failure of a job.
- Click toast → opens Queue page with the job selected.
- Notifications can be globally muted in Settings (post-MVP allows per-profile mute).

### 9. Auto-update (basic)

- On startup (delayed 30 s) and at most once per 24 h, check `https://updates.maecly.com/maeclyprint/stable.json`.
- If newer version available: notification, manual trigger to install.
- Signature verification on download.
- Per-user install (no admin needed).

Channel switching (beta/nightly) deferred to v1; MVP is stable channel only.

### 10. Installer

- **Inno Setup** generates `MaeclyPrint-Setup-0.1.0-x64.exe` (~10 MB).
- Installs to `%LOCALAPPDATA%\Programs\MaeclyPrint`.
- Bundles SumatraPDF binary (~10 MB additional).
- Registers Sparse MSIX for shell extension.
- Creates Start Menu shortcut + optional desktop shortcut.
- Code-signed with EV cert from day 1 to avoid SmartScreen warnings.

### 11. Localization

- **English only** in MVP. Spanish lands in v1.

### 12. Documentation

- README.md, all `docs/` content, CONTRIBUTING.md, CODE_OF_CONDUCT.md, SECURITY.md, LICENSE all complete.

## Out of scope for MVP (explicitly NOT in v0.1.0)

| Feature | Defer to |
|---|---|
| Office (DOCX/XLSX/PPTX) | v0.2 |
| ODF (ODT/ODS/ODP) | v0.2 |
| HTML printing | v0.2 |
| Markdown printing | v0.2 |
| TXT/RTF | v0.2 |
| Tray selection | v0.2 |
| N-up | v1.0 |
| Custom paper sizes | v1.0 |
| Watermark | v1.0 |
| Separator pages | v1.0 |
| Profile import/export | v1.0 |
| Profile keyboard shortcuts | v1.0 |
| Multi-language UI (Spanish) | v1.0 |
| Update channels (beta/nightly) | v1.0 |
| macOS support | v1.1 (M5) |
| Hot folder / watch folder | v2.0 |
| Multi-printer load balancing | v2.0 |
| Email-to-print | v2.0 |
| Cloud folder watching | v2.0 |
| OCR | v2.0 |
| CAD support | v2.0 |
| Plugin API | v2.0 |

## Success criteria for MVP

The MVP is a success if, three months after release:

- A first-time user can install and complete a 50-file batch print in under 5 minutes.
- 95% of jobs in real-world use complete without errors (excluding driver/printer hardware failures).
- Cold context-menu invocation reliably under 500 ms p95.
- Crash rate under 0.5% of jobs (telemetry is opt-in; we measure via opt-in crash reporter and GitHub issues).
- At least 3 external contributors with merged PRs.
- A documentation site (`docs/`) that lets a new contributor set up a dev environment in under 30 minutes.

## What MVP is NOT

- Not a PDF editor.
- Not a virtual printer (we don't pretend to be a printer; we drive real printers).
- Not a cloud service.
- Not an Office replacement.
- Not a print-shop ERP.
- Not a notification daemon for non-print events.

We resist scope creep aggressively. Every feature added before v1.0 must serve the core thesis: **right-click N files, they print correctly, fast, and free.**
