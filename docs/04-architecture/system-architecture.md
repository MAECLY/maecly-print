# System Architecture

> **TL;DR** — Pluryprint is a Tauri 2 app composed of (a) a thin C++ COM DLL hooked into Windows Explorer, (b) a single Rust binary that hosts the webview, the queue, and the CLI, and (c) external converter processes (LibreOffice, SumatraPDF, ImageMagick, Pandoc) orchestrated as short-lived subprocesses. Communication between layers uses well-defined IPC: shell-ext → CLI args (or named pipe), webview → Rust via Tauri commands, Rust → converters via stdin/stdout + exit codes.

## 1. C4 model — Context

```
+----------------------------------------------------------------------+
|                                                                      |
|                          End user                                    |
|             (right-clicks N files in Explorer/Finder)                |
|                                                                      |
+--------------------------------+-------------------------------------+
                                 |
                  "Print all" (context menu click)
                                 |
                                 v
+----------------------------------------------------------------------+
|                                                                      |
|                          Pluryprint                                 |
|         (Tauri app + Rust core + shell extension)                    |
|                                                                      |
+----+-----------------+-------------------+--------------------+------+
     |                 |                   |                    |
     v                 v                   v                    v
+--------+      +------------+     +--------------+     +-------------+
| OS print|     | External   |     | Updater CDN  |     | Crash       |
| spooler |     | converters |     | updates      |     | reporter    |
| (Win    |     | (LibreOff, |     | .maecly.com  |     | (opt-in,    |
| spooler |     | Sumatra,   |     |              |     | M3+)        |
| / CUPS) |     | Magick,    |     |              |     |             |
+----+----+     | Pandoc)    |     +------+-------+     +-------------+
     |          +-----+------+            |
     v                |                   v
+----------+          |             +-----------+
| Printer  |<---------+             | Manifest  |
| hardware |  (final spool)         | + delta   |
+----------+                        | binaries  |
                                    +-----------+
```

Key external systems:

- **OS print spooler.** Windows Print Spooler service or macOS CUPS daemon. Pluryprint never touches drivers directly; it always goes through the spooler.
- **External converters.** Optional binaries on the user's PATH (or bundled): LibreOffice (`soffice`), SumatraPDF, ImageMagick (`magick`), Pandoc, headless Chromium.
- **Updater CDN.** `https://updates.maecly.com/` — static manifest JSON + signed binary deltas served from S3 / CloudFront / R2.
- **Crash reporter (opt-in, M3+).** Sentry-compatible endpoint; off by default, requires explicit user consent.

## 2. C4 model — Container

```
+------------------------------------------------------------------+
|                       Pluryprint (process model)                |
|                                                                  |
|  +----------------------------+    +-----------------------+     |
|  | Shell extension DLL        |    | Tauri main process    |     |
|  | (C++17, ~500-1000 LOC)     |    | (single Rust binary)  |     |
|  | - IExplorerCommand impl    |    |                       |     |
|  | - Sparse MSIX-registered   |    |  +-----------------+  |     |
|  | - Receives IShellItemArray |    |  | Webview (UI)    |  |     |
|  | - Serializes paths to JSON |    |  | React + TS      |  |     |
|  | - Spawns Tauri exe with    |    |  | (WebView2 / WK) |  |     |
|  |   --invoke=context-menu    |    |  +--------+--------+  |     |
|  |   --files=<json-or-pipe>   |    |           ^           |     |
|  +-------------+--------------+    |           | Tauri IPC |     |
|                |                   |           v           |     |
|         spawn / pipe               |  +-----------------+  |     |
|                v                   |  | Rust core       |  |     |
|        +---------------------------+  | (modules below) |  |     |
|        |                              +--------+--------+  |     |
|        |                                       |           |     |
|        +---------------------------------------+           |     |
|                                                ^           |     |
|                                                | spawn     |     |
|                                                v           |     |
|  +-------------------------+   +-------------------------+ |     |
|  | Converter subprocesses  |   | Printer adapter         | |     |
|  | (soffice, magick, ...)  |   | (SumatraPDF / lpr)      | |     |
|  +-------------------------+   +-------------------------+ |     |
|                                                            |     |
+------------------------------------------------------------+-----+
```

Process boundaries:

- **Shell extension DLL** runs *inside `explorer.exe`*. It is platform code, written in C++17 against `IExplorerCommand`. It must be tiny, fast, and crash-safe — a crash here brings down Explorer. It does no real work other than serializing the user's selection and spawning the Tauri executable.
- **Tauri main process** is the single Rust binary that hosts the webview and exposes IPC commands. It also serves as the CLI when invoked with `--cli`.
- **Converter subprocesses** are short-lived. Each conversion is its own process with a bounded timeout (default 60 s). Process death is just an `Err` returned to the queue.

