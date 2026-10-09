//! Printer enumeration IPC commands.
//!
//! These commands surface the host's *real* printers. The OS-specific work
//! lives in [`crate::platform`] (CUPS CLI on Unix, the winspool API on
//! Windows); this module only defines the wire types and the thin Tauri
//! command wrappers, keeping the binding-generation surface clean.
//!
//! Capability fields are deliberately permissive: not every printer or OS
//! reports every attribute (a label printer has no toner levels; CUPS may not
//! expose a resolution list). Absent data is `None`/empty rather than faked,
//! and the UI hides whatever is missing. See
//! `docs/04-architecture/printer-enumeration.md`.

use std::path::Path;

use serde::Serialize;
use specta::Type;

use crate::util::error::IpcError;

/// Coarse printer family, used by the UI only to pick an icon.
#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PrinterKind {
    Laser,
    Mfp,
    Wide,
    Label,
    Virtual,
}

/// Normalized printer state (a small superset-free view of the OS state).
#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PrinterStatus {
    Ready,
    Offline,
    Error,
    Busy,
}

/// One input tray/bin. `paper` and `level` are optional because most OS APIs
/// expose the tray *name* but not the loaded media or fill level.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Tray {
    pub name: String,
    pub paper: Option<String>,
    /// Fill level in `0.0..=1.0`, if the device reports it.
    pub level: Option<f64>,
}

/// Toner/ink supply levels in `0.0..=1.0`. Any channel the device does not
/// report stays `None`; an all-`None` value renders no supply section.
#[derive(Debug, Clone, Default, Serialize, Type)]
pub struct Supplies {
    pub cyan: Option<f64>,
    pub magenta: Option<f64>,
    pub yellow: Option<f64>,
    pub black: Option<f64>,
}

/// A printer installed on the system, enriched with whatever capabilities the
/// OS could be coaxed into reporting.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Printer {
    /// Stable spooler/queue key (used as the React list key).
    pub id: String,
    /// Human-friendly name (falls back to the queue id).
    pub name: String,
    pub kind: PrinterKind,
    pub color: bool,
    pub duplex: bool,
    pub status: PrinterStatus,
    pub is_default: bool,
    pub location: String,
    /// Readable connection/port string, e.g. `USB`, `EPSON … (IPP)`.
    pub connection: String,
    pub driver: String,
    pub shared: bool,
    pub queued_jobs: u32,
    pub max_dpi: Option<u32>,
    pub papers: Vec<String>,
    pub trays: Vec<Tray>,
    pub supplies: Supplies,
    pub default_tray: Option<String>,
    pub default_paper: Option<String>,
}

/// A printer found on the network that is not (necessarily) installed yet.
#[derive(Debug, Clone, Serialize, Type)]
pub struct DiscoveredPrinter {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub kind: PrinterKind,
    /// Discovery transport, e.g. `mDNS · IPP`.
    pub via: String,
}

/// List every printer the OS spooler knows about, with capabilities.
#[tauri::command]
#[specta::specta]
pub async fn list_printers() -> Result<Vec<Printer>, IpcError> {
    run_blocking(crate::platform::list_printers).await
}

/// The OS default printer's id, or `None` when no default is configured.
#[tauri::command]
#[specta::specta]
pub async fn default_printer() -> Result<Option<String>, IpcError> {
    run_blocking(crate::platform::default_printer).await
}

/// Best-effort network discovery of not-yet-installed printers (mDNS/Bonjour
/// on Unix; empty on Windows until WSD lands).
#[tauri::command]
#[specta::specta]
pub async fn discover_printers() -> Result<Vec<DiscoveredPrinter>, IpcError> {
    run_blocking(crate::platform::discover_printers).await
}

// ---------------------------------------------------------------------------
// Preferences (persisted MaeclyPrint-level state) + OS actions
// ---------------------------------------------------------------------------

/// Read the persisted MaeclyPrint printer preferences.
#[tauri::command]
#[specta::specta]
pub async fn get_printer_prefs() -> Result<crate::settings::PrinterPrefs, IpcError> {
    run_blocking(|| Ok(crate::settings::Config::load_or_default().printers)).await
}

