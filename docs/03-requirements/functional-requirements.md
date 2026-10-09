# Functional Requirements

> **TL;DR** — Numbered, testable statements of what MaeclyPrint must do, grouped by capability area, each tagged with priority (`MUST`/`SHOULD`/`MAY`), MVP inclusion (yes/no), and **implementation status verified against the source** (Done / Partial / Open).

This document is the canonical list of functional requirements (FRs) for MaeclyPrint. Each requirement has a stable ID (`FR-XXX`) used in commit messages, PR descriptions, and test names. Non-functional requirements (performance, security, accessibility, i18n) live in [non-functional-requirements.md](./non-functional-requirements.md).

## Conventions

- **Priority** uses RFC 2119 keywords: `MUST` (mandatory), `SHOULD` (recommended), `MAY` (optional).
- **MVP** is `yes` if the requirement must ship in v0.1.0, `no` otherwise.
- **Status** is what the code does *today*, not what is planned. Three values only:
  - **Done** — implemented and reachable by a user, with test coverage.
  - **Partial** — some of it works; the row says which part does not. A requirement whose machinery is written but which a user cannot actually reach is Partial, not Done.
  - **Open** — not implemented.
- Requirements may be revised between drafts but their ID is permanent. Withdrawn requirements remain in the file marked `[WITHDRAWN]` with a pointer to the replacement.

> **Status audit: 2026-07-20.** Every row below was checked against the source, not against another document. Where a doc and the code disagreed, the code won. Milestone-level status lives in [milestones.md](../06-roadmap/milestones.md); this table and that page are the only two places status is tracked.
>
> One theme dominates the Partial rows: **there is no installer**. `installers/` is empty, so nothing registers the Sparse MSIX, the shell extension, or a shortcut. Several requirements are fully implemented in code and still unreachable for a user because of it.

---

## 1. Selection & Invocation

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-001 | The user MUST be able to invoke MaeclyPrint by selecting one or more files in Windows Explorer and choosing **Print with MaeclyPrint** from the right-click context menu. | MUST | yes | **Partial** — the `IExplorerCommand` handler and the handoff into the app are implemented and tested (`shell-ext-win/`, `cli::handoff`), and the macOS `NSServices` equivalent works. But no Sparse MSIX or installer registers the handler, so on a normal machine the entry does not appear. |
| FR-002 | The context menu entry MUST appear in the **modern Windows 11 menu** (not relegated to "Show more options"). | MUST | yes | **Partial** — `IExplorerCommand` is the right interface for the modern menu, but it needs package identity to be registered. Blocked on the same MSIX. |
| FR-003 | The system MUST accept selections of **any size** (no 15-file limit). The shell extension SHALL receive the full `IShellItemArray` regardless of count. | MUST | yes | **Done** — the handler enumerates the whole array with no cap, and a selection too large for argv (32,767 chars on Windows) travels in a JSON handoff file. Round-trip tested. |
| FR-004 | The system MUST accept folder selections; when a folder is right-clicked, the user MUST be offered the option to recurse or stay top-level. | MUST | yes | **Done** — folders always qualify in the handler; `inspect_files` reports whether a folder has subfolders, the screen asks recurse-or-top-level only when there is something to recurse into, and `expand_folders` walks it with the answer. |
| FR-005 | The system MUST filter out files whose extension is not in the supported list, and warn the user before discarding them. | MUST | yes | **Done** — filtered against `SUPPORTED_EXTS`; discarded files are returned with a reason and surfaced in the UI, never dropped silently. |
| FR-006 | The user MUST be able to launch the GUI via Start menu, desktop shortcut, or `maeclyprint` command. | MUST | yes | **Partial** — running the binary with no subcommand opens the GUI. No installer exists, so there is no Start-menu entry and no desktop shortcut, and nothing puts `maeclyprint` on `PATH`. |
| FR-007 | The user MUST be able to drag and drop files/folders onto the GUI window to enqueue them for printing. | SHOULD | yes | **Done** — files and folders both, folders through the same recurse prompt as FR-004. |
| FR-008 | The user MAY invoke MaeclyPrint headlessly via CLI: `maeclyprint print --files <list> [--profile <name>] [--printer <name>] [...settings]`. | MUST | yes | **Done** — `clap` surface in `cli::args`, five subcommand roots (`print`, `printers`, `profiles`, `queue`, `update`), no window created in headless mode. |
| FR-009 | The CLI MUST return non-zero exit codes on failure with stable, documented codes. See [ipc-and-cli.md](../04-architecture/ipc-and-cli.md). | MUST | yes | **Done** — `cli::exit::ExitCode` maps every `IpcError` variant to a stable code; pinned by `tests/cli_contract.rs`. `queue cancel` / `cancel-all` fail (code 1, nothing written) while the app is running instead of reporting a cancel that would not happen. On Windows, shells only wait for (and read the code of) the console-subsystem `maeclyprint-cli.exe` (`tests/cli_console_binary.rs`); the GUI-subsystem `maeclyprint.exe` needs `start /wait` / `Start-Process -Wait`. |
| FR-010 | The CLI MUST support `--json` mode that emits machine-readable status to stdout. | SHOULD | no | **Done**, ahead of MVP — global `--json`, machine-readable on stdout, errors as JSON on stderr. |
| FR-011 | The system MAY accept selections via Windows "Send to" menu as a fallback when the modern menu is unavailable. | MAY | no | **Open** — nothing registers a "Send to" target. |

