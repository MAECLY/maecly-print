//! Global app settings (`config.toml`).
//!
//! Loads/saves the file with atomic writes. The `printers` section is exposed
//! to the frontend via the `get_printer_prefs` / `set_printer_prefs` IPC
//! commands (see `ipc::printers`), and holds Pluryprint-level printer state
//! that must survive restarts: the app default, disabled printers, file-type
//! routing rules, and per-printer tray/paper overrides.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::util::paths;

pub mod locale;
pub mod policy;

pub use policy::PrinterPolicy;

/// Current `config.toml` format version, written into every saved file.
pub const SCHEMA_VERSION: u32 = 1;

/// Refusal message returned by every `config.toml` mutator when the file exists
/// but cannot be parsed. Shared so the three mutators
/// ([`crate::ipc::settings::set_app_settings`],
/// [`crate::ipc::printers::set_printer_prefs`],
/// [`crate::ipc::profiles::set_default_profile`]) speak with one voice: they
/// refuse rather than silently rewrite a corrupt file as defaults+patch (which
/// would erase the user's printer prefs/routes/overrides).
pub(crate) const CONFIG_CORRUPT_MSG: &str =
    "config.toml could not be parsed — fix or remove it; your settings were not modified";

/// The [`CONFIG_CORRUPT_MSG`] refusal as a localizable IPC error (FR-092): the
/// UI translates the stable `config_corrupt` reason.
pub(crate) fn config_corrupt_error() -> crate::util::error::IpcError {
    crate::util::error::IpcError::localized(
        crate::util::error::IpcErrorCode::Internal,
        "config_corrupt",
        CONFIG_CORRUPT_MSG,
    )
}

/// Serializes every read-modify-write of `config.toml`. Any command that mutates
/// the file — [`crate::ipc::printers::set_printer_prefs`],
/// [`crate::ipc::settings::set_app_settings`],
/// [`crate::ipc::profiles::set_default_profile`] — takes this guard so a
/// concurrent write can't clobber a section another command just wrote. Held
/// only across the synchronous load→mutate→save (those run inside
/// `spawn_blocking`, never across an `.await`), so it never blocks the async
/// runtime.
///
/// **This mutex is intra-process only**, and since the CLI shipped there is a
/// second process writing the same file (`pluryprint update channel beta` is a
/// full read-modify-write of `config.toml`). The cross-process half of the
/// problem is handled at the write itself, by [`Config::save_to`]'s
/// section-wise reconciliation — see its documentation for the trade-off.
///
/// **Lock ordering:** when a command needs both this and
/// [`crate::profiles::PROFILES_LOCK`], it must acquire `PROFILES_LOCK` **before**
/// `CONFIG_LOCK` (the only such command is
/// [`crate::ipc::profiles::set_default_profile`]). No path takes them in the
/// other order, so the two can't deadlock.
pub(crate) static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The whole `config.toml`. Every section beyond `schema_version` is
/// `#[serde(default)]` so a file written by an older build (which lacked the
/// section entirely) still loads, filling the missing sections with defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Format version the file was written with (see [`SCHEMA_VERSION`]).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// `[app]`: UI and launch preferences.
    #[serde(default)]
    pub app: AppCfg,
    /// `[printers]`: Pluryprint's own printer preferences.
    #[serde(default)]
    pub printers: PrinterPrefs,
    /// `[engine]`: conversion and printing limits.
    #[serde(default)]
    pub engine: EngineCfg,
    /// `[updater]`: update-check preferences.
    #[serde(default)]
    pub updater: UpdaterCfg,
    /// `[telemetry]`: consent flags (nothing is transmitted).
    #[serde(default)]
    pub telemetry: TelemetryCfg,
    /// What [`Config::load_strict`] read off disk, kept so [`Config::save_to`]
    /// can tell the sections this caller actually changed from the ones it is
    /// merely carrying. Never serialized, never part of equality — it is
    /// bookkeeping about *this* value, not configuration. `None` (every other
    /// constructor) means "write me wholesale", which is the old behaviour.
    #[serde(skip)]
    baseline: Option<Box<Config>>,
}

/// Two configs are equal when their *settings* are, whatever either one
/// remembers about where it came from. Hand-written rather than derived so the
/// [`Config::baseline`] field cannot make a round-trip assertion fail.
impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.app == other.app
            && self.printers == other.printers
            && self.engine == other.engine
            && self.updater == other.updater
            && self.telemetry == other.telemetry
    }
}

/// How a stored `0` is treated for one knob, on top of the `min..=max` range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ZeroPolicy {
    /// `0` is just an out-of-range value: rejected on write, clamped up to
    /// `min` when a hand-edited file is sanitized.
    Clamp,
    /// `0` is a meaningful value that turns the knob off (the timeout), so it
    /// is accepted on write and passes through sanitization untouched.
    Disables,
    /// `0` is bricked input that would make the app unusable (a 0 MB ceiling
    /// rejects every file): rejected on write, and sanitized to `default`.
    #[serde(rename = "default")]
    FallsBackToDefault,
}

/// Inclusive bounds and the shipped default of one numeric engine knob.
///
/// This is the SINGLE SOURCE OF TRUTH for the engine limits (see
/// [`ENGINE_LIMITS`]): [`EngineCfg::default`] takes its values from here,
/// [`EngineCfg::sanitized`] clamps with [`Bounds::sanitize`], the write path
/// ([`crate::ipc::settings::set_app_settings`]) rejects with
/// [`Bounds::accepts`], and the very same numbers ride to the frontend in
/// [`SettingsMeta::limits`] so the Settings screen validates against the
/// server's bounds instead of a hand-copied second set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Bounds {
    /// Smallest accepted value (ignoring [`ZeroPolicy`]).
    pub min: u32,
    /// Largest accepted value.
    pub max: u32,
    /// The value a fresh install starts from, and the fallback for a bricked
    /// `0` under [`ZeroPolicy::FallsBackToDefault`].
    pub default: u32,
    /// What a stored `0` means for this knob.
    pub zero: ZeroPolicy,
}

