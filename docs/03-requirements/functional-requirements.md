# Functional Requirements

> **TL;DR** — Numbered, testable statements of what MaeclyPrint must do, grouped by capability area, each tagged with priority (`MUST`/`SHOULD`/`MAY`) and MVP inclusion (yes/no).

This document is the canonical list of functional requirements (FRs) for MaeclyPrint. Each requirement has a stable ID (`FR-XXX`) used in commit messages, PR descriptions, and test names. Non-functional requirements (performance, security, accessibility, i18n) live in [non-functional-requirements.md](./non-functional-requirements.md).

## Conventions

- **Priority** uses RFC 2119 keywords: `MUST` (mandatory), `SHOULD` (recommended), `MAY` (optional).
- **MVP** is `yes` if the requirement must ship in v0.1.0, `no` otherwise.
- Requirements may be revised between drafts but their ID is permanent. Withdrawn requirements remain in the file marked `[WITHDRAWN]` with a pointer to the replacement.

---

## 1. Selection & Invocation

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-001 | The user MUST be able to invoke MaeclyPrint by selecting one or more files in Windows Explorer and choosing **Print with MaeclyPrint** from the right-click context menu. | MUST | yes |
| FR-002 | The context menu entry MUST appear in the **modern Windows 11 menu** (not relegated to "Show more options"). | MUST | yes |
| FR-003 | The system MUST accept selections of **any size** (no 15-file limit). The shell extension SHALL receive the full `IShellItemArray` regardless of count. | MUST | yes |
| FR-004 | The system MUST accept folder selections; when a folder is right-clicked, the user MUST be offered the option to recurse or stay top-level. | MUST | yes |
| FR-005 | The system MUST filter out files whose extension is not in the supported list, and warn the user before discarding them. | MUST | yes |
| FR-006 | The user MUST be able to launch the GUI via Start menu, desktop shortcut, or `maeclyprint` command. | MUST | yes |
| FR-007 | The user MUST be able to drag and drop files/folders onto the GUI window to enqueue them for printing. | SHOULD | yes |
| FR-008 | The user MAY invoke MaeclyPrint headlessly via CLI: `maeclyprint print --files <list> [--profile <name>] [--printer <name>] [...settings]`. | MUST | yes |
| FR-009 | The CLI MUST return non-zero exit codes on failure with stable, documented codes. See [ipc-and-cli.md](../04-architecture/ipc-and-cli.md). | MUST | yes |
| FR-010 | The CLI MUST support `--json` mode that emits machine-readable status to stdout. | SHOULD | no |
| FR-011 | The system MAY accept selections via Windows "Send to" menu as a fallback when the modern menu is unavailable. | MAY | no |

## 2. Profile Management

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-020 | The user MUST be able to create, read, update, and delete print profiles. | MUST | yes |
| FR-021 | The system MUST ship with **3 built-in profiles** that cannot be deleted but can be cloned: `Quick`, `Duplex BW`, `Photo Color`. | MUST | yes |
| FR-022 | The user MUST be able to mark exactly one profile as **default** for context menu invocations. | MUST | yes |
| FR-023 | A profile MUST persist all supported print settings (printer, duplex, color, copies, paper, orientation, scaling, page ranges if applicable). | MUST | yes |
| FR-024 | The user SHOULD be able to import and export profiles as TOML files for sharing across machines. | SHOULD | no |
| FR-025 | The user MAY assign a profile-specific keyboard shortcut from the GUI (e.g., `Ctrl+Shift+1` invokes "Quick" profile on selected files). | MAY | no |
| FR-026 | Profile changes MUST be saved atomically; an interrupted save MUST NOT corrupt the profile store. | MUST | yes |
| FR-027 | The system MUST support an unlimited number of user-defined profiles (no artificial cap). | MUST | yes |

## 3. Print Job Lifecycle

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-030 | The system MUST maintain a queue of pending and active print jobs persisted to disk so that a crash does not lose unprinted work. | MUST | yes |
| FR-031 | The user MUST be able to view queue status: pending, converting, spooling, printed, failed. | MUST | yes |
| FR-032 | The user MUST be able to **pause**, **resume**, and **cancel** individual jobs. | MUST | yes |
| FR-033 | The user MUST be able to **pause/resume the entire queue**. | MUST | yes |
| FR-034 | Failed jobs MUST be retried automatically up to **3 times** with exponential backoff before being marked permanently failed. | MUST | yes |
| FR-035 | The system MUST log every job (start time, end time, file path, profile used, printer, page count, status, error if any) in a structured format. | MUST | yes |
| FR-036 | The user MUST be able to view and clear the job history from the GUI. | SHOULD | yes |
| FR-037 | The system SHOULD process jobs concurrently up to a configurable limit (default: 2 concurrent jobs to avoid printer queue contention). | SHOULD | no |
| FR-038 | The system MAY support reordering pending jobs in the queue via drag and drop. | MAY | no |

## 4. Settings

See [print-settings.md](./print-settings.md) for the full settings matrix.

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-040 | The user MUST be able to override profile settings on a per-invocation basis without modifying the saved profile. | MUST | yes |
| FR-041 | The "Configure print job" GUI MUST display all overridable settings with the profile's current values pre-filled. | MUST | yes |
| FR-042 | The system MUST validate setting values before submitting (e.g., page ranges must parse, copies must be ≥1). | MUST | yes |
| FR-043 | Invalid settings MUST surface inline error messages, not generic dialog boxes. | SHOULD | yes |
| FR-044 | The system MUST list available printers from the OS spooler dynamically; no hard-coded printer names. | MUST | yes |
| FR-045 | The system SHOULD remember the last-used printer per profile if the profile's `printer = "<default>"` placeholder is used. | SHOULD | no |