## 2. Profile Management

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-020 | The user MUST be able to create, read, update, and delete print profiles. | MUST | yes | **Done** — `ipc::profiles` CRUD behind a Profiles screen with an editor, clone and delete-with-undo. |
| FR-021 | The system MUST ship with **3 built-in profiles** that cannot be deleted but can be cloned: `Quick`, `Duplex BW`, `Photo Color`. | MUST | yes | **Done** — built-ins live in code, are rejected for deletion, and clone into customs. |
| FR-022 | The user MUST be able to mark exactly one profile as **default** for context menu invocations. | MUST | yes | **Done** — the default id lives in `config.toml`; `set_default_profile` switches it. |
| FR-023 | A profile MUST persist all supported print settings (printer, duplex, color, copies, paper, orientation, scaling, page ranges if applicable). | MUST | yes | **Done** for every setting listed. A profile *also* carries `tray` and `quality`; both persist, are editable, and travel with every job (`toJobSettings`): CUPS applies both, SumatraPDF applies the tray and notes on the job that it cannot apply the quality. The printer is stored as the printer id (queue name), not its display name. |
| FR-024 | The user SHOULD be able to import and export profiles as TOML files for sharing across machines. | SHOULD | no | **Done**, ahead of plan (was v0.3) — `export_profile` / `import_profiles`, wired to the Profiles screen. Not available from the CLI, and no drag-drop import. |
| FR-025 | The user MAY assign a profile-specific keyboard shortcut from the GUI (e.g., `Ctrl+Shift+1` invokes "Quick" profile on selected files). | MAY | no | **Open** — app-wide shortcuts exist (NFR-034), per-profile ones do not. |
| FR-026 | Profile changes MUST be saved atomically; an interrupted save MUST NOT corrupt the profile store. | MUST | yes | **Done** — temp-file + rename, covered by `tests/profile_roundtrip.rs::fr_026_atomic_save_no_partial_file`. |
| FR-027 | The system MUST support an unlimited number of user-defined profiles (no artificial cap). | MUST | yes | **Done** — no cap in the store; `fr_027_five_hundred_profiles` exercises 500. The *menu* caps how many profiles it lists, which is a display limit, not a store limit. |

