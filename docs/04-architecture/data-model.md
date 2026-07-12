# Data Model

> **TL;DR** — MaeclyPrint persists three kinds of state: human-editable **TOML** (config and profiles), machine-managed **JSON** (the live print queue), and append-only **JSONL** (logs). Files live under XDG-equivalent paths on each OS. Each file carries a `schema_version` so older clients can refuse to load forward-incompatible data and a forward migration path is always defined.

## 1. File locations

| File                                | Windows                                                          | macOS                                                          |
| ----------------------------------- | ---------------------------------------------------------------- | -------------------------------------------------------------- |
| Config                              | `%APPDATA%\MaeclyPrint\config.toml`                              | `~/Library/Application Support/MaeclyPrint/config.toml`        |
| Profiles                            | `%APPDATA%\MaeclyPrint\profiles.toml`                            | `~/Library/Application Support/MaeclyPrint/profiles.toml`      |
| Queue (live state)                  | `%LOCALAPPDATA%\MaeclyPrint\queue.json`                          | `~/Library/Application Support/MaeclyPrint/queue.json`         |
| Logs (rotated daily)                | `%LOCALAPPDATA%\MaeclyPrint\logs\maeclyprint-YYYY-MM-DD.jsonl`   | `~/Library/Logs/MaeclyPrint/maeclyprint-YYYY-MM-DD.jsonl`      |
| Cache (converted PDFs)              | `%LOCALAPPDATA%\MaeclyPrint\cache\<job-id>.pdf`                  | `~/Library/Caches/MaeclyPrint/<job-id>.pdf`                    |
| Updater downloads (transient)       | `%LOCALAPPDATA%\MaeclyPrint\update-staging\`                     | `~/Library/Caches/MaeclyPrint/update-staging/`                 |
| Enterprise policy (**read-only**)   | `%ProgramData%\MaeclyPrint\policy.json`                          | `/Library/Application Support/MaeclyPrint/policy.json`         |

We do not write under `%PROGRAMFILES%` or `/Applications` after install — the user does not need admin rights at any time after setup, and uninstall does not orphan user-state.

Unlike every file above, `policy.json` is **read-only to MaeclyPrint**: an administrator provisions it and the app only ever reads it. The machine-wide system path (above) is preferred; a per-user copy at `<config dir>/policy.json` is honored as a dev/testing fallback. Its schema, precedence, and malformed-file behavior are documented in [security-model.md §4.3](./security-model.md#43-enterprise-policy-policyjson); the shape is a single optional field, `{ "default_printer": "<printer id>" }`.

These paths are computed by the `directories` crate (XDG-conformant on Linux, `KnownFolderId` on Windows, `~/Library/...` on macOS) wrapped in a `paths` module that is the only place hardcoded names appear.

```rust
// src/util/paths.rs
use directories::ProjectDirs;
use std::path::PathBuf;

const QUALIFIER: &str = "com";
const ORG:       &str = "Maecly";
const APP:       &str = "MaeclyPrint";

pub fn config_dir() -> PathBuf { dirs().config_dir().to_path_buf() }
pub fn data_dir()   -> PathBuf { dirs().data_dir().to_path_buf() }
pub fn cache_dir()  -> PathBuf { dirs().cache_dir().to_path_buf() }
pub fn log_dir()    -> PathBuf {
    #[cfg(target_os = "macos")]
    { home_dir().unwrap().join("Library/Logs/MaeclyPrint") }
    #[cfg(not(target_os = "macos"))]
    { data_dir().join("logs") }
}

fn dirs() -> ProjectDirs {
    ProjectDirs::from(QUALIFIER, ORG, APP).expect("home dir resolvable")
}
```

## 2. `config.toml` — global settings

### 2.1 Schema

```toml
schema_version = 1

[app]
locale = "en-US"
theme  = "system"            # "light" | "dark" | "system"
launch_minimized = false

[shell_extension]
mode = "sparse-msix"         # "sparse-msix" | "registry"
appear_for_extensions = []   # empty = all; otherwise [".pdf", ".docx", ...]
hide_for_extensions   = []   # explicit deny-list