## 5. Format Support

See [supported-formats.md](./supported-formats.md) for the full per-format matrix.

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-050 | The system MUST print **PDF** files natively (no conversion). | MUST | yes |
| FR-051 | The system MUST print common raster images: **JPG, PNG**. | MUST | yes |
| FR-052 | The system SHOULD print additional images: **TIFF, BMP, WebP, HEIC**. | SHOULD | yes |
| FR-053 | The system SHOULD print **DOCX, XLSX, PPTX** via LibreOffice headless conversion. | SHOULD | no |
| FR-054 | The system SHOULD print **ODT, ODS, ODP** via LibreOffice. | SHOULD | no |
| FR-055 | The system SHOULD print **HTML** via Chromium headless `--print-to-pdf`. | SHOULD | no |
| FR-056 | The system SHOULD print **Markdown** via Pandoc → PDF pipeline. | SHOULD | no |
| FR-057 | The system SHOULD print **TXT** and **RTF** via LibreOffice. | SHOULD | no |
| FR-058 | The system MUST detect missing optional dependencies (LibreOffice, Chromium, Pandoc) at startup and provide a clear remediation message in the UI ("LibreOffice is not installed — Office files will be skipped"). | MUST | no |
| FR-059 | The system MUST NOT silently skip files; every unsupported file MUST be reported in the job log with reason. | MUST | yes |
| FR-060 | The system MAY support additional formats via a plugin API in v2+ (CAD, Visio, EPUB, email). | MAY | no |

## 6. Notifications

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-070 | The system MUST surface job completion via a non-modal toast notification ("12 of 12 files printed successfully"). | MUST | yes |
| FR-071 | The system MUST surface job failures with actionable error text and a link to the job log entry. | MUST | yes |
| FR-072 | The system SHOULD use native OS notifications (Windows Toast, macOS UNNotification) when available. | SHOULD | yes |
| FR-073 | The system MUST allow disabling notifications globally and per-profile. | SHOULD | no |
| FR-074 | The system MAY support custom notification sounds. | MAY | no |

## 7. Updates

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-080 | The system MUST check for updates on startup (delayed by 30 seconds) and at most once per 24 hours. | MUST | yes |
| FR-081 | The system MUST allow the user to manually trigger an update check from the **About** dialog ("Check for updates now"). | MUST | yes |
| FR-082 | The system MUST verify the cryptographic signature of any downloaded update before applying it. Updates with invalid signatures MUST be rejected. | MUST | yes |
| FR-083 | The system MUST install updates without requiring administrator privileges (per-user install). | MUST | yes |
| FR-084 | The user MUST be able to **defer**, **install now**, or **skip this version** when an update is available. | MUST | yes |
| FR-085 | The system SHOULD support release channels: **stable**, **beta**, **nightly**. Default channel is `stable`. | SHOULD | no |
| FR-086 | The user MUST be able to switch channels from Settings; switching to a lower channel MUST require explicit confirmation. | MUST | no |
| FR-087 | The system MUST never auto-install a major version bump (e.g., 1.x → 2.x); these require explicit user opt-in. | MUST | no |

## 8. Internationalization

See [non-functional-requirements.md](./non-functional-requirements.md#internationalization) for non-functional aspects.

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-090 | The GUI MUST be available in **English** in MVP. | MUST | yes |
| FR-091 | The GUI SHOULD be available in **Spanish** by v1.0. | SHOULD | no |
| FR-092 | All user-facing strings MUST be externalized in JSON resource files; no hard-coded strings in components. | MUST | yes |
| FR-093 | The system MUST detect the OS locale on first run and select the matching language if available, falling back to English. | MUST | no |
| FR-094 | The user MUST be able to override the language from Settings. | MUST | no |

## 9. Configuration & Persistence

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-100 | The system MUST persist configuration in human-readable TOML files at OS-appropriate locations (`%APPDATA%\MaeclyPrint\` on Windows, `~/Library/Application Support/MaeclyPrint/` on macOS). | MUST | yes |
| FR-101 | The system MUST gracefully handle a missing or corrupt config file by falling back to defaults and surfacing a recoverable error. | MUST | yes |
| FR-102 | All config writes MUST be atomic (write to temp + rename). | MUST | yes |
| FR-103 | The system MUST embed a `schema_version` field in every persisted file and migrate old schemas forward when loading. | MUST | yes |

## 10. Help & Discoverability

| ID | Requirement | Priority | MVP |
|---|---|---|---|
| FR-110 | The GUI MUST include an **About** dialog showing version, license link, and a link to the documentation site. | MUST | yes |
| FR-111 | The GUI MUST include a **First Run** wizard that explains the context menu integration and creates a desktop shortcut on opt-in. | SHOULD | yes |
| FR-112 | The CLI MUST support `--help` for every subcommand. | MUST | yes |
| FR-113 | The system MUST log to a user-accessible file (`%LOCALAPPDATA%\MaeclyPrint\logs\app.log`) with rotation. | MUST | yes |

---

## Traceability

Every functional requirement above SHOULD be referenced by:

- At least one **acceptance test** in the test suite (`tests/fr_XXX_*.rs` or `tests/fr_XXX_*.spec.ts`).
- At least one **issue or PR** that introduces or modifies the corresponding code path.
- A row in the **release checklist** for manual QA verification when relevant.

Requirements without coverage at release time MUST be either implemented, withdrawn (`[WITHDRAWN]` tag), or deferred (move `MVP: yes` → `no`).