impl Bounds {
    /// Whether the write path accepts `value` verbatim.
    pub fn accepts(&self, value: u32) -> bool {
        (value == 0 && matches!(self.zero, ZeroPolicy::Disables))
            || (self.min..=self.max).contains(&value)
    }

    /// `value` forced into a usable range, honouring this knob's
    /// [`ZeroPolicy`]. Used for values that never went through the write path
    /// (a hand-edited file, or one written by an older build).
    pub fn sanitize(&self, value: u32) -> u32 {
        match (value, self.zero) {
            (0, ZeroPolicy::Disables) => 0,
            (0, ZeroPolicy::FallsBackToDefault) => self.default,
            _ => value.clamp(self.min, self.max),
        }
    }
}

/// The bounds of every numeric engine knob, as one bundle. Exposed on the wire
/// so the UI can render and validate the limits without duplicating them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EngineLimits {
    /// Bounds of [`EngineCfg::max_concurrent_conversions`].
    pub max_concurrent_conversions: Bounds,
    /// Bounds of [`EngineCfg::conversion_timeout_seconds`].
    pub conversion_timeout_seconds: Bounds,
    /// Bounds of [`EngineCfg::max_file_size_mb`].
    pub max_file_size_mb: Bounds,
    /// How long one print may take, from spooling until the OS job leaves the
    /// queue. Separate from the conversion budget: a print waits on paper and a
    /// physical device, so its natural length has nothing to do with how long a
    /// rasterisation may take.
    pub print_timeout_seconds: Bounds,
}

/// The engine limits. Documented in `docs/05-features/settings.md` § Engine;
/// every other copy in the codebase (validation, sanitization, the UI's inline
/// bounds check and error text) is derived from this constant.
pub const ENGINE_LIMITS: EngineLimits = EngineLimits {
    max_concurrent_conversions: Bounds {
        min: 1,
        max: 8,
        default: crate::queue::DEFAULT_MAX_CONCURRENT_CONVERSIONS as u32,
        zero: ZeroPolicy::Clamp,
    },
    conversion_timeout_seconds: Bounds {
        min: 10,
        max: 3600,
        default: 60,
        zero: ZeroPolicy::Disables,
    },
    max_file_size_mb: Bounds {
        min: 1,
        max: 4096,
        default: 200,
        zero: ZeroPolicy::FallsBackToDefault,
    },
    print_timeout_seconds: Bounds {
        min: 10,
        max: 3600,
        default: 300,
        zero: ZeroPolicy::Disables,
    },
};

impl Default for EngineLimits {
    fn default() -> Self {
        ENGINE_LIMITS
    }
}

/// Engine/queue tuning knobs. The concurrency cap and both timeouts are pushed
/// to the running dispatcher when they are saved (`set_app_settings` sends it a
/// `SetLimits` command), so they apply to the next conversion or print without
/// a restart; the size ceiling is read on every submit/inspect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EngineCfg {
    /// Concurrent conversions the dispatcher may run (clamped to
    /// [`ENGINE_LIMITS`] at boot). Defaults to the dispatcher's own
    /// [`DEFAULT_MAX_CONCURRENT_CONVERSIONS`](crate::queue::DEFAULT_MAX_CONCURRENT_CONVERSIONS).
    pub max_concurrent_conversions: u32,
    /// Per-conversion timeout in seconds; `0` disables the timeout.
    pub conversion_timeout_seconds: u32,
    /// Largest input file accepted for submission/inspection, in megabytes.
    pub max_file_size_mb: u32,
    /// Wall-clock budget for one print, in seconds — from handing the PDF to
    /// the spooler until the OS job leaves the queue; `0` disables it.
    /// `#[serde(default)]` so a `config.toml` written before this knob existed
    /// still loads, with the shipped default.
    #[serde(default = "default_print_timeout_seconds")]
    pub print_timeout_seconds: u32,
    /// Profile id applied by default in the print dialog. `None` means
    /// `builtin-quick`. Must skip-serialize when `None`: `toml` errors trying to
    /// serialize a bare `Option::None` value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    /// **Retired** (round 6): the value of the removed `[engine]
    /// html_renderer` key, when an older build's `config.toml` still has it.
    /// HTML, Markdown and text always print through the system web view now
    /// (the headless-browser fallback is gone, ADR-0012), so the engine never
    /// reads this. It is kept on disk only until the app has told the user
    /// once ([`take_retired_html_renderer_at`]); a settings save preserves it
    /// until then. Not on the wire (`specta(skip)`).
    #[serde(
        default,
        rename = "html_renderer",
        skip_serializing_if = "Option::is_none"
    )]
    #[specta(skip)]
    pub retired_html_renderer: Option<String>,
}

impl Default for EngineCfg {
    fn default() -> Self {
        Self {
            max_concurrent_conversions: ENGINE_LIMITS.max_concurrent_conversions.default,
            conversion_timeout_seconds: ENGINE_LIMITS.conversion_timeout_seconds.default,
            max_file_size_mb: ENGINE_LIMITS.max_file_size_mb.default,
            print_timeout_seconds: ENGINE_LIMITS.print_timeout_seconds.default,
            default_profile: None,
            retired_html_renderer: None,
        }
    }
}

fn default_print_timeout_seconds() -> u32 {
    ENGINE_LIMITS.print_timeout_seconds.default
}

