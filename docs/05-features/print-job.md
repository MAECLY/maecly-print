# Print Job

> **TL;DR** — The **Print Job** screen: a two-pane composer where the user assembles a batch of files on the left and dials in print settings on the right, sees the *exact* command MaeclyPrint will run, and submits the batch to the print queue. Files are added from the OS picker or by dragging them onto the window; every file is inspected for type/size/page-count before it can be printed.

This document is the source of truth for the **content and behavior** of the Print Job screen (`src/screens/PrintJobScreen.tsx`). The queue that receives a submitted batch lives in [queue.md](./queue.md); the conversion/print pipeline behind it is the [print engine](../04-architecture/print-engine.md); the wire contract is the [IPC & CLI doc](../04-architecture/ipc-and-cli.md); persisted shapes are in the [data model](../04-architecture/data-model.md).

The screen is the GUI counterpart to the `print_files` IPC command, with `inspect_files` and `preview_command` as its two read-only helpers. It is the destination of the **New print job** action on Home and of the right-click "Print with MaeclyPrint" shell entry.

## Screen layout

A two-pane split: the file list fills the main area; a fixed settings panel sits on the right.

```
┌─ Print Job ───────────────  3 files · 12 pages · 4.2 MB      [+ Add files] [🖨 Print 3] ┐
├──────────────────────────────────────────────────────┬─────────────────────────────────┤
│ Files in this job                          [↑↓ reorder]│ PROFILE                         │
│ ┌────────────────────────────────────────────────────┐│ [Quick · built-in           ▾] │
│ │ 01  📄 report.pdf            8 pages · 3.1 MB  ↑ ↓ ✕││ PRINTER                         │
│ │ 02  🖼 chart.png             image · 480 KB    ↑ ↓ ✕││ [System default             ▾] │
│ │ 03  📄 cover.pdf             4 pages · 610 KB  ↑ ↓ ✕││ SIDES  [Single][Long][Short]   │
│ └────────────────────────────────────────────────────┘│ COLOR  [Color][B&W]            │
│                                                        │ COPIES [ 1 ] × per file        │
│  (drag files from Explorer / Finder anywhere here)     │ PAPER [A4 ▾]   ORIENT [Auto ▾] │
│                                                        │ SCALING [Fit][Shrink][100%]    │
│                                                        │ PAGE RANGE [ 1-5,10        ]   │
│                                                        │ ┌ RESOLVED COMMAND ──────────┐ │
│                                                        │ │ lp -d "Office" -o sides=…  │ │
│                                                        │ └────────────────────────────┘ │
└──────────────────────────────────────────────────────┴─────────────────────────────────┘
```

- **Top bar** — title `Print Job`, subtitle `{files} files · {pages} pages · {size}` (page total sums per-file page counts, images counting as 1), and two actions: **Add files** (opens the OS picker; disabled outside the desktop app) and **Print {n}** (submits; disabled until at least one file is present and every field is valid).
- **File list (left)** — one row per file: an index, a file-type glyph, `name.ext`, a `{pages}·{size}` sub-line, and per-row **move up / move down / remove** controls. The whole pane is a drop target (dashed accent outline while dragging).
- **Settings panel (right, 280 px)** — profile picker, then the per-invocation overrides, then the honesty **Resolved command** preview.

## Adding files

Two input paths, both funnelling through one `addPaths` routine:

1. **Add files button** — opens the OS file picker via `@tauri-apps/plugin-dialog`, filtered to the supported extensions (multi-select).
2. **Native drag & drop** — dragging files from Explorer/Finder onto the window (`getCurrentWebview().onDragDropEvent`, FR-007). The pane highlights on `over` and ingests paths on `drop`.

`addPaths` calls `inspect_files` for the dropped/picked paths, then:

- **Dedupes** against files already in the list (by absolute path).
- **Appends** the printable ones as list rows.
- **Rejects** unsupported/missing files, listing them by name and reason in a single error toast (never silently dropping them).

Adding files requires the desktop host; in the standalone web preview the button is disabled and drag & drop is inert.

## File inspection

Each path is classified independently by `inspect_files` → `FileInfo { path, name, ext, bytes, pages, supported, reason }`:

- `supported` is `true` only for the engine's printable extensions (below). An unsupported or missing file comes back `supported: false` with a human `reason` (`"Unsupported file type"`, `"File not found"`, `"Not a file"`) rather than failing the whole call.
- `pages` is best-effort: PDFs are parsed with `lopdf` when ≤ 100 MB (larger PDFs report `null` until conversion), supported images are always `1`, everything else is unknown.