/// Replace the persisted printer preferences and return the saved value.
#[tauri::command]
#[specta::specta]
pub async fn set_printer_prefs(
    prefs: crate::settings::PrinterPrefs,
) -> Result<crate::settings::PrinterPrefs, IpcError> {
    // Uses the shared `ipc::run_blocking` (typed `IpcError`) rather than this
    // module's `anyhow`-based helper, so the corrupt-config refusal keeps its
    // exact message and code.
    super::run_blocking(move || {
        set_printer_prefs_at(&crate::settings::Config::config_path(), prefs)
    })
    .await
}

/// Path-injected core of [`set_printer_prefs`]. `pub` so the integration tests
/// can point it at a tempdir; not part of the binding surface.
///
/// Loads through [`crate::settings::Config::load_strict`], so a corrupt
/// `config.toml` is refused instead of being silently rewritten as
/// defaults + these prefs (which would erase the engine/updater/telemetry
/// sections and the app default-profile).
pub fn set_printer_prefs_at(
    config_path: &Path,
    prefs: crate::settings::PrinterPrefs,
) -> Result<crate::settings::PrinterPrefs, IpcError> {
    use crate::settings::{Config, CONFIG_CORRUPT_MSG, CONFIG_LOCK};

    // Serialize the whole load-mutate-save against other config writers so a
    // concurrent settings/profile write can't clobber the printers section.
    let _guard = CONFIG_LOCK.lock().unwrap();
    let mut cfg = Config::load_strict(config_path)
        .map_err(|_| IpcError::Internal(CONFIG_CORRUPT_MSG.to_string()))?;
    cfg.printers = prefs;
    cfg.save_to(config_path)?;
    Ok(cfg.printers)
}

// ---------------------------------------------------------------------------
// File-type routing (`PrinterPrefs::routes`)
// ---------------------------------------------------------------------------

/// Normalize an extension or a file name to the `".ext"` key shape the Printers
/// screen writes into [`crate::settings::PrinterPrefs::routes`].
///
/// Accepts `pdf`, `.PDF`, `*.pdf` and `C:\a\Report.PDF` alike. A dotless token
/// with no path separator is read as the extension itself (that is how the
/// queue stores `Job::input_ext`). Returns `None` when there is no usable
/// extension — an extension-less *path* has no route rather than matching an
/// empty key.
pub fn route_key(file_name_or_ext: &str) -> Option<String> {
    let raw = file_name_or_ext.trim().trim_start_matches('*');
    let is_path = raw.contains('/') || raw.contains('\\');
    // Take the last dot-separated segment of the *file name* only, so a dotted
    // directory ("/home/a.b/README") cannot masquerade as an extension.
    let name = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let ext = match name.rsplit_once('.') {
        Some((_, e)) => e,
        None if is_path => return None,
        None => name,
    };
    let ext = ext.trim().to_ascii_lowercase();
    if ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(format!(".{ext}"))
}

/// The printer a file-type routing rule points at, if the user made one.
///
/// This is the read side of the "Route file types here" block on the Printers
/// screen: `docs/05-features/printers-management.md` § Detail sections says
/// those extensions "default to this printer when no profile overrides it".
pub fn routed_printer(
    prefs: &crate::settings::PrinterPrefs,
    file_name_or_ext: &str,
) -> Option<String> {
    let key = route_key(file_name_or_ext)?;
    prefs
        .routes
        .get(&key)
        .filter(|p| !p.is_empty() && !is_disabled(prefs, p))
        .cloned()
}

/// Whether `printer` is disabled in MaeclyPrint (`PrinterPrefs::disabled`).
/// Disabled printers stay installed in the OS; MaeclyPrint just never sends
/// them anything.
pub fn is_disabled(prefs: &crate::settings::PrinterPrefs, printer: &str) -> bool {
    prefs.disabled.iter().any(|d| d == printer)
}