impl EngineCfg {
    /// A copy with every tuning knob clamped to a usable range, so a
    /// hand-edited (or downgrade-corrupted) `config.toml` can't brick the
    /// engine. `set_app_settings` validates these bounds on the write path, but
    /// a value written directly to disk — or by an older build — bypasses that,
    /// so every *consumer* (queue spawn, submit's size check, `inspect_files`)
    /// reads through this rather than trusting the raw stored numbers.
    ///
    /// Every knob goes through its own [`Bounds::sanitize`], so the ranges and
    /// the `0` handling come from [`ENGINE_LIMITS`] rather than a second set of
    /// literals: concurrency clamps into range, a `0` timeout stays `0`
    /// (disabled) while any other value clamps (a 1s timeout would kill every
    /// conversion), and a `0` size ceiling falls back to the default (`0` would
    /// reject every file).
    ///
    /// `default_profile` is passed through unchanged.
    pub fn sanitized(&self) -> EngineCfg {
        EngineCfg {
            max_concurrent_conversions: ENGINE_LIMITS
                .max_concurrent_conversions
                .sanitize(self.max_concurrent_conversions),
            conversion_timeout_seconds: ENGINE_LIMITS
                .conversion_timeout_seconds
                .sanitize(self.conversion_timeout_seconds),
            max_file_size_mb: ENGINE_LIMITS
                .max_file_size_mb
                .sanitize(self.max_file_size_mb),
            print_timeout_seconds: ENGINE_LIMITS
                .print_timeout_seconds
                .sanitize(self.print_timeout_seconds),
            default_profile: self.default_profile.clone(),
            retired_html_renderer: self.retired_html_renderer.clone(),
        }
    }
}

/// Update-check preferences. `channel` picks the release manifest the check
/// and the install read (FR-085, `update::install::default_endpoint`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UpdaterCfg {
    /// Release stream to follow.
    pub channel: UpdateChannel,
    /// Check for an update once at startup.
    pub check_on_startup: bool,
}

impl Default for UpdaterCfg {
    fn default() -> Self {
        Self {
            channel: UpdateChannel::Stable,
            check_on_startup: true,
        }
    }
}

/// Which release stream the user follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Final releases only (the default).
    #[default]
    Stable,
    /// Beta pre-releases.
    Beta,
    /// Nightly builds. None are published: this channel reads the beta
    /// manifest.
    Nightly,
}

/// Telemetry consent flags. These record the user's *consent*; this version
/// transmits nothing regardless of their value — no crash reports and no usage
/// metrics leave the machine. They exist so the choice survives restarts and a
/// future build can honour it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TelemetryCfg {
    /// Consent to sending crash reports.
    #[serde(default)]
    pub crash_reports: bool,
    /// Consent to sending anonymous usage metrics.
    #[serde(default)]
    pub anonymous_usage_metrics: bool,
}

/// Read-only annotations the server attaches to every settings response: state
/// the UI cannot derive from the values themselves.
///
/// It travels *inside* [`AppSettings`] so the screen learns about it on the one
/// read it already does. The client echoes it back on a write (it sends the
/// whole bundle) and the server **ignores it entirely** — it is recomputed from
/// disk on the way out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SettingsMeta {
    /// `config.toml` exists but could not be read or parsed, so the values in
    /// this bundle are [`Config::default`], **not** the user's settings
    /// (FR-101). Every mutator refuses while this holds (see
    /// [`CONFIG_CORRUPT_MSG`]), so the UI must say so instead of presenting
    /// defaults as if they were the stored configuration.
    pub config_unreadable: bool,
    /// Absolute path of the config file the values came from, so the UI can
    /// name the file the user has to fix or remove.
    pub config_path: String,
    /// The engine bounds ([`ENGINE_LIMITS`]) the write path enforces, so the
    /// client validates against the server's numbers instead of its own copy.
    pub limits: EngineLimits,
}

impl Default for SettingsMeta {
    fn default() -> Self {
        Self {
            config_unreadable: false,
            config_path: String::new(),
            limits: ENGINE_LIMITS,
        }
    }
}

/// The subset of [`Config`] exposed to the frontend as one bundle by
/// `get_app_settings` / `set_app_settings`. Deliberately excludes `printers`
/// (owned by the Printers screen) and `schema_version` (server-managed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct AppSettings {
    /// The `[app]` section.
    pub app: AppCfg,
    /// The `[engine]` section.
    pub engine: EngineCfg,
    /// The `[updater]` section.
    pub updater: UpdaterCfg,
    /// The `[telemetry]` section.
    pub telemetry: TelemetryCfg,
    /// Server-computed annotations; `#[serde(default)]` so a client that does
    /// not know about them (or a test building the bundle by hand) still
    /// deserializes, and so the field is optional in the generated TypeScript.
    #[serde(default)]
    pub meta: SettingsMeta,
}

/// Pluryprint-level printer preferences (never mutate the OS spooler).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PrinterPrefs {
    /// App default printer id; falls back to the OS default when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_printer: Option<String>,
    /// Printer ids disabled *in Pluryprint* — still installed in the OS.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// File-type routing: extension (e.g. ".pdf") → printer id.
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    /// Per-printer default tray/paper overrides, keyed by printer id.
    #[serde(default)]
    pub overrides: BTreeMap<String, PrinterOverride>,
}

/// One printer's default tray and paper, applied when a job leaves them unset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PrinterOverride {
    /// Tray to use by default, or `None` for the printer's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tray: Option<String>,
    /// Paper size to use by default, or `None` for the printer's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

/// The `[app]` section: UI and launch preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppCfg {
    /// UI language as a BCP 47 tag, e.g. `"en-US"`.
    pub locale: String,
    /// Colour scheme of the window.
    pub theme: Theme,
    /// Start with the main window minimized (a plain launch only; see
    /// [`should_start_minimized`]).
    pub launch_minimized: bool,
    /// UI spacing preference. `#[serde(default)]` so a config written before this
    /// field existed still loads (filling in [`Density::Comfortable`]).
    #[serde(default)]
    pub density: Density,
    /// FR-072: announce finished, failed and stuck batches through the OS
    /// notification centre while the window is in the background. On by
    /// default; `#[serde(default)]` so a config written before this field
    /// existed keeps notifications on.
    #[serde(default = "default_native_notifications")]
    pub native_notifications: bool,
    /// FR-093: the first-run OS-language detection is settled — it ran, the
    /// user picked a language, or the locale came over in an upgrade from the
    /// former product (ADR-0009) — so detection must never change `locale`
    /// again. Kept here rather than in the webview's storage, which a new
    /// bundle identifier starts empty.
    #[serde(default)]
    pub locale_detected: bool,
    /// The first-run tour was completed or skipped (or this is an upgrade from
    /// the former product). Kept here for the same reason as `locale_detected`.
    #[serde(default)]
    pub onboarding_complete: bool,
}