**Supported extensions (M1):** `pdf`, `jpg`, `jpeg`, `png`, `tif`, `tiff`, `bmp`, `webp`. This mirrors the engine's allowlist; Office/HTML/Markdown are on the [print engine](../04-architecture/print-engine.md) roadmap, not yet accepted here.

## Settings panel

A profile provides the baseline; the controls below it are per-invocation overrides that win over the profile (FR-040/041). All of them map 1:1 onto the wire `JobSettings` — no hidden flags.

| Control      | Options                                   | Wire field (`JobSettings`)                       |
| ------------ | ----------------------------------------- | ------------------------------------------------ |
| **Profile**  | built-in + saved custom profiles          | `profile_name` (display/log only)                |
| **Printer**  | `System default` + installed printers (minus MaeclyPrint-disabled) | `printer` (`null` = default) |
| **Sides**    | Single / Long / Short                     | `duplex` = `none` / `long` / `short`             |
| **Color**    | Color / B&W                               | `color_mode` = `color` / `monochrome`            |
| **Copies**   | integer, validated `1..=999`              | `copies`                                         |
| **Collate copies** | switch, on by default               | `collate` (CUPS `-o collate=…` when copies > 1)  |
| **Paper**    | Printer default / A4 / Letter / Legal / A3 + every size the chosen printer reports | `paper_size` (`auto` = printer default; classic sizes lowercase, others verbatim) |
| **Orient.**  | Auto / Portrait / Landscape               | `orientation`                                    |
| **Scaling**  | Fit / Shrink / 100%                       | `scaling` = `fit` / `shrink` / `none`            |
| **Page range** | e.g. `1-5,10` (blank = all)             | `page_ranges` (`null` when blank)                |

- **Profiles are backend-persisted** (`list_profiles` / `profiles.toml`, see [profiles-management](./profiles-management.md)), with `BUILT_IN_PROFILES` in `src/lib/profiles.ts` as the static fallback when the backend is unreachable. A profile only seeds the panel — the overrides are the real source of truth and win over it; `profile_name` rides along purely for the log/UI.
- **Changing the profile resets the overrides** (FR-041): from the dropdown, from Ctrl/⌘+K or from a context-menu selection, the new profile's own values are what the panel shows and what is sent — overrides made against the previous profile never linger behind a dropdown that names another one. Overrides are also dropped after a successful print: they were tweaks for that job.
- **Copies** shows a *draft* only while the field differs from the committed value (an invalid entry, or one mid-edit); otherwise it shows the effective value, so a profile change from anywhere is reflected immediately. An out-of-range entry shows an inline error instead of being silently clamped; only a valid value flows into the request (`isValidCopies`). The draft lives in `App`, so the Ctrl/⌘+P shortcut refuses the same invalid entry the Print button does (one shared rule, `printBlocker`).
- **Page range** is validated client-side against a mirror of the engine grammar (`isValidPageRange`) so the Print button reflects intent; the backend re-validates authoritatively (`engine::pagerange::parse`). Ranges apply to PDFs and to converted documents (pages of the converted PDF); images ignore them.
- **Printer** options come from `list_printers` with MaeclyPrint-disabled queues filtered out (via `get_printer_prefs`).
- **Paper** lists the sizes the selected printer reports (`Printer.papers`), so photo, envelope and other sizes can be chosen; a size the printer does not list pauses the job (`unsupported_setting`) instead of being substituted silently. **Printer default** sends no size, so the printer's override from the Printers screen (or its own default) applies.

## Resolved command (honesty preview)

Below the settings, a **Resolved command** panel shows the *exact* command the backend will run for the current files + settings — `lp -d <printer> -t <file name> … <pdf>` on macOS/Linux (built by the same `cups::lp_args` the spooler calls, job title included), the `SumatraPDF.exe -print-to … -print-settings '…'` line on Windows. The macOS/Linux line is written for a POSIX shell and the Windows line for PowerShell. Every argument a shell would split or expand is single-quoted: POSIX escapes an inner `'` as `'\''`, and PowerShell doubles every character it accepts as a single quote (`'` and the typographic U+2018, U+2019, U+201A, U+201B). File names come from outside, so a download called `x$(curl …|sh).pdf` pastes as a literal name and never runs as a command. For an image the spooled argument is shown as `<converted PDF of chart.png>`, because the engine converts it first and spools that PDF, not the source file. It is produced by the `preview_command` IPC (the same argument builders the dispatcher uses, so preview and dispatch never drift), debounced 300 ms, and hidden outside the desktop app, with no files, or while any field is invalid. A failed preview simply renders nothing — it is advisory.