## 3. Print Job Lifecycle

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-030 | The system MUST maintain a queue of pending and active print jobs persisted to disk so that a crash does not lose unprinted work. | MUST | yes | **Done** — `queue.json` behind a single-writer dispatcher with crash recovery on boot. |
| FR-031 | The user MUST be able to view queue status: pending, converting, spooling, printed, failed. | MUST | yes | **Done** — three views (list, board, timeline) fed by typed `queue-updated` / `job-progress-event` events. "Printed" is real: a spooled job stays `printing` until the OS job completes (CUPS `ipptool`/`lpstat`, Windows `GetJob`), and a job the OS queue holds, or a printer that needs attention, is shown as such. |
| FR-032 | The user MUST be able to **pause**, **resume**, and **cancel** individual jobs. | MUST | yes | **Done** — `pause_job` / `resume_job` / `cancel_job`, plus `retry_job` and `cancel_all`. |
| FR-033 | The user MUST be able to **pause/resume the entire queue**. | MUST | yes | **Done** — `pause_queue` / `resume_queue`. |
| FR-034 | Failed jobs MUST be retried automatically up to **3 times** with exponential backoff before being marked permanently failed. | MUST | yes | **Done** — `MAX_ATTEMPTS = 4` (initial attempt plus 3 retries) with `RETRY_BACKOFFS` of 1s/5s/25s. Errors the user must fix (missing dependency, unsupported setting) pause instead of burning retries. |
| FR-035 | The system MUST log every job (start time, end time, file path, profile used, printer, page count, status, error if any) in a structured format. | MUST | yes | **Done** — structured `tracing` records per job. Full paths are deliberately kept out of the default-level log (NFR-082): records carry the file name and the job id, so the "file path" clause is satisfied by an identifier, not an absolute path. The absolute path is emitted only at TRACE (`job_input_path`), for a user who opts in through `RUST_LOG`. The same rule covers `file_rejected`, `page_range_out_of_bounds`, `cli_job_printed`, `cache_delete_failed` and the handoff records. |
| FR-036 | The user MUST be able to view and clear the job history from the GUI. | SHOULD | yes | **Partial** — finished jobs are visible in the Queue and clearable via `clear_done` behind a confirm dialog with an undo window. There is no separate persisted history with search/filter; that is v0.8. |
| FR-037 | The system SHOULD process jobs concurrently up to a configurable limit (default: 2 concurrent jobs to avoid printer queue contention). | SHOULD | no | **Done**, ahead of MVP, with one scoping correction: the limit bounds concurrent **conversions**, not concurrent spooling. `engine.max_concurrent_conversions`, clamped 1–8 in both the loader and the IPC command, default 2, read into the dispatcher at app start and pushed to it again whenever the setting is saved (no restart). A paused, cancelled or timed-out conversion keeps its slot until its task actually stops, so the cap counts conversions in flight rather than jobs in the `converting` state. |
| FR-038 | The system MAY support reordering pending jobs in the queue via drag and drop. | MAY | no | **Open** — no reorder command exists. |

## 4. Settings

See [print-settings.md](./print-settings.md) for the full settings matrix.

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-040 | The user MUST be able to override profile settings on a per-invocation basis without modifying the saved profile. | MUST | yes | **Done** — the Print Job screen edits a job-local copy; the profile is untouched unless explicitly saved. |
| FR-041 | The "Configure print job" GUI MUST display all overridable settings with the profile's current values pre-filled. | MUST | yes | **Partial** — every setting except two is shown pre-filled and overridable, including the page range. The profile's `tray` and `quality` are sent with the job but the screen neither shows nor overrides them; change them in the profile. |
| FR-042 | The system MUST validate setting values before submitting (e.g., page ranges must parse, copies must be ≥1). | MUST | yes | **Done** — `engine::pagerange` parses and bounds-checks against real page counts on both sides of the IPC boundary; an out-of-range value blocks Print. |
| FR-043 | Invalid settings MUST surface inline error messages, not generic dialog boxes. | SHOULD | yes | **Done** — errors render inline next to the field. |
| FR-044 | The system MUST list available printers from the OS spooler dynamically; no hard-coded printer names. | MUST | yes | **Done** — `list_printers` / `discover_printers` over the Windows spooler and CUPS; the queue id is what gets sent, not the display name. |
| FR-045 | The system SHOULD remember the last-used printer per profile if the profile's `printer = "<default>"` placeholder is used. | SHOULD | no | **Open** — the `"<default>"` sentinel exists and resolves to the app or OS default printer, but nothing records a *last-used* printer per profile. |

## 5. Format Support