/// Refuse `printer` when it is disabled in MaeclyPrint. Called on the printer
/// a job actually resolved to — an explicit request, or the OS default — since
/// routing and the app default already skip disabled printers on their own
/// ([`resolve_app_printer`]).
pub fn ensure_enabled(
    prefs: &crate::settings::PrinterPrefs,
    printer: &str,
) -> Result<(), crate::engine::print::PrintError> {
    if is_disabled(prefs, printer) {
        return Err(crate::engine::print::PrintError::PrinterDisabled {
            printer: printer.to_string(),
        });
    }
    Ok(())
}

/// The MaeclyPrint-level printer choice for one input file, in spec order:
///
/// 1. an explicit request (a profile's printer, `--printer`, the Print Job
///    dropdown) — a deliberate choice always wins, even a disabled printer, so
///    that the caller can refuse it *by name* ([`ensure_enabled`]) instead of
///    silently printing somewhere else;
/// 2. the file-type routing rule for the input's extension, unless it points
///    at a printer disabled in MaeclyPrint;
/// 3. the app default printer (`PrinterPrefs::default_printer`), unless it is
///    disabled.
///
/// Routing never picks a disabled printer: a rule or default naming one is
/// skipped as if it were not set.
///
/// `None` means "nothing at the app level applies" and the caller should fall
/// back to the **OS** default, which is not this layer's business to read.
///
/// This function is the single place that ordering is defined; the print path
/// (`queue::dispatcher::EngineRunner::resolve_printer` and
/// `cli::print::resolve_printer`) is expected to call it instead of reading
/// `prefs.default_printer` directly, so the GUI, the CLI and the queue can
/// never disagree about where a `.pdf` goes.
pub fn resolve_app_printer(
    prefs: &crate::settings::PrinterPrefs,
    requested: Option<&str>,
    file_name_or_ext: &str,
) -> Option<String> {
    if let Some(p) = requested.map(str::trim).filter(|p| !p.is_empty()) {
        return Some(p.to_string());
    }
    if let Some(p) = routed_printer(prefs, file_name_or_ext) {
        return Some(p);
    }
    prefs
        .default_printer
        .as_deref()
        .filter(|p| !p.is_empty() && !is_disabled(prefs, p))
        .map(str::to_string)
}

/// Set the OS user-default printer (no elevation required).
#[tauri::command]
#[specta::specta]
pub async fn set_default_printer(name: String) -> Result<(), IpcError> {
    run_blocking(move || crate::platform::set_os_default(&name)).await
}

/// Send a test page to the named printer.
#[tauri::command]
#[specta::specta]
pub async fn print_test_page(name: String) -> Result<(), IpcError> {
    run_blocking(move || crate::platform::print_test_page(&name)).await
}

/// Open the OS "Printers" settings pane so the user can add a printer manually.
#[tauri::command]
#[specta::specta]
pub async fn open_printer_settings() -> Result<(), IpcError> {
    run_blocking(crate::platform::open_printer_settings).await
}

/// Install a printer into the OS spooler from a device URI.
///
/// `name` is the human label; `uri` selects the transport (driverless IPP/IPPS
/// or a raw socket/LPD on Unix; IPP/IPPS on Windows). On failure the spooler's
/// own diagnostic (e.g. lpadmin's "Unable to connect to …") is surfaced so the
/// UI can show it verbatim. Never mutates MaeclyPrint preferences — only the OS.
#[tauri::command]
#[specta::specta]
pub async fn add_printer(name: String, uri: String) -> Result<(), IpcError> {
    run_blocking(move || crate::platform::add_printer(&name, &uri)).await
}

/// Read the effective organization printer policy (`policy.json`).
///
/// Returns the default [`crate::settings::PrinterPolicy`] when no policy file is
/// present or it is malformed; the loader never errors, so the UI can rely on a
/// value always coming back.
#[tauri::command]
#[specta::specta]
pub async fn get_printer_policy() -> Result<crate::settings::PrinterPolicy, IpcError> {
    run_blocking(|| Ok(crate::settings::PrinterPolicy::load())).await
}

/// Run a blocking enumeration closure off the IPC thread so the webview never
/// stalls on a slow spooler, normalizing both the join error and the inner
/// `anyhow` error into [`IpcError`].
async fn run_blocking<T, F>(f: F) -> Result<T, IpcError>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let joined = tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::Internal(format!("printer task panicked: {e}")))?;
    joined.map_err(IpcError::from)
}