[engine]
max_concurrent_conversions = 4
conversion_timeout_seconds = 60
max_file_size_bytes        = 209_715_200    # 200 MiB
default_profile = "Quick BW Duplex"
auto_retry_timeouts = false

[engine.converters]
libreoffice_path = "auto"     # "auto" or absolute path
imagemagick_path = "auto"
pandoc_path      = "auto"
chromium_path    = "auto"
sumatra_path     = "auto"

[updater]
channel = "stable"
check_on_startup = true
startup_delay_seconds = 30
include_prerelease = false

[telemetry]
crash_reports = false         # opt-in; false by default
anonymous_usage_metrics = false

[logging]
level = "info"                # "trace" | "debug" | "info" | "warn" | "error"
retain_days = 14
```

### 2.2 Rust binding

```rust
// src/settings/mod.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub app:    AppCfg,
    pub shell_extension: ShellExtCfg,
    pub engine: EngineCfg,
    pub updater: UpdaterCfg,
    pub telemetry: TelemetryCfg,
    pub logging: LoggingCfg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppCfg {
    pub locale: String,
    pub theme:  Theme,
    pub launch_minimized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme { Light, Dark, System }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineCfg {
    pub max_concurrent_conversions: u32,
    pub conversion_timeout_seconds: u64,
    pub max_file_size_bytes: u64,
    pub default_profile: String,
    pub auto_retry_timeouts: bool,
    pub converters: ConverterPaths,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConverterPaths {
    pub libreoffice_path: AutoPath,
    pub imagemagick_path: AutoPath,
    pub pandoc_path:      AutoPath,
    pub chromium_path:    AutoPath,
    pub sumatra_path:     AutoPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AutoPath {
    Auto(String),                 // "auto"
    Path(std::path::PathBuf),
}
```

The `Default` impl provides the values shown above. Reads use `figment::Figment::from(Toml::file(...))` so that env-var overrides (`MAECLYPRINT_ENGINE__MAX_CONCURRENT_CONVERSIONS=8`) are easy.

### 2.3 `[printers]` — printer preferences (M1)

MaeclyPrint-level printer state that must survive restarts, persisted as a `[printers]` table inside `config.toml` and exposed to the UI via the `get_printer_prefs` / `set_printer_prefs` IPC commands (see [ipc-and-cli.md](./ipc-and-cli.md) and [printers-management.md](../05-features/printers-management.md)). These are **preferences only — they never mutate the OS spooler**: "removing" a printer here soft-disables it in the app while leaving the OS queue installed.

```toml
[printers]
default_printer = "EPSON_L15160"      # app default printer id; absent → use the OS default
disabled = ["Old_Fax"]                # printer ids hidden in MaeclyPrint (still installed in the OS)

[printers.routes]
".pdf"  = "EPSON_L15160"              # file extension → printer id
".docx" = "HP_LaserJet"

[printers.overrides.EPSON_L15160]     # per-printer default tray/paper, keyed by printer id
tray  = "Tray 1"
paper = "A4"
```

Rust binding (`src/settings/mod.rs`):

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct PrinterPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_printer: Option<String>,               // app default printer id
    #[serde(default)]
    pub disabled: Vec<String>,                          // soft-disabled printer ids
    #[serde(default)]
    pub routes: BTreeMap<String, String>,               // extension → printer id
    #[serde(default)]
    pub overrides: BTreeMap<String, PrinterOverride>,   // printer id → tray/paper
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct PrinterOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tray: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
}
```

Every field defaults, so a `config.toml` with no `[printers]` table still loads. The Printers screen reconciles these ids against the live spooler on each load: a route target, override key, or default that the spooler no longer reports drives the missing-printer banner ([printers-management.md](../05-features/printers-management.md#edge-states)). Because `overrides` keys are tray/media names specific to one model, they are **dropped, not carried over**, when references are reassigned to a different printer. The effective default printer resolves as `policy.default_printer ?? printers.default_printer ?? OS default` — the org policy (§1, `policy.json`) wins when present.

## 3. `profiles.toml` — print profiles

A profile is a named bundle of print settings. The user can create, rename, duplicate, and delete profiles. One is marked as the default (the one used when invoked from the context menu without a chooser).

### 3.1 Schema

```toml
schema_version = 1

[[profile]]
id           = "01HX5K9C1ZAB...01HX"   # ULID, stable
name         = "Quick BW Duplex"
printer      = "<default>"             # "<default>" sentinel or printer name
color_mode   = "monochrome"            # "color" | "monochrome"
duplex       = "long-edge"             # "off" | "long-edge" | "short-edge"
copies       = 1
paper_size   = "A4"                    # ISO name or "Letter", "Legal", ...
orientation  = "auto"                  # "portrait" | "landscape" | "auto"
scaling      = "fit"                   # "fit" | "shrink" | "actual"
page_range   = "all"                   # "all" or "1-5,8,11-13"
collate      = true
quality      = "normal"                # "draft" | "normal" | "high"
tray         = "auto"                  # printer-tray name or "auto"
margins_mm   = { top = 10, right = 10, bottom = 10, left = 10 }
notes        = ""

[[profile]]
id           = "01HX5K9D2..."
name         = "Color Single-sided"
printer      = "HP LaserJet Pro M404dn"
color_mode   = "color"
duplex       = "off"
copies       = 1
paper_size   = "Letter"
orientation  = "auto"
scaling      = "fit"
page_range   = "all"
collate      = true
quality      = "high"
tray         = "auto"
margins_mm   = { top = 12.7, right = 12.7, bottom = 12.7, left = 12.7 }
```

### 3.2 Rust binding

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub id: String,                       // ULID
    pub name: String,
    pub printer: PrinterRef,
    pub color_mode: ColorMode,
    pub duplex: Duplex,
    pub copies: u32,
    pub paper_size: PaperSize,
    pub orientation: Orientation,
    pub scaling: Scaling,
    pub page_range: PageRange,
    pub collate: bool,
    pub quality: Quality,
    pub tray: Tray,
    pub margins_mm: Margins,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum PrinterRef { Default(DefaultMarker), Named(String) }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DefaultMarker { #[serde(rename = "<default>")] Sentinel }
```

The `<default>` marker is preserved verbatim on round-trip so the file remains human-readable.

### 3.3 Profile invariants

- `id` is set on creation (ULID) and never changed. Renaming changes `name` only.
- `name` must be unique within the file; the loader rejects duplicates.
- `copies` is in `1..=999`.
- `page_range` is parsed eagerly on load; invalid expressions cause the profile to be marked `disabled` rather than crashing.

## 4. `queue.json` — live print queue

JSON, not TOML, because:

- It changes very frequently (per-job state transitions).
- It is machine-managed; not intended for hand-editing.
- It must be readable in partial form by other tools (e.g., a tray-icon companion).

The file lives at `<data_dir>/queue.json` (`paths::data_dir()`); its wire shape is the `QueueSnapshot` reused verbatim by the `queue_status` IPC command and the `queue-updated` event (see [ipc-and-cli.md](./ipc-and-cli.md)). A resolved `JobSettings` bundle is embedded in each job — there is no `profile_id`/`overrides` split (profiles are frontend-local presets in M1) and no persisted OS spool id.

### 4.1 Schema

```json
{
  "schema_version": 1,
  "queue_paused": false,
  "jobs": [
    {
      "id": "018f9c2e-6a10-7b3a-9c4d-2f1e0a8b7c6d",   // UUID v7 (time-sortable)
      "batch_id": "018f9c2e-6a10-7b3a-9c4d-000000000001",
      "state": "converting",
      "input_path": "C:\\Users\\Miguel\\Pictures\\chart.png",
      "file_name": "chart.png",
      "input_ext": "png",
      "input_bytes": 491520,
      "pages": 1,
      "cache_pdf": "…\\MaeclyPrint\\cache\\conversions\\018f9c2e-…-7b3a.pdf",
      "settings": {
        "printer": null,
        "duplex": "none",
        "color_mode": "color",
        "copies": 1,
        "paper_size": "a4",
        "orientation": "auto",
        "scaling": "fit",
        "page_ranges": null
      },
      "profile_name": "Photo Color",
      "printer_resolved": null,
      "attempts": 0,
      "error": null,
      "progress": { "phase": "converting", "percent": 0.5 },
      "submitted_at": "2026-07-11T10:42:00Z",
      "started_at": "2026-07-11T10:42:01Z",
      "finished_at": null
    }
  ]
}
```

Field notes: `input_ext` is the lowercased extension without a dot; `pages` is best-effort (PDFs parsed with `lopdf` when ≤ 100 MB, images always `1`, else `null`); `cache_pdf` is set only for converted inputs (a PDF prints in place, so it stays `null`); `printer_resolved` records the printer actually chosen once a job reaches `printing`/`done`; all timestamps are RFC 3339 UTC. The full TS shapes are in [ipc-and-cli.md § 2.2](./ipc-and-cli.md#22-type-definitions).

States (`state` enum, lowercase):

- `queued` — created but not started.
- `converting` — a converter is producing the cache PDF.
- `converted` — cache PDF ready, waiting for a free printer.
- `printing` — handed off to the OS spooler (exactly one job prints at a time).
- `done` — printer reported completion (terminal); the cache PDF is deleted.
- `failed` — terminal error after retries; details in `error`.
- `paused` — user-paused, interrupted by shutdown, or stopped on a user-fixable error (`unsupported_setting` / `dependency_missing`); resumable, **not** auto-retried.
- `cancelled` — user cancelled (terminal).

Transient conversion/print failures do not go straight to `paused`: the dispatcher retries them (up to 3 attempts, 1 s then 5 s backoff) by returning the job to `queued` (re-convert) or `converted` (re-print, cache kept), and only marks `failed` once attempts are exhausted. The full state machine (`can_transition`) and retry policy live in `src-tauri/src/queue/{job,dispatcher}.rs` and the [print engine](./print-engine.md).

### 4.2 Atomic writes

The queue file is rewritten on every state transition. To avoid corruption from a crash mid-write:

```rust
use std::io::Write;
use tempfile::NamedTempFile;

fn write_queue(path: &Path, q: &Queue) -> std::io::Result<()> {
    let dir = path.parent().unwrap();
    let mut tmp = NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut tmp, q)?;
    tmp.flush()?;
    tmp.persist(path)?;            // atomic rename on same filesystem
    Ok(())
}
```

`tempfile::NamedTempFile::persist` is `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING` on Windows and `rename` on Unix; both are atomic across the same volume.

### 4.3 Load, recovery, and history cap

On launch the core loads `queue.json` and runs shutdown recovery before serving the queue: any job left `converting` or `printing` when the app last closed is moved to `paused` with `error.code = "interrupted_by_shutdown"` and its transient progress cleared. Such a job is surfaced as resumable rather than failed; resuming it clears the error and resets its attempt count. The recovered state is immediately re-persisted (so it survives an immediate re-crash), and orphaned cache PDFs — files in `<cache_dir>/conversions` that no live job's `cache_pdf` references — are swept.

**Corruption / forward-incompatibility.** A missing file loads as an empty queue with no side effects. An unreadable file, invalid JSON, or a `schema_version` greater than this build supports is renamed to `queue.json.bak` (best-effort), logged, and replaced with an empty queue — the file is taken whole or discarded whole, never partial-loaded.

**History cap.** On every save the terminal (done/failed/cancelled) jobs are pruned to the 200 most-recent (ranked by `finished_at`, falling back to `submitted_at`); every non-terminal job is always kept, so the file cannot grow without bound.

## 5. Logs — JSONL

Logs are *structured* JSON Lines, one event per line, newline-terminated. This is friendly to `jq`, `Sentry`, log shippers, and humans (`less`).

### 5.1 Schema

Each line:

```json
{"ts":"2026-05-03T10:42:01.123Z","lvl":"info","module":"engine::router","event":"converter_selected","job":"01HX5KAB","mime":"application/pdf","converter":"passthrough"}
{"ts":"2026-05-03T10:42:02.501Z","lvl":"warn","module":"engine::converters::libreoffice","event":"timeout","job":"01HX5KCD","timeout_s":60}
{"ts":"2026-05-03T10:42:03.000Z","lvl":"info","module":"engine::printer::windows","event":"job_spooled","job":"01HX5KAB","spool_id":42,"printer":"HP LaserJet"}
```

Mandatory fields:

- `ts`  — RFC 3339 with millisecond precision, always UTC.
- `lvl` — `trace` | `debug` | `info` | `warn` | `error`.
- `module` — `tracing` target.
- `event` — short identifier for the event type (snake_case).

Other fields are event-specific. We do not log file *contents* and do not log absolute paths in WARN/ERROR records unless `logging.include_paths_in_errors = true`.

### 5.2 Rotation and retention

`tracing-appender` rotates daily (`maeclyprint-YYYY-MM-DD.jsonl`). On startup the logger sweeps anything older than `logging.retain_days` (default 14).

### 5.3 Tracing setup

```rust
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init(dir: &Path, level: tracing::Level) {
    let file = tracing_appender::rolling::daily(dir, "maeclyprint.jsonl");
    let (nb, guard) = tracing_appender::non_blocking(file);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(level.into()))
        .with(fmt::layer().json().with_writer(nb))
        .init();

    // Hold guard for the lifetime of the process.
    Box::leak(Box::new(guard));
}
```

## 6. Schema migration policy

Every persisted file carries a `schema_version: u32` at the top.

### 6.1 Forward compatibility

When MaeclyPrint sees a file with `schema_version` *higher* than it knows about, it:

1. Logs a warning.
2. Refuses to write to that file (would otherwise downgrade it).
3. Continues to function with in-memory defaults where possible (e.g., for `config.toml` it falls back to `Config::default()`).

Specifically, profiles and queue with future versions block the relevant feature with a clear "this file was written by a newer MaeclyPrint" error rather than silently dropping fields.

### 6.2 Backward migrations

When we read a file with an *older* version, we run a chain of migration functions:

```rust
// src/util/migrate.rs
pub fn migrate_config(value: &mut toml::Value) -> Result<()> {
    let mut v = value["schema_version"].as_integer().unwrap_or(1) as u32;
    while v < CURRENT_CONFIG_VERSION {
        match v {
            1 => { migrate_config_1_to_2(value)?; v = 2; }
            2 => { migrate_config_2_to_3(value)?; v = 3; }
            _ => break,
        }
    }
    value["schema_version"] = toml::Value::Integer(v as i64);
    Ok(())
}
```

Each `migrate_X_to_Y` is small, pure, and unit-tested with golden files.

### 6.3 Pre-migration backup

Before running migrations, the original file is copied to `<filename>.v<old>.bak`. If the migration succeeds the backup stays for one major version cycle, then is GC'd.

## 7. Concurrency rules

- **Config and profiles:** read on startup, watched with `notify`, hot-reloaded on change. Writes go through a single `Settings` actor (mpsc) that owns the in-memory value.
- **Queue:** owned by the dispatcher task; persisted on every transition (atomic write). The UI reads via `invoke('queue_status')` which returns a serializable snapshot, not a borrow.
- **Logs:** `tracing-appender` `non_blocking` queues writes off the hot path.

## 8. Testing the data model

- **Round-trip tests.** For each schema, `serde` round-trip from a golden TOML/JSON file and assert byte-identical output (with stable formatting).
- **Migration tests.** For each old-version golden file, run the migration chain and assert the result equals the corresponding current-version golden.
- **Forward-version tests.** Load a file with `schema_version = u32::MAX` and assert the loader returns `Err(SchemaTooNew)`.
- **Crash-resistance tests.** Simulate process kill mid-write of `queue.json` and verify the on-disk file is either the old or new value, never partial.

## 9. Related documents

- [`system-architecture.md`](./system-architecture.md) — how these files fit into the running system.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — IPC commands that read/write profiles and queue.
- [`security-model.md`](./security-model.md) — file permissions and what we never log.