See [supported-formats.md](./supported-formats.md) for the full per-format matrix.

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-050 | The system MUST print **PDF** files natively (no conversion). | MUST | yes | **Done** — passthrough to SumatraPDF (Windows) or `lp` (macOS). |
| FR-051 | The system MUST print common raster images: **JPG, PNG**. | MUST | yes | **Done** — but not the way the docs used to say. The pipeline is pure Rust: the `image` crate decodes and `lopdf` embeds the bitmap in a one-page PDF. **No ImageMagick dependency.** |
| FR-052 | The system SHOULD print additional images: **TIFF, BMP, WebP, HEIC**. | SHOULD | yes | **Done** — TIFF, BMP and WebP are decoded in-process. HEIC/HEIF use the OS decoder (`engine::convert::heif`): `sips` on macOS, WIC on Windows. A Windows machine without the Microsoft Store *HEIF Image Extensions* pauses the job with a `DependencyMissing` that names them. Linux has no system decoder, so the job pauses with "convert to JPEG or PNG first". The Windows WIC path compiles cleanly for the Windows target but has not yet been run on a Windows machine. |
| FR-053 | The system SHOULD print **DOCX, XLSX, PPTX** via LibreOffice headless conversion. | SHOULD | no | **Done** — `engine::convert::office` runs `soffice --headless --convert-to pdf` with a private `-env:UserInstallation` profile per conversion, so it works while LibreOffice is open. It is bounded by the conversion timeout and the job's cancel flag (the process tree is killed). Legacy DOC/XLS/PPT take the same path. |
| FR-054 | The system SHOULD print **ODT, ODS, ODP** via LibreOffice. | SHOULD | no | **Done** — the same LibreOffice path as FR-053. |
| FR-055 | The system SHOULD print **HTML** via Chromium headless `--print-to-pdf`. | SHOULD | no | **Done** — `engine::convert::html` uses Edge, Chrome or Chromium headless with a private `--user-data-dir` and `--no-pdf-header-footer`. The step ends when the PDF is complete, because some Chrome builds never exit after printing. |
| FR-056 | The system SHOULD print **Markdown** via Pandoc → PDF pipeline. | SHOULD | no | **Done**, by a different route: Pandoc renders standalone HTML (print stylesheet and a `<base href>` at the source folder), then the FR-055 browser step prints it. This avoids LaTeX/wkhtmltopdf. Needs both Pandoc and a browser. |
| FR-057 | The system SHOULD print **TXT** and **RTF** via LibreOffice. | SHOULD | no | **Done** — the FR-053 LibreOffice path. TXT is imported as UTF-8 (`--infilter="Text (encoded):UTF8,LF,,,"`). |
| FR-058 | The system MUST detect missing optional dependencies (LibreOffice, Chromium, Pandoc) at startup and provide a clear remediation message in the UI ("LibreOffice is not installed — Office files will be skipped"). | MUST | no | **Done**, ahead of MVP — `engine::deps` looks for each dependency in bundled dirs, on `PATH` and in per-OS install locations, and reports through `app_info`. Each report lists the extensions it blocks while missing, and its remediation names the program and where to download it. The Print Job screen shows that text for the dependencies the selected files need. A job that reaches a missing converter **pauses** with the same text (`DependencyMissing`) instead of being skipped. No always-on Settings panel yet (v0.2). |
| FR-059 | The system MUST NOT silently skip files; every unsupported file MUST be reported in the job log with reason. | MUST | yes | **Done** — discarded files come back from inspection and folder expansion with a reason, are toasted, and are recorded against the job. |
| FR-060 | The system MAY support additional formats via a plugin API in v2+ (CAD, Visio, EPUB, email). | MAY | no | **Open** — v2. |

## 6. Notifications

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-070 | The system MUST surface job completion via a non-modal toast notification ("12 of 12 files printed successfully"). | MUST | yes | **Done** — the queue store announces a batch once, when every job in it is terminal; an all-cancelled batch is reported as cancelled, not printed. |
| FR-071 | The system MUST surface job failures with actionable error text and a link to the job log entry. | MUST | yes | **Partial** — a failed batch reports the error and offers the logs folder, and a job the dispatcher pauses on a user-fixable error (`printer_unavailable`, `dependency_missing`, `unsupported_setting`, …) is announced on its own with an **Open queue** action, since its batch never completes. The link is to the logs folder and the Queue, not to the individual job's log entry: the backend has no per-entry deep link. |
| FR-072 | The system SHOULD use native OS notifications (Windows Toast, macOS UNNotification) when available. | SHOULD | yes | **Done** — `tauri-plugin-notification`, driven Rust-side by `notify_native`. A finished batch, a failed batch and a job stopped for the user are offered to the OS notification centre, which shows them only while the window is unfocused, hidden or minimized, and only while **Settings → Native notifications** (`app.native_notifications`, on by default) is on. |
| FR-073 | The system MUST allow disabling notifications globally and per-profile. | SHOULD | no | **Partial** — **Settings → Native notifications** (`app.native_notifications`) turns OS notifications off globally. In-app toasts cannot be turned off, and there is no per-profile switch. |
| FR-074 | The system MAY support custom notification sounds. | MAY | no | **Open**. |