fn default_native_notifications() -> bool {
    true
}

impl Default for AppCfg {
    fn default() -> Self {
        Self {
            locale: "en-US".into(),
            theme: Theme::System,
            launch_minimized: false,
            density: Density::default(),
            native_notifications: default_native_notifications(),
            locale_detected: false,
            onboarding_complete: false,
        }
    }
}

/// Colour scheme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Always light.
    Light,
    /// Always dark.
    Dark,
    /// Follow the OS appearance.
    System,
}

/// UI spacing density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    /// Default spacing.
    #[default]
    Comfortable,
    /// Tighter spacing, for more rows on screen.
    Compact,
}

/// The result of a tolerant load ([`Config::load_reporting`]): the config to
/// use, plus whether it is a stand-in for a file that could not be read.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigLoad {
    /// The configuration to use.
    pub config: Config,
    /// The file exists but could not be read/parsed; `config` is the default.
    pub unreadable: bool,
}

impl ConfigLoad {
    fn intact(config: Config) -> Self {
        Self {
            config,
            unreadable: false,
        }
    }

    fn degraded() -> Self {
        Self {
            config: Config::default(),
            unreadable: true,
        }
    }
}

/// Whether the main window should start minimized (`[app].launch_minimized`).
///
/// Startup reads the config directly rather than waiting for the frontend: the
/// window has to be minimized before it is shown, which is long before the
/// first IPC round-trip. Path-injected core for the tests; [`launch_minimized`]
/// is what the app calls.
pub fn launch_minimized_at(config_path: &Path) -> bool {
    Config::load_from(config_path).app.launch_minimized
}

/// [`launch_minimized_at`] against the real per-user config. A corrupt or
/// missing file yields the default (`false`) — never a window the user cannot
/// find.
pub fn launch_minimized() -> bool {
    launch_minimized_at(&Config::config_path())
}

/// Whether this launch should actually minimize the main window.
///
/// The preference only applies to a plain launch. A launch that carries a file
/// selection (Explorer/Finder context menu, "open with") exists to show the
/// user that selection — minimizing it would make the right-click appear to do
/// nothing — so the window opens normally then.
pub fn should_start_minimized(launch_minimized: bool, has_selection: bool) -> bool {
    launch_minimized && !has_selection
}

/// The one-time migration of the retired `[engine] html_renderer` key
/// (round 6): remove it from `config_path` and say whether it named the
/// headless browser (`"browser"`, any case), the case the app tells the user
/// about once — their pages now print with the system web view. Any other
/// value is removed silently, and a missing, unreadable or key-less file is
/// left alone (`false`). Takes [`CONFIG_LOCK`], so the caller must not hold
/// it.
pub fn take_retired_html_renderer_at(config_path: &Path) -> bool {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let Ok(mut cfg) = Config::load_strict(config_path) else {
        return false;
    };
    let Some(value) = cfg.engine.retired_html_renderer.take() else {
        return false;
    };
    if let Err(e) = cfg.save_to(config_path) {
        // Not removed: the notice comes back on the next launch instead.
        tracing::warn!(event = "retired_html_renderer_not_removed", error = %e);
        return false;
    }
    let was_browser = value.trim().eq_ignore_ascii_case("browser");
    tracing::info!(event = "retired_html_renderer_removed", was_browser);
    was_browser
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            app: AppCfg::default(),
            printers: PrinterPrefs::default(),
            engine: EngineCfg::default(),
            updater: UpdaterCfg::default(),
            telemetry: TelemetryCfg::default(),
            baseline: None,
        }
    }
}

impl Config {
    /// Whether this config holds nothing a user chose: the defaults, plus
    /// what a first run writes for itself (the locale detected from the OS
    /// and the first-run flags). The data migration (`util::legacy`) lets old
    /// data replace such a file.
    pub fn is_first_run_default(&self) -> bool {
        let mut normalized = self.clone();
        let defaults = AppCfg::default();
        normalized.app.locale = defaults.locale;
        normalized.app.locale_detected = defaults.locale_detected;
        normalized.app.onboarding_complete = defaults.onboarding_complete;
        normalized == Config::default()
    }

    /// Absolute path of `config.toml` in the config directory.
    pub fn config_path() -> PathBuf {
        paths::config_dir().join("config.toml")
    }

    /// Load and parse `config.toml` at `path`, degrading to
    /// [`Config::default`] on any read or parse failure (a missing file is the
    /// common, non-error case). Path-injected so integration tests can point at
    /// a tempdir instead of the real config dir.
    ///
    /// Callers that can *tell the user* about a broken file should use
    /// [`Config::load_reporting`] instead: this one throws the fact away.
    pub fn load_from(path: &Path) -> Self {
        Self::load_reporting(path).config
    }

    /// [`Config::load_from`] that also reports whether it degraded.
    ///
    /// The tolerant read path must not fail (the app has to start, and every
    /// screen has to render), but silently showing defaults as if they were the
    /// user's settings is how a corrupt file becomes invisible until the first
    /// save is refused. FR-101 asks for the fallback *and* a recoverable error,
    /// so `get_app_settings` reads through this and forwards `unreadable` to
    /// the UI in [`SettingsMeta::config_unreadable`].
    pub fn load_reporting(path: &Path) -> ConfigLoad {
        if !path.exists() {
            return ConfigLoad::intact(Self::default());
        }
        match std::fs::read_to_string(path) {
            Ok(s) => match toml::from_str::<Self>(&s) {
                Ok(c) => ConfigLoad::intact(c),
                Err(e) => {
                    tracing::warn!(
                        event = "config_parse_failed",
                        error = %e,
                        "falling back to default config"
                    );
                    ConfigLoad::degraded()
                }
            },
            Err(e) => {
                tracing::warn!(event = "config_read_failed", error = %e);
                ConfigLoad::degraded()
            }
        }
    }