The preview covers **every file**, not just the first: each file is resolved the way dispatch resolves it — its printer (a file-type routing rule can send the `.pdf` and the `.png` of one batch to different printers), its settings narrowed to the input (page ranges are dropped for images, which are one page; PDFs and the PDFs converted from Office, HTML and Markdown keep them), and the printer's own tray/paper defaults where the job leaves them open. Files that come out identical share one command. When the batch splits, the panel shows one command per group, each preceded by a `#` comment line naming its files (`# chart.png, photo.png`). A line break or other control character in a file name is shown as `?` there, so it cannot end the comment. Lines the backend cannot express on Windows (quality, collation, an unsupported paper size) follow as `# note:` lines.

## Submitting

**Print {n}** (or Ctrl/⌘+P, which applies exactly the same `printBlocker` rule and is refused with a toast while a field is invalid) builds a `PrintRequest { files, settings, profile_name }` (`buildPrintRequest`) and calls `print_files`, which validates and fans the batch out into one job per file (see [queue.md](./queue.md)). On success the app toasts and navigates to the Queue, and **only the files that were sent** leave the list: anything added while the request was in flight (a late folder walk, a drop) stays for the next batch. When the list ends up empty, unanswered folder questions go with it. A validation error (empty list, bad copies/range, missing/unsupported file) surfaces as an error toast and nothing is enqueued.

While a folder is being read (FR-004) a loading toast names it and Print stays disabled, so a batch can never be spooled without the files still arriving. The "Added N files" toast counts only files that were new to the job.

A file whose page count is unknown (a PDF above the probe ceiling or one that will not parse) is labelled "pages unknown" — never "image" — and the top-bar total then reads "at least N pages".

**Context-menu selections that print immediately** ("Print with MaeclyPrint", "Add to print queue") do not go through this screen's list at all: their files are resolved on their own (every folder walked top-level, no question), printed with the named profile exactly as saved (no overrides), and the job the user was composing here is left untouched. "Add to print queue" submits the batch with `hold: true`, so its jobs land paused while the rest of the queue keeps running. Selections are handled strictly in order, one at a time.

## Actions

| Action         | Where          | Behavior                                                                 |
| -------------- | -------------- | ------------------------------------------------------------------------ |
| **Add files**  | top bar        | OS picker (desktop only); adds the printable, non-duplicate selections.  |
| **Print {n}**  | top bar        | Submits the batch via `print_files`; disabled until valid.               |
| **Reorder**    | file row       | Move a file up/down in the batch order.                                  |
| **Remove**     | file row       | Drop a file from the batch (does not touch the file on disk).            |

## Per-platform differences

| Concern            | Windows                                   | macOS / Linux                          |
| ------------------ | ----------------------------------------- | -------------------------------------- |
| File picker & DnD  | `tauri-plugin-dialog` + `onDragDropEvent` | same (platform-neutral)                |
| Print backend      | SumatraPDF (`-print-to … -print-settings`) | CUPS `lp`                              |
| Preview string     | `SumatraPDF.exe -print-to …`              | `lp -d "…" -o …`                       |

The screen itself is platform-neutral; only the resolved command and the eventual spool differ, and both come from the engine.

## IPC mapping

| UI surface                    | IPC command                                | Notes                                        |
| ----------------------------- | ------------------------------------------ | -------------------------------------------- |
| Add files / drag & drop       | `inspect_files(paths) => FileInfo[]`       | Per-file type/size/pages; bad paths flagged, not thrown |
| Resolved command panel        | `preview_command(req) => string`           | Exact backend command; debounced 300 ms      |
| Print {n}                     | `print_files(req) => JobBatch`             | Validates + enqueues one job per file        |
| Printer dropdown              | `list_printers()` / `get_printer_prefs()`  | Installed printers minus MaeclyPrint-disabled |

See [ipc-and-cli.md](../04-architecture/ipc-and-cli.md) (§ jobs/queue) for the request/response types.

## Implementation status

- **M0** — front-end only, mock files/settings (superseded).
- **M1 (current)** — **real end-to-end submission, no mock data.** Files are added via the OS dialog and native drag & drop, inspected with `inspect_files`, validated inline (copies + page-range mirrors), previewed with `preview_command`, and submitted with `print_files` into the real queue. Rust: `src-tauri/src/ipc/jobs.rs` (+ `queue`, `engine`). Frontend: `PrintJobScreen.tsx`, `src/lib/profiles.ts`, `src/lib/ipc.ts`.
- **Follow-up** — additional input formats (Office/HTML/Markdown via the engine's documented converters), folder import, and per-file (rather than per-batch) settings.