## 7. Updates

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-080 | The system MUST check for updates on startup (delayed by 30 seconds) and at most once per 24 hours. | MUST | yes | **Done** — runs in Rust `setup` on a detached thread, so a cold start never waits on the network, and single-instance guarantees it runs once per process. The 24 h throttle is persisted. It queries the GitHub `releases/latest` API, not `updates.maecly.com`. |
| FR-081 | The system MUST allow the user to manually trigger an update check from the **About** dialog ("Check for updates now"). | MUST | yes | **Done**. |
| FR-082 | The system MUST verify the cryptographic signature of any downloaded update before applying it. Updates with invalid signatures MUST be rejected. | MUST | yes | **Done** — `tauri-plugin-updater` verifies the minisign signature of the downloaded artifact against `plugins.updater.pubkey` before installing anything. A bad signature fails with `UpdateSignatureInvalid` ("failed its signature check and was not installed"), in About and as exit code 11 from `update install`. Releases are signed with `TAURI_SIGNING_PRIVATE_KEY` (see [ADR-0008](../adr/0008-installer-nsis-per-user.md)). No release pipeline publishes signed artifacts yet. |
| FR-083 | The system MUST install updates without requiring administrator privileges (per-user install). | MUST | yes | **Done** — Windows installs and updates through the per-user NSIS setup (`installMode: "currentUser"`, run passively by the updater). macOS swaps the `.app` in place and refuses to update a binary outside an app bundle. |
| FR-084 | The user MUST be able to **defer**, **install now**, or **skip this version** when an update is available. | MUST | yes | **Done** — About offers **Install now** (`install_update`, with download progress and a restart), **Remind me later**, and **Skip this version**. Skipping is persisted and version-scoped, so a newer release prompts again, and a skip at or below the running version is cleared. |
| FR-085 | The system SHOULD support release channels: **stable**, **beta**, **nightly**. Default channel is `stable`. | SHOULD | no | **Partial** — `updater.channel` is persisted and defaults to `stable`, but every value resolves to the same feed. The setting is inert. |
| FR-086 | The user MUST be able to switch channels from Settings; switching to a lower channel MUST require explicit confirmation. | MUST | no | **Partial** — the Settings selector exists and writes the value; there is no downgrade confirmation, and nothing behaves differently afterwards. |
| FR-087 | The system MUST never auto-install a major version bump (e.g., 1.x → 2.x); these require explicit user opt-in. | MUST | no | **Done** — nothing installs on its own. A major jump needs a separate confirmation: a dialog in About (`allowMajor`, which the backend requires) and `--yes` or a typed "yes" to a prompt naming the major update on the CLI. |

## 8. Internationalization