## 3. C4 model — Component (Rust core)

The Rust core is organized into modules that map roughly to crates inside a workspace:

```
src/
├── main.rs               # entry point: routes to GUI, CLI, or context-menu mode
├── app.rs                # Tauri builder, capabilities, plugin registration
├── ipc/                  # tauri::command handlers (web → Rust)
│   ├── mod.rs
│   ├── printers.rs       # list_printers, default_printer
│   ├── profiles.rs       # list/save/delete profiles
│   ├── jobs.rs           # print_files, cancel_job, queue_status
│   └── update.rs         # check/install update
├── cli/                  # CLI command parsing and dispatch (clap)
│   ├── mod.rs
│   ├── print.rs
│   ├── profiles.rs
│   ├── queue.rs
│   └── update.rs
├── queue/                # in-memory + on-disk print queue
│   ├── mod.rs
│   ├── state.rs          # job lifecycle, transitions
│   └── persistence.rs    # queue.json read/write, atomic swap
├── engine/               # the print engine
│   ├── mod.rs
│   ├── router.rs         # format → converter selection
│   ├── converters/
│   │   ├── libreoffice.rs
│   │   ├── imagemagick.rs
│   │   ├── pandoc.rs
│   │   ├── chromium.rs
│   │   └── passthrough.rs   # already-PDF
│   └── printer/
│       ├── windows.rs      # SumatraPDF dispatch
│       └── macos.rs        # lpr dispatch (M5+)
├── profiles/             # profile schema + storage
├── settings/             # global config
├── updater/              # tauri-plugin-updater integration glue
├── shell_ipc/            # named-pipe server for the COM DLL (when not using args)
├── logging/              # tracing setup, JSONL appender
├── platform/
│   ├── windows/
│   │   ├── printers.rs   # EnumPrinters via windows-rs
│   │   └── registry.rs   # COM CLSID lookups
│   └── macos/            # M5+
└── util/                 # error types, paths, file detection
```

Module boundaries are enforced by `pub(crate)` and crate-level visibility. The `engine`, `queue`, `profiles`, `settings`, and `updater` modules are independently unit-testable with mock implementations of `printer` and `converter` traits.

### Key traits

```rust
// engine/converters/mod.rs
#[async_trait]
pub trait Converter: Send + Sync {
    fn supports(&self, mime: &str) -> bool;
    async fn convert(
        &self,
        input: &Path,
        output: &Path,
        opts: &ConvertOptions,
    ) -> Result<(), ConvertError>;
}

// engine/printer/mod.rs
#[async_trait]
pub trait Printer: Send + Sync {
    async fn list(&self) -> Result<Vec<PrinterInfo>, PrintError>;
    async fn print(&self, pdf: &Path, settings: &PrintSettings) -> Result<JobId, PrintError>;
    async fn status(&self, id: JobId) -> Result<JobStatus, PrintError>;
    async fn cancel(&self, id: JobId) -> Result<(), PrintError>;
}
```

This isolation is what allows us to swap SumatraPDF for `lpr` on macOS, and to mock both in tests.

## 4. Sequence — "Right-click → Print all"

```
User           Explorer        Shell DLL          Tauri exe         Rust core         Converter       Printer
 |                 |               |                  |                 |                |              |
 | right-click on  |               |                  |                 |                |              |
 | N files (>15)   |               |                  |                 |                |              |
 |---------------> |               |                  |                 |                |              |
 |                 | Invoke()      |                  |                 |                |              |
 |                 | with          |                  |                 |                |              |
 |                 | IShellItem-   |                  |                 |                |              |
 |                 | Array         |                  |                 |                |              |
 |                 |-------------->|                  |                 |                |              |
 |                 |               | enumerate items, |                 |                |              |
 |                 |               | resolve full     |                 |                |              |
 |                 |               | paths, dedupe    |                 |                |              |
 |                 |               |                  |                 |                |              |
 |                 |               | spawn(           |                 |                |              |
 |                 |               |  Pluryprint.exe |                 |                |              |
 |                 |               |  --invoke=ctx    |                 |                |              |
 |                 |               |  --files=<json>) |                 |                |              |
 |                 |               |----------------->|                 |                |              |
 |                 |               | (DLL exits)      |                 |                |              |
 |                 |               X                  |                 |                |              |
 |                 |                                  | parse args,     |                |              |
 |                 |                                  | open webview,   |                |              |
 |                 |                                  | render print    |                |              |
 |                 |                                  | dialog          |                |              |
 |                 |                                  |                 |                |              |
 | dialog appears  |                                  |                 |                |              |
 |<-------------------------------------------------- |                 |                |              |
 |                                                    |                 |                |              |
 | choose profile, |                                  |                 |                |              |
 | click "Print"   |                                  |                 |                |              |
 |--------------------------------------------------->|                 |                |              |
 |                                                    | invoke(         |                |              |
 |                                                    |  print_files,   |                |              |
 |                                                    |  {files,profile}|                |              |
 |                                                    |---------------->|                |              |
 |                                                    |                 | enqueue, build |              |
 |                                                    |                 | per-file plan  |              |
 |                                                    |                 | (router)       |              |
 |                                                    |                 |                |              |
 |                                                    |                 | for each file: |              |
 |                                                    |                 | spawn converter|              |
 |                                                    |                 |--------------->|              |
 |                                                    |                 |                | (write PDF)  |
 |                                                    |                 |<---------------|              |
 |                                                    |                 |                |              |
 |                                                    |                 | spawn printer  |              |
 |                                                    |                 | (SumatraPDF)   |              |
 |                                                    |                 |---------------------------->| |
 |                                                    |                 |                |              |
 |                                                    | progress events |                |              |
 |                                                    |<----------------|                |              |
 |                                                    |                                                  |
 | progress UI     |                                  |                                                  |
 | updates         |                                  |                                                  |
 |<----------------------------------------------------                                                  |
```