    /// Load the real per-user config, or defaults if it is missing/corrupt.
    pub fn load_or_default() -> Self {
        Self::load_from(&Self::config_path())
    }

    /// Load `config.toml` at `path`, but *fail loudly* on a corrupt file instead
    /// of degrading to defaults. A **missing** file is still the normal case
    /// (→ `Ok(Config::default())`); a read or parse error yields `Err(message)`.
    ///
    /// Every mutator loads through this so it can refuse to touch a config it
    /// could not parse — [`Config::load_from`] (which silently defaults) would
    /// otherwise turn a save into "defaults + this one patch", permanently
    /// erasing the sections it did not set (printer prefs, routes, overrides).
    /// Read-only paths keep using [`Config::load_from`]/[`load_or_default`].
    ///
    /// The value it returns remembers what was on disk (see
    /// [`Config::baseline`]), which is what lets [`Config::save_to`] write back
    /// only the sections this caller changed.
    pub fn load_strict(path: &Path) -> Result<Config, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut cfg = toml::from_str::<Self>(&s).map_err(|e| e.to_string())?;
        cfg.baseline = Some(Box::new(cfg.clone()));
        Ok(cfg)
    }

    /// Atomic write to `path`: tempfile in the same directory, then rename.
    /// `tempfile::NamedTempFile::persist` uses MoveFileEx on Windows (atomic on
    /// the same volume) and rename(2) on Unix. Path-injected so tests can write
    /// into a tempdir.
    ///
    /// # Two processes, one file
    ///
    /// [`CONFIG_LOCK`] serializes writers inside one process, but the GUI and
    /// the CLI are two processes: `pluryprint update channel beta` running
    /// against a Settings save is a load→mutate→save on each side, and the
    /// slower one used to write back its whole (now stale) copy — no corruption
    /// (the write is atomic) but a silently lost change.
    ///
    /// When this value came from [`Config::load_strict`], the fix is applied
    /// here: the file is re-read at the last possible moment and only the
    /// sections that differ from the loaded baseline — the ones this caller
    /// actually meant to change — are written over it. A CLI channel change and
    /// a GUI theme change now both survive, in either order.
    ///
    /// **The trade-off, stated honestly.** This is a section-granular merge, not
    /// a lock. It was chosen over an inter-process file lock because a lock adds
    /// a failure mode that is worse than the bug: a crashed or killed process
    /// leaves a lock whose recovery has to be guessed at, advisory locks behave
    /// differently on the network shares roaming Windows profiles live on, and
    /// it would need a second lock ordering next to
    /// [`crate::profiles::PROFILES_LOCK`]. What remains uncovered is (a) two
    /// processes changing the **same** section within the microseconds between
    /// this re-read and the rename, where the last writer still wins — and two
    /// people setting the same knob to different values has no better answer
    /// anyway — and (b) writes from a value that was not loaded through
    /// `load_strict`, which are wholesale by construction and are only made by
    /// first-run/seed paths and tests.
    ///
    /// One consequence callers should know about: after a save, `self` is not
    /// necessarily what is on disk any more — a section this caller did not
    /// change may have been left as another process wrote it. Mutators return
    /// their own value, which is correct for the sections they set; anything
    /// that needs the whole file must re-read it.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let parent = path.parent().expect("config path has a parent directory");
        paths::ensure_dir(parent)?;

        let merged;
        let to_write = match &self.baseline {
            Some(baseline) => {
                merged = self.reconciled_with_disk(path, baseline);
                &merged
            }
            None => self,
        };

        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        let toml_bytes = toml::to_string_pretty(to_write)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        tmp.write_all(toml_bytes.as_bytes())?;
        tmp.flush()?;
        tmp.persist(path)
            .map_err(|e| std::io::Error::other(format!("persist failed: {e}")))?;
        Ok(())
    }

    /// This config's changed sections laid over whatever is in `path` **now**.
    ///
    /// A section counts as changed when it differs from `baseline`, the copy
    /// [`Config::load_strict`] read; every other section is taken from the file
    /// as it stands, which is where another process' concurrent change lives.
    /// A file that has since become unreadable (or was removed) leaves this
    /// value untouched — refusing to save because someone else corrupted the
    /// file would lose the user's change for nothing.
    fn reconciled_with_disk(&self, path: &Path, baseline: &Config) -> Config {
        let Ok(current) = Self::load_strict(path) else {
            return self.clone();
        };
        // `load_strict` returns the default for a missing file, which is exactly
        // the right base: there is nothing of anyone else's to preserve.
        let mut out = current;
        if self.schema_version != baseline.schema_version {
            out.schema_version = self.schema_version;
        }
        if self.app != baseline.app {
            out.app = self.app.clone();
        }
        if self.printers != baseline.printers {
            out.printers = self.printers.clone();
        }
        if self.engine != baseline.engine {
            out.engine = self.engine.clone();
        }
        if self.updater != baseline.updater {
            out.updater = self.updater.clone();
        }
        if self.telemetry != baseline.telemetry {
            out.telemetry = self.telemetry.clone();
        }
        out.baseline = None;
        out
    }

    /// Atomically write to the real per-user config path.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::config_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trip() {
        let c = Config::default();
        let s = toml::to_string_pretty(&c).unwrap();
        let c2: Config = toml::from_str(&s).unwrap();
        assert_eq!(c.schema_version, c2.schema_version);
        assert_eq!(c.app.locale, c2.app.locale);
    }

    #[test]
    fn round_trip_preserves_theme() {
        let mut c = Config::default();
        c.app.theme = Theme::Dark;
        let s = toml::to_string_pretty(&c).unwrap();
        assert!(s.contains("dark"));
    }

    #[test]
    fn printer_prefs_round_trip() {
        let mut c = Config::default();
        c.printers.default_printer = Some("EPSON_L15160".into());
        c.printers.disabled.push("Old_Fax".into());
        c.printers
            .routes
            .insert(".pdf".into(), "EPSON_L15160".into());
        c.printers.overrides.insert(
            "EPSON_L15160".into(),
            PrinterOverride {
                tray: Some("Tray 1".into()),
                paper: Some("A4".into()),
            },
        );
        let s = toml::to_string_pretty(&c).unwrap();
        let c2: Config = toml::from_str(&s).unwrap();
        assert_eq!(c2.printers.default_printer.as_deref(), Some("EPSON_L15160"));
        assert_eq!(c2.printers.disabled, vec!["Old_Fax".to_string()]);
        assert_eq!(
            c2.printers.routes.get(".pdf").map(String::as_str),
            Some("EPSON_L15160")
        );
        assert_eq!(
            c2.printers.overrides["EPSON_L15160"].paper.as_deref(),
            Some("A4")
        );
    }

    fn _assert_atomic_save_signature() {
        // Compile-time check: save() returns std::io::Result<()>.
        fn _t(_: fn(&Config) -> std::io::Result<()>) {}
        _t(Config::save);
    }

    /// The first-run bookkeeping the webview used to keep in its own storage
    /// (which a new bundle identifier wipes) lives in config.toml; a file
    /// written before it existed loads with both unset.
    #[test]
    fn first_run_flags_default_to_unset_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[app]\nlocale = \"es-ES\"\ntheme = \"dark\"\nlaunch_minimized = false\n",
        )
        .unwrap();
        let mut c = Config::load_from(&path);
        assert!(!c.app.locale_detected);
        assert!(!c.app.onboarding_complete);

        c.app.locale_detected = true;
        c.app.onboarding_complete = true;
        c.save_to(&path).unwrap();
        let back = Config::load_from(&path);
        assert!(back.app.locale_detected && back.app.onboarding_complete);
    }

    /// What a first run writes for itself is not a user's setting.
    #[test]
    fn first_run_default_ignores_the_first_run_bookkeeping() {
        let mut c = Config::default();
        assert!(c.is_first_run_default());
        c.app.locale = "es-ES".into();
        c.app.locale_detected = true;
        c.app.onboarding_complete = true;
        assert!(c.is_first_run_default());
        c.app.theme = Theme::Dark;
        assert!(!c.is_first_run_default());
    }

    #[test]
    fn v1_file_without_new_sections_loads_with_defaults() {
        // A config.toml written by the pre-settings build has only [app] and
        // [printers]; the new sections must fill in from their defaults.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "schema_version = 1\n\
             [app]\n\
             locale = \"en-US\"\n\
             theme = \"system\"\n\
             launch_minimized = false\n\
             [printers]\n\
             disabled = []\n",
        )
        .unwrap();

        let c = Config::load_from(&path);
        assert_eq!(c.schema_version, 1);
        // Missing [app].density defaults to Comfortable.
        assert_eq!(c.app.density, Density::Comfortable);
        // Whole missing sections default.
        assert_eq!(c.engine, EngineCfg::default());
        assert_eq!(c.updater, UpdaterCfg::default());
        assert_eq!(c.telemetry, TelemetryCfg::default());
        assert_eq!(c.updater.channel, UpdateChannel::Stable);
        assert!(c.updater.check_on_startup);
        assert_eq!(c.engine.max_concurrent_conversions, 2);
    }

    #[test]
    fn engine_default_profile_none_serializes() {
        // Regression pin: `toml` errors trying to serialize a bare `None`, so
        // EngineCfg::default_profile MUST skip-serialize when None. This is the
        // failure the whole save() path would hit on a fresh install.
        let c = Config::default();
        assert!(c.engine.default_profile.is_none());
        let s = toml::to_string_pretty(&c).expect("default config serializes to TOML");
        assert!(
            !s.contains("default_profile"),
            "None default_profile must be omitted, not serialized"
        );
    }

    #[test]
    fn fr_072_native_notifications_default_on_and_round_trip() {
        // A config written before the switch existed keeps notifications on.
        let old: Config = toml::from_str(
            "[app]\nlocale = \"es-ES\"\ntheme = \"dark\"\nlaunch_minimized = false\n",
        )
        .unwrap();
        assert!(old.app.native_notifications);
        assert!(Config::default().app.native_notifications);

        let mut c = Config::default();
        c.app.native_notifications = false;
        let c2: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert!(!c2.app.native_notifications);
    }

    #[test]
    fn density_and_channel_round_trip() {
        let mut c = Config::default();
        c.app.density = Density::Compact;
        c.updater.channel = UpdateChannel::Beta;
        c.engine.default_profile = Some("builtin-photo-color".into());
        let s = toml::to_string_pretty(&c).unwrap();
        assert!(s.contains("compact"));
        assert!(s.contains("beta"));
        let c2: Config = toml::from_str(&s).unwrap();
        assert_eq!(c2.app.density, Density::Compact);
        assert_eq!(c2.updater.channel, UpdateChannel::Beta);
        assert_eq!(
            c2.engine.default_profile.as_deref(),
            Some("builtin-photo-color")
        );
    }

    #[test]
    fn fr_engine_sanitized_clamps_hand_edited_values() {
        // Regression: queue::spawn clamps concurrency, but a raw
        // conversion_timeout in 1..10 or a max_file_size_mb of 0 used to pass
        // straight through — 0 MB rejects every file, a 1s timeout kills every
        // conversion. `sanitized()` is the single choke point all consumers use.
        let mk = |mc: u32, to: u32, mb: u32| EngineCfg {
            max_concurrent_conversions: mc,
            conversion_timeout_seconds: to,
            max_file_size_mb: mb,
            print_timeout_seconds: 300,
            ..Default::default()
        };

        // Concurrency clamps into 1..=8.
        assert_eq!(mk(0, 60, 200).sanitized().max_concurrent_conversions, 1);
        assert_eq!(mk(99, 60, 200).sanitized().max_concurrent_conversions, 8);
        assert_eq!(mk(4, 60, 200).sanitized().max_concurrent_conversions, 4);

        // Timeout: 0 stays disabled; 1..10 floors to 10; huge clamps to 3600.
        assert_eq!(mk(2, 0, 200).sanitized().conversion_timeout_seconds, 0);
        assert_eq!(mk(2, 1, 200).sanitized().conversion_timeout_seconds, 10);
        assert_eq!(mk(2, 9, 200).sanitized().conversion_timeout_seconds, 10);
        assert_eq!(mk(2, 60, 200).sanitized().conversion_timeout_seconds, 60);
        assert_eq!(
            mk(2, 99_999, 200).sanitized().conversion_timeout_seconds,
            3600
        );

        // Max file size: 0 → the 200 MB default; else clamp to 1..=4096.
        assert_eq!(mk(2, 60, 0).sanitized().max_file_size_mb, 200);
        assert_eq!(mk(2, 60, 99_999).sanitized().max_file_size_mb, 4096);
        assert_eq!(mk(2, 60, 512).sanitized().max_file_size_mb, 512);

        // default_profile is carried through untouched.
        let mut e = mk(2, 60, 200);
        e.default_profile = Some("builtin-quick".into());
        assert_eq!(
            e.sanitized().default_profile.as_deref(),
            Some("builtin-quick")
        );
    }

    #[test]
    fn load_strict_missing_is_default_but_corrupt_errors() {
        // Regression: a missing file is not an error; a corrupt one is,
        // so mutators never silently rewrite it as defaults.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(Config::load_strict(&path).unwrap(), Config::default());

        std::fs::write(&path, b"this is = not valid = toml ][").unwrap();
        assert!(Config::load_strict(&path).is_err());

        // A well-formed file round-trips through load_strict.
        let mut cfg = Config::default();
        cfg.app.theme = Theme::Dark;
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_strict(&path).unwrap(), cfg);
    }

    #[test]
    fn engine_defaults_come_from_the_limits() {
        // Regression: the defaults used to be literals sitting next to a second
        // set of literals in the clamp. Both now read from ENGINE_LIMITS.
        let e = EngineCfg::default();
        assert_eq!(
            e.max_concurrent_conversions,
            ENGINE_LIMITS.max_concurrent_conversions.default
        );
        assert_eq!(
            e.conversion_timeout_seconds,
            ENGINE_LIMITS.conversion_timeout_seconds.default
        );
        assert_eq!(e.max_file_size_mb, ENGINE_LIMITS.max_file_size_mb.default);
        // Every default is a value the write path would accept.
        assert!(ENGINE_LIMITS
            .max_concurrent_conversions
            .accepts(e.max_concurrent_conversions));
        assert!(ENGINE_LIMITS
            .conversion_timeout_seconds
            .accepts(e.conversion_timeout_seconds));
        assert!(ENGINE_LIMITS.max_file_size_mb.accepts(e.max_file_size_mb));
        assert_eq!(
            e.print_timeout_seconds,
            ENGINE_LIMITS.print_timeout_seconds.default
        );
        assert!(ENGINE_LIMITS
            .print_timeout_seconds
            .accepts(e.print_timeout_seconds));
    }

    /// The print budget is its own knob: default 300 s, `0` turns it off, and
    /// `10..=3600` otherwise — and a config written before it existed still
    /// loads with the default rather than failing to parse.
    #[test]
    fn fr_034_print_timeout_is_a_separate_bounded_setting() {
        let b = ENGINE_LIMITS.print_timeout_seconds;
        assert_eq!(b.default, 300);
        assert!(b.accepts(0), "0 disables it");
        assert!(b.accepts(10) && b.accepts(3600));
        assert!(!b.accepts(9) && !b.accepts(3601));

        let mut e = EngineCfg {
            print_timeout_seconds: 5,
            ..Default::default()
        };
        assert_eq!(e.sanitized().print_timeout_seconds, 10);
        e.print_timeout_seconds = 0;
        assert_eq!(e.sanitized().print_timeout_seconds, 0);

        let old: Config = toml::from_str(
            "schema_version = 1\n[engine]\nmax_concurrent_conversions = 3\nconversion_timeout_seconds = 60\nmax_file_size_mb = 200\n",
        )
        .expect("an engine section without the print budget still parses");
        assert_eq!(old.engine.print_timeout_seconds, 300);
        assert_eq!(old.engine.max_concurrent_conversions, 3);
    }

    #[test]
    fn bounds_zero_policies() {
        let clamp = Bounds {
            min: 1,
            max: 8,
            default: 2,
            zero: ZeroPolicy::Clamp,
        };
        assert!(!clamp.accepts(0));
        assert_eq!(clamp.sanitize(0), 1);

        let disables = Bounds {
            min: 10,
            max: 3600,
            default: 60,
            zero: ZeroPolicy::Disables,
        };
        assert!(disables.accepts(0));
        assert_eq!(disables.sanitize(0), 0);
        assert_eq!(disables.sanitize(9), 10);

        let fallback = Bounds {
            min: 1,
            max: 4096,
            default: 200,
            zero: ZeroPolicy::FallsBackToDefault,
        };
        assert!(!fallback.accepts(0));
        assert_eq!(fallback.sanitize(0), 200);
        assert_eq!(fallback.sanitize(99_999), 4096);
    }

    #[test]
    fn fr_101_load_reporting_flags_only_an_unreadable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        // Missing is the normal first-run case, not a degradation.
        let load = Config::load_reporting(&path);
        assert!(!load.unreadable);
        assert_eq!(load.config, Config::default());

        // A readable file is not flagged either.
        let mut cfg = Config::default();
        cfg.app.theme = Theme::Dark;
        cfg.save_to(&path).unwrap();
        let load = Config::load_reporting(&path);
        assert!(!load.unreadable);
        assert_eq!(load.config.app.theme, Theme::Dark);

        // A corrupt one degrades to defaults AND says so.
        std::fs::write(&path, b"nope ][ =").unwrap();
        let load = Config::load_reporting(&path);
        assert!(load.unreadable);
        assert_eq!(load.config, Config::default());
        // `load_from` keeps its old, lossy contract for callers that can't act.
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn launch_minimized_is_read_from_the_stored_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.app.launch_minimized = true;
        cfg.save_to(&path).unwrap();
        assert!(launch_minimized_at(&path));
    }

    #[test]
    fn start_minimized_applies_only_to_a_plain_launch() {
        assert!(should_start_minimized(true, false));
        // A context-menu launch must show its selection, preference or not.
        assert!(!should_start_minimized(true, true));
        assert!(!should_start_minimized(false, false));
        assert!(!should_start_minimized(false, true));
    }

    #[test]
    fn nfr_014_a_second_process_writing_another_section_is_not_clobbered() {
        // The defect: `pluryprint update channel beta` (a whole
        // load→mutate→save of config.toml) racing a save from the GUI lost one
        // of the two changes. The mutex that serializes writers is
        // intra-process; these two are not the same process.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::default().save_to(&path).unwrap();

        // Process A loads, and is about to change the theme…
        let mut gui = Config::load_strict(&path).unwrap();
        gui.app.theme = Theme::Dark;

        // …while process B changes the update channel and gets there first.
        let mut cli = Config::load_strict(&path).unwrap();
        cli.updater.channel = UpdateChannel::Beta;
        cli.save_to(&path).unwrap();

        // A now writes its stale copy. Both changes must survive.
        gui.save_to(&path).unwrap();

        let stored = Config::load_from(&path);
        assert_eq!(stored.app.theme, Theme::Dark, "A's change was written");
        assert_eq!(
            stored.updater.channel,
            UpdateChannel::Beta,
            "B's change was not clobbered by A's stale copy"
        );
    }

    #[test]
    fn nfr_014_the_reconciliation_is_section_wise_and_the_writer_wins_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::default().save_to(&path).unwrap();

        // A writer that touches printers keeps its own printers section even
        // though someone else wrote that same section meanwhile — a same-section
        // conflict has no better answer than last-writer-wins, and this pins it.
        let mut mine = Config::load_strict(&path).unwrap();
        mine.printers.default_printer = Some("Front Desk".into());

        let mut theirs = Config::load_strict(&path).unwrap();
        theirs.printers.default_printer = Some("Back Office".into());
        theirs.engine.max_file_size_mb = 512;
        theirs.save_to(&path).unwrap();

        mine.save_to(&path).unwrap();

        let stored = Config::load_from(&path);
        assert_eq!(
            stored.printers.default_printer.as_deref(),
            Some("Front Desk")
        );
        assert_eq!(
            stored.engine.max_file_size_mb, 512,
            "a section this writer never touched is kept as the other left it"
        );
    }

    #[test]
    fn a_config_that_was_not_loaded_is_written_wholesale() {
        // Seed/first-run writes have nothing to reconcile against, so they must
        // keep the old, unconditional behaviour.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut existing = Config::default();
        existing.app.theme = Theme::Dark;
        existing.printers.disabled.push("Old_Fax".into());
        existing.save_to(&path).unwrap();

        Config::default().save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn saving_over_a_file_that_became_corrupt_still_writes_this_config() {
        // Refusing here would lose the user's change to protect a file someone
        // else already broke; the corrupt-file refusal belongs on the *load*
        // side, where it can be reported.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        Config::default().save_to(&path).unwrap();

        let mut cfg = Config::load_strict(&path).unwrap();
        cfg.app.theme = Theme::Dark;
        std::fs::write(&path, b"not ][ toml").unwrap();
        cfg.save_to(&path).unwrap();

        assert_eq!(Config::load_from(&path).app.theme, Theme::Dark);
    }

    #[test]
    fn full_config_save_load_round_trips_via_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = Config::default();
        c.app.theme = Theme::Dark;
        c.telemetry.crash_reports = true;
        c.engine.max_file_size_mb = 512;
        c.save_to(&path).unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(loaded, c);
    }

    /// Round 6: the headless-browser renderer is gone. A `config.toml` that
    /// still names it loads (the value is kept, never used), is cleaned on
    /// the first take, and only `"browser"` asks for the one-time notice.
    #[test]
    fn fr_055_retired_html_renderer_is_migrated_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(!take_retired_html_renderer_at(&path), "missing file");

        let mut c = Config::default();
        c.app.theme = Theme::Dark;
        c.save_to(&path).unwrap();
        let clean = std::fs::read_to_string(&path).unwrap();
        assert!(!clean.contains("html_renderer"), "{clean}");
        assert!(!take_retired_html_renderer_at(&path), "nothing to retire");

        std::fs::write(
            &path,
            clean.replace("[engine]\n", "[engine]\nhtml_renderer = \"Browser\"\n"),
        )
        .unwrap();
        let loaded = Config::load_from(&path);
        assert_eq!(
            loaded.engine.retired_html_renderer.as_deref(),
            Some("Browser")
        );
        assert_eq!(loaded.app.theme, Theme::Dark);
        assert!(take_retired_html_renderer_at(&path), "told once");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("html_renderer"), "{after}");
        assert_eq!(Config::load_from(&path).app.theme, Theme::Dark);
        assert!(!take_retired_html_renderer_at(&path), "only once");

        // "system" (the old default, written by hand) is removed silently.
        std::fs::write(
            &path,
            clean.replace("[engine]\n", "[engine]\nhtml_renderer = \"system\"\n"),
        )
        .unwrap();
        assert!(!take_retired_html_renderer_at(&path));
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("html_renderer"));

        // A corrupt file is never rewritten.
        std::fs::write(&path, "html_renderer = ][").unwrap();
        assert!(!take_retired_html_renderer_at(&path));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "html_renderer = ]["
        );
    }
}