// ---------------------------------------------------------------------------
// Tests
//
// Everything here is hermetic: the persisted-prefs cases run against a tempdir
// through the path-injected `set_printer_prefs_at`, and the resolution cases
// are pure. The `#[tauri::command]` wrappers themselves are one-line
// delegations to `crate::platform::*`, which is covered by its own tests, so
// they are not re-tested through a Tauri runtime here.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Config, PrinterOverride, PrinterPrefs};

    fn prefs_with_routes(pairs: &[(&str, &str)]) -> PrinterPrefs {
        let mut prefs = PrinterPrefs::default();
        for (ext, printer) in pairs {
            prefs
                .routes
                .insert((*ext).to_string(), (*printer).to_string());
        }
        prefs
    }

    #[test]
    fn fr_route_key_normalizes_every_shape_the_ui_can_produce() {
        for input in ["pdf", ".pdf", "*.pdf", ".PDF", "  .Pdf  "] {
            assert_eq!(route_key(input).as_deref(), Some(".pdf"), "{input}");
        }
        assert_eq!(
            route_key("/Users/ana/Q3 Report.DOCX").as_deref(),
            Some(".docx")
        );
        assert_eq!(
            route_key(r"C:\Users\ana\Invoice.xlsx").as_deref(),
            Some(".xlsx")
        );
        // No usable extension → no key (never an empty ".").
        assert_eq!(route_key(""), None);
        assert_eq!(route_key("archive.tar.gz").as_deref(), Some(".gz"));
        // A dotted directory must not be mistaken for an extension, and an
        // extension-less path has no route at all.
        assert_eq!(route_key("/home/a.b/README"), None);
        assert_eq!(route_key(r"C:\docs\README"), None);
        // Punctuation is not an extension.
        assert_eq!(route_key("weird.p df"), None);
    }

    #[test]
    fn fr_routed_printer_matches_case_insensitively_and_ignores_blanks() {
        let prefs = prefs_with_routes(&[(".pdf", "EPSON_L15160"), (".jpg", "")]);
        assert_eq!(
            routed_printer(&prefs, "Report.PDF").as_deref(),
            Some("EPSON_L15160")
        );
        assert_eq!(
            routed_printer(&prefs, "pdf").as_deref(),
            Some("EPSON_L15160")
        );
        // A rule with an empty target is not a rule.
        assert_eq!(routed_printer(&prefs, "photo.jpg"), None);
        // No rule for this type.
        assert_eq!(routed_printer(&prefs, "notes.txt"), None);
    }

    #[test]
    fn fr_resolve_app_printer_follows_request_then_route_then_default() {
        let mut prefs = prefs_with_routes(&[(".pdf", "ROUTED")]);
        prefs.default_printer = Some("APP_DEFAULT".into());

        // 1. An explicit request always wins, even when a route exists.
        assert_eq!(
            resolve_app_printer(&prefs, Some("CHOSEN"), "a.pdf").as_deref(),
            Some("CHOSEN")
        );
        // 2. Otherwise the file-type route decides.
        assert_eq!(
            resolve_app_printer(&prefs, None, "a.pdf").as_deref(),
            Some("ROUTED")
        );
        // 3. Otherwise the app default.
        assert_eq!(
            resolve_app_printer(&prefs, None, "a.docx").as_deref(),
            Some("APP_DEFAULT")
        );
        // Blank/whitespace requests are not requests.
        assert_eq!(
            resolve_app_printer(&prefs, Some("   "), "a.pdf").as_deref(),
            Some("ROUTED")
        );
    }

    #[test]
    fn fr_resolve_app_printer_defers_to_the_os_when_nothing_applies() {
        let prefs = PrinterPrefs::default();
        assert_eq!(resolve_app_printer(&prefs, None, "a.pdf"), None);
        // An empty app default is not a default.
        let prefs = PrinterPrefs {
            default_printer: Some(String::new()),
            ..PrinterPrefs::default()
        };
        assert_eq!(resolve_app_printer(&prefs, None, "a.pdf"), None);
    }

    /// A printer disabled in MaeclyPrint is never chosen by routing or as the
    /// app default; an explicit request for it comes back by name so the
    /// caller can refuse it with a reason.
    #[test]
    fn fr_disabled_printers_are_never_routed_to_and_refused_by_name() {
        let mut prefs = prefs_with_routes(&[(".pdf", "OFF")]);
        prefs.default_printer = Some("OFF".into());
        prefs.disabled = vec!["OFF".into()];

        assert_eq!(routed_printer(&prefs, "a.pdf"), None);
        assert_eq!(
            resolve_app_printer(&prefs, None, "a.pdf"),
            None,
            "falls to the OS"
        );

        prefs.default_printer = Some("ON".into());
        assert_eq!(
            resolve_app_printer(&prefs, None, "a.pdf").as_deref(),
            Some("ON"),
            "a disabled route falls through to the enabled default"
        );

        // An explicit request is returned as asked — and then refused.
        assert_eq!(
            resolve_app_printer(&prefs, Some("OFF"), "a.pdf").as_deref(),
            Some("OFF")
        );
        match ensure_enabled(&prefs, "OFF") {
            Err(crate::engine::print::PrintError::PrinterDisabled { printer }) => {
                assert_eq!(printer, "OFF")
            }
            other => panic!("expected PrinterDisabled, got {other:?}"),
        }
        assert!(ensure_enabled(&prefs, "ON").is_ok());
    }

    /// Happy path of the only command in this module with logic of its own:
    /// writing the printers section must round-trip AND leave every other
    /// section of `config.toml` untouched.
    #[test]
    fn fr_set_printer_prefs_at_round_trips_and_preserves_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Seed a config whose non-printer sections carry non-default values.
        let mut seed = Config::default();
        seed.app.locale = "es-ES".into();
        seed.engine.max_concurrent_conversions = 7;
        seed.updater.check_on_startup = false;
        seed.save_to(&path).unwrap();

        let mut prefs = prefs_with_routes(&[(".pdf", "EPSON_L15160")]);
        prefs.default_printer = Some("EPSON_L15160".into());
        prefs.disabled = vec!["HL_L2350".into()];
        prefs.overrides.insert(
            "EPSON_L15160".into(),
            PrinterOverride {
                tray: Some("Tray 2".into()),
                paper: Some("Letter".into()),
            },
        );

        let saved = set_printer_prefs_at(&path, prefs.clone()).expect("prefs should persist");
        assert_eq!(saved, prefs, "the saved value is returned verbatim");

        let reloaded = Config::load_from(&path);
        assert_eq!(reloaded.printers, prefs, "prefs survive a reload");
        assert_eq!(reloaded.app.locale, "es-ES", "app section preserved");
        assert_eq!(
            reloaded.engine.max_concurrent_conversions, 7,
            "engine section preserved"
        );
        assert!(!reloaded.updater.check_on_startup, "updater preserved");
    }

    /// Writing prefs twice must replace, not merge: clearing a route from the
    /// UI has to actually remove it from disk.
    #[test]
    fn fr_regression_set_printer_prefs_at_replaces_rather_than_merges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::default().save_to(&path).unwrap();

        set_printer_prefs_at(&path, prefs_with_routes(&[(".pdf", "A"), (".jpg", "B")])).unwrap();
        set_printer_prefs_at(&path, prefs_with_routes(&[(".pdf", "A")])).unwrap();

        let reloaded = Config::load_from(&path);
        assert!(reloaded.printers.routes.contains_key(".pdf"));
        assert!(
            !reloaded.printers.routes.contains_key(".jpg"),
            "a removed rule must not survive on disk"
        );
    }

    /// The policy loader is documented as never erroring, because the UI relies
    /// on always getting a value back (`get_printer_policy`).
    #[test]
    fn fr_printer_policy_load_always_yields_a_value() {
        let policy = crate::settings::PrinterPolicy::load();
        // No policy.json on a test machine → no pinned default.
        let _ = policy.default_printer;
    }
}