Notes on this flow:

- The DLL is **dead before the user even sees the dialog**. This keeps Explorer responsive.
- The Tauri exe deduplicates instances by named mutex (Windows) or PID-file (Mac). A second right-click while one is open routes the new selection into the existing instance via named-pipe.
- The print dialog can be **skipped** entirely if the user holds `Shift` while clicking (or the option is on in settings) — in that case the active default profile is used and the queue starts immediately.

## 5. Threading and concurrency model

- **Single-process app.** No multi-process renderer split (Tauri uses the system webview, not a Chromium child process).
- **`tokio` multi-thread runtime** for the Rust core; defaults to `num_cpus / 2` worker threads.
- **The print queue is a `tokio::sync::mpsc` channel** producer-consumer. Producers: IPC handlers, CLI commands. Consumer: a single dispatcher task that owns the queue state.
- **Conversions run in a `rayon` pool** to bound concurrency at `min(num_cpus, max_concurrent_conversions)` (default 4). This stops 200 LibreOffice instances from crushing the machine.
- **Printer dispatch is sequential by default.** Most printers spool sequentially anyway, and printing one PDF at a time keeps the UI status meaningful.

## 6. Data flow — what crosses each boundary

| Boundary                  | Format                              | Notes                                         |
| ------------------------- | ----------------------------------- | --------------------------------------------- |
| Explorer → DLL            | `IShellItemArray` COM interface     | Native; no serialization                      |
| DLL → Tauri exe           | CLI args (small N) or named pipe (large N) | JSON-encoded list of absolute paths      |
| Webview → Rust            | Tauri IPC (JSON over channel)       | Typed via TS bindings                         |
| Rust → Converter          | CLI args + stdin                    | Per-converter; see [`print-engine.md`](./print-engine.md) |
| Converter → Rust          | Output file + exit code + stderr    | Captured into JSONL log                       |
| Rust → Printer (Win)      | `SumatraPDF.exe -print-to ...`      | Settings serialized as `-print-settings`     |
| Rust → Printer (Mac)      | `lpr -P ... -o ... file.pdf`        | UNIX-style options                            |
| Rust → Updater endpoint   | HTTPS GET (manifest), HTTPS GET (delta) | minisign signature verified                   |
| Rust → Disk (logs/queue)  | JSONL (logs), JSON (queue), TOML (config/profiles) | Atomic writes via `tempfile::NamedTempFile` + `persist` |

## 7. Failure isolation

| Failure                              | Effect                                      | Mitigation                                       |
| ------------------------------------ | ------------------------------------------- | ------------------------------------------------ |
| Shell DLL crashes                    | Explorer may hang or restart                | Tiny code surface, no allocations in hot path, fuzz tests |
| Tauri exe crashes mid-print          | Queue persisted to disk; resumes on relaunch | `queue.json` flushed after each state transition |
| Converter timeout                    | Single file marked failed; queue continues  | Per-job timeout + structured error logged        |
| Printer offline                      | Job marked retryable; user notified         | Spooler reports state; we surface in UI          |
| Update download fails                | App keeps running on current version        | Manifest signature verified before any swap      |
| Updater corruption mid-write         | Tauri keeps the old binary; reports error   | Atomic file replace via temp + rename            |

## 8. Related documents

- [`tech-stack.md`](./tech-stack.md) — the technology choices.
- [`shell-extensions.md`](./shell-extensions.md) — the DLL and its OS-level glue in detail.
- [`print-engine.md`](./print-engine.md) — the converter + printer pipeline.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — exhaustive IPC and CLI reference.
- [`data-model.md`](./data-model.md) — config, profile, queue, log schemas.