See [non-functional-requirements.md](./non-functional-requirements.md#internationalization) for non-functional aspects.

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-090 | The GUI MUST be available in **English** in MVP. | MUST | yes | **Done** — `en-US` is the default and the fallback for every missing key. |
| FR-091 | The GUI SHOULD be available in **Spanish** by v1.0. | SHOULD | no | **Done**, well ahead of plan — `es-ES`, complete across all eight namespaces. There is no separate `es-MX`; it matches on the language subtag. |
| FR-092 | All user-facing strings MUST be externalized in JSON resource files; no hard-coded strings in components. | MUST | yes | **Done** for the frontend — `src/i18n/locales/{lang}/<namespace>.json`, one file per screen. **Backend (Rust) strings are not externalized or translated**, so an error message that originates in the engine reaches the user in English. |
| FR-093 | The system MUST detect the OS locale on first run and select the matching language if available, falling back to English. | MUST | no | **Done**, ahead of plan — detected from `navigator.language`, narrowed to a supported locale, persisted once so the detection never overrides a later manual choice. The "detected" marker is only set after that write is saved, so a refused write (locked or unreadable `config.toml`) retries on the next launch. |
| FR-094 | The user MUST be able to override the language from Settings. | MUST | no | **Done**, ahead of plan — a Settings selector writing `app.locale`; the UI re-renders as soon as the write lands. |

## 9. Configuration & Persistence

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-100 | The system MUST persist configuration in human-readable TOML files at OS-appropriate locations (`%APPDATA%\MaeclyPrint\` on Windows, `~/Library/Application Support/MaeclyPrint/` on macOS). | MUST | yes | **Done** — `config.toml` and `profiles.toml` via `util::paths`; covered by `tests/settings_persist.rs::fr_100_*`. |
| FR-101 | The system MUST gracefully handle a missing or corrupt config file by falling back to defaults and surfacing a recoverable error. | MUST | yes | **Done** — `fr_101_*`. |
| FR-102 | All config writes MUST be atomic (write to temp + rename). | MUST | yes | **Done** — `fr_102_atomic_write`. |
| FR-103 | The system MUST embed a `schema_version` field in every persisted file and migrate old schemas forward when loading. | MUST | yes | **Partial** — `schema_version` is embedded in `config.toml`, `profiles.toml` and `queue.json` and is validated on load (`fr_103_schema_version_embedded`). There is no forward-migration path yet, because there is only one schema version; the requirement's second clause is untested until there is a second. |

## 10. Help & Discoverability

| ID | Requirement | Priority | MVP | Status |
|---|---|---|---|---|
| FR-110 | The GUI MUST include an **About** dialog showing version, license link, and a link to the documentation site. | MUST | yes | **Done** — version and build date from the build, honest component versions read from the real manifests, filesystem paths with open-logs, and allow-listed external links. A test fails the build if About claims a dependency the project does not have. |
| FR-111 | The GUI MUST include a **First Run** wizard that explains the context menu integration and creates a desktop shortcut on opt-in. | SHOULD | yes | **Partial** — a five-step onboarding flow exists (welcome, printers, CLI, profiles, ready) and is deliberately limited to things this build actually does. It therefore does **not** explain the context menu (unregistered) and does **not** offer a desktop shortcut (no installer). Both clauses of this requirement are unmet; the wizard's honesty is the reason. |
| FR-112 | The CLI MUST support `--help` for every subcommand. | MUST | yes | **Done** — `clap`-derived help on every subcommand and sub-verb, pinned by `tests/cli_contract.rs`. |
| FR-113 | The system MUST log to a user-accessible file (`%LOCALAPPDATA%\MaeclyPrint\logs\app.log`) with rotation. | MUST | yes | **Done** — size-capped rotation with retention, rather than `tracing-appender`'s clock-boundary rolling, plus an open-logs-folder action in About. |

---

## Traceability

Every functional requirement above SHOULD be referenced by:

- At least one **acceptance test**, tagged with the requirement id. The convention is a **test name**, not a file name: Rust test functions are named `fr_<NNN>_<scenario>` (`fr_026_atomic_save_no_partial_file`, `fr_102_atomic_write`), and Vitest `it()` titles are prefixed `FR-###:`. Integration binaries are named after the feature (`profile_roundtrip.rs`, `cli_contract.rs`, `folder_expansion.rs`), and the full naming table lives in [testing-strategy.md](../08-development/testing-strategy.md). *An earlier version of this section asked for files called `tests/fr_XXX_*.rs`; no such file has ever existed and none should be created.*
- At least one **issue or PR** that introduces or modifies the corresponding code path.
- A row in the **release checklist** for manual QA verification when relevant.

Requirements without coverage at release time MUST be either implemented, withdrawn (`[WITHDRAWN]` tag), or deferred (move `MVP: yes` → `no`).

### Open scope decisions raised by the 2026-07-20 audit

These are contradictions the audit could not resolve by reading the code, because they are decisions rather than defects. They need a human call before v0.1.0 is cut.

1. ~~**FR-052 / HEIC.**~~ Resolved: HEIC/HEIF are decoded by the OS (`sips` on macOS, WIC on Windows) rather than by a bundled codec. See the FR-052 row.
2. **FR-072 / native notifications.** *Resolved:* implemented with `tauri-plugin-notification` (see the FR-072 row).
3. **FR-082 / FR-083 vs. the MVP line.** *Resolved in code:* both are implemented (see [ADR-0008](../adr/0008-installer-nsis-per-user.md)). What remains is the release pipeline that signs and publishes the artifacts.
4. **Windows 10.** FR-002 speaks of the modern Windows 11 menu, and [milestones.md](../06-roadmap/milestones.md) M2 lists an `IShellExtInit` + `IContextMenu` fallback for Windows 10. The handler implements neither and `shell-ext-win/README.md` documents that route as deliberately not registered. Windows 10 support is currently an unstated *no*.
5. **Requirements ahead of their milestone.** FR-010, FR-024, FR-037, FR-058, FR-091, FR-093 and FR-094 are all `MVP: no` and all done. The `MVP` column no longer describes the build. Consider flipping them to `yes` so the column stays meaningful, or accept that it records original intent and read the `Status` column instead.
