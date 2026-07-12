//! Global app settings (`config.toml`).
//!
//! Loads/saves the file with atomic writes. The `printers` section is exposed
//! to the frontend via the `get_printer_prefs` / `set_printer_prefs` IPC
//! commands (see `ipc::printers`), and holds MaeclyPrint-level printer state
//! that must survive restarts: the app default, disabled printers, file-type
//! routing rules, and per-printer tray/paper overrides.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::util::paths;

pub mod policy;

pub use policy::PrinterPolicy;

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

/// Serializes every read-modify-write of `config.toml`. Any command that mutates
/// the file — [`crate::ipc::printers::set_printer_prefs`],
/// [`crate::ipc::settings::set_app_settings`],
/// [`crate::ipc::profiles::set_default_profile`] — takes this guard so a
/// concurrent write can't clobber a section another command just wrote. Held
/// only across the synchronous load→mutate→save (those run inside
/// `spawn_blocking`, never across an `.await`), so it never blocks the async
/// runtime.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub app: AppCfg,
    #[serde(default)]
    pub printers: PrinterPrefs,
    #[serde(default)]
    pub engine: EngineCfg,
    #[serde(default)]
    pub updater: UpdaterCfg,
    #[serde(default)]
    pub telemetry: TelemetryCfg,
}

/// Engine/queue tuning knobs. All values take effect on the next app start
/// (the queue reads them once at boot); changing them mid-session does not
/// re-tune the running dispatcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EngineCfg {
    /// Concurrent conversions the dispatcher may run (clamped to `1..=8` at
    /// boot). Defaults to the dispatcher's own
    /// [`DEFAULT_MAX_CONCURRENT_CONVERSIONS`](crate::queue::DEFAULT_MAX_CONCURRENT_CONVERSIONS).
    pub max_concurrent_conversions: u32,
    /// Per-conversion timeout in seconds; `0` disables the timeout.
    pub conversion_timeout_seconds: u32,
    /// Largest input file accepted for submission/inspection, in megabytes.
    pub max_file_size_mb: u32,
    /// Profile id applied by default in the print dialog. `None` means
    /// `builtin-quick`. Must skip-serialize when `None`: `toml` errors trying to
    /// serialize a bare `Option::None` value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
}

impl Default for EngineCfg {
    fn default() -> Self {
        Self {
            max_concurrent_conversions: crate::queue::DEFAULT_MAX_CONCURRENT_CONVERSIONS as u32,
            conversion_timeout_seconds: 60,
            max_file_size_mb: 200,
            default_profile: None,
        }
    }
}

/// Fallback used by [`EngineCfg::sanitized`] when a stored `max_file_size_mb` of
/// `0` (which would reject every file) is encountered. Mirrors the field's
/// default so a bricked value lands on the same ceiling a fresh install uses.
const DEFAULT_MAX_FILE_SIZE_MB: u32 = 200;

impl EngineCfg {
    /// A copy with every tuning knob clamped to a usable range, so a
    /// hand-edited (or downgrade-corrupted) `config.toml` can't brick the
    /// engine. `set_app_settings` validates these bounds on the write path, but
    /// a value written directly to disk — or by an older build — bypasses that,
    /// so every *consumer* (queue spawn, submit's size check, `inspect_files`)
    /// reads through this rather than trusting the raw stored numbers.
    ///
    /// - `max_concurrent_conversions`: clamped to `1..=8`.
    /// - `conversion_timeout_seconds`: `0` stays `0` (timeout disabled); any
    ///   other value clamps to `10..=3600` (a 1s timeout would kill every
    ///   conversion).
    /// - `max_file_size_mb`: `0` becomes [`DEFAULT_MAX_FILE_SIZE_MB`] (0 would
    ///   reject every file); any other value clamps to `1..=4096`.
    ///
    /// `default_profile` is passed through unchanged.
    pub fn sanitized(&self) -> EngineCfg {
        EngineCfg {
            max_concurrent_conversions: self.max_concurrent_conversions.clamp(1, 8),
            conversion_timeout_seconds: match self.conversion_timeout_seconds {
                0 => 0,
                secs => secs.clamp(10, 3600),
            },
            max_file_size_mb: match self.max_file_size_mb {
                0 => DEFAULT_MAX_FILE_SIZE_MB,
                mb => mb.clamp(1, 4096),
            },
            default_profile: self.default_profile.clone(),
        }
    }
}

/// Update-check preferences. Only the check itself is wired in this version;
/// `channel` is recorded but every check currently hits the `latest` release
/// feed (there are no separate beta/nightly feeds yet).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UpdaterCfg {
    pub channel: UpdateChannel,
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
    #[default]
    Stable,
    Beta,
    Nightly,
}

/// Telemetry consent flags. These record the user's *consent*; this version
/// transmits nothing regardless of their value — no crash reports and no usage
/// metrics leave the machine. They exist so the choice survives restarts and a
/// future build can honour it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TelemetryCfg {
    #[serde(default)]
    pub crash_reports: bool,
    #[serde(default)]
    pub anonymous_usage_metrics: bool,
}

/// The subset of [`Config`] exposed to the frontend as one bundle by
/// `get_app_settings` / `set_app_settings`. Deliberately excludes `printers`
/// (owned by the Printers screen) and `schema_version` (server-managed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct AppSettings {
    pub app: AppCfg,
    pub engine: EngineCfg,
    pub updater: UpdaterCfg,
    pub telemetry: TelemetryCfg,
}

/// MaeclyPrint-level printer preferences (never mutate the OS spooler).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PrinterPrefs {
    /// App default printer id; falls back to the OS default when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_printer: Option<String>,
    /// Printer ids disabled *in MaeclyPrint* — still installed in the OS.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// File-type routing: extension (e.g. ".pdf") → printer id.
    #[serde(default)]
    pub routes: BTreeMap<String, String>,
    /// Per-printer default tray/paper overrides, keyed by printer id.
    #[serde(default)]
    pub overrides: BTreeMap<String, PrinterOverride>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PrinterOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tray: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppCfg {
    pub locale: String,
    pub theme: Theme,
    pub launch_minimized: bool,
    /// UI spacing preference. `#[serde(default)]` so a config written before this
    /// field existed still loads (filling in [`Density::Comfortable`]).
    #[serde(default)]
    pub density: Density,
}

impl Default for AppCfg {
    fn default() -> Self {
        Self {
            locale: "en-US".into(),
            theme: Theme::System,
            launch_minimized: false,
            density: Density::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    System,
}

/// UI spacing density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
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
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        paths::config_dir().join("config.toml")
    }

    /// Load and parse `config.toml` at `path`, degrading to
    /// [`Config::default`] on any read or parse failure (a missing file is the
    /// common, non-error case). Path-injected so integration tests can point at
    /// a tempdir instead of the real config dir.
    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(s) => match toml::from_str::<Self>(&s) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        event = "config_parse_failed",
                        error = %e,
                        "falling back to default config"
                    );
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!(event = "config_read_failed", error = %e);
                Self::default()
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
    pub fn load_strict(path: &Path) -> Result<Config, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str::<Self>(&s).map_err(|e| e.to_string())
    }

    /// Atomic write to `path`: tempfile in the same directory, then rename.
    /// `tempfile::NamedTempFile::persist` uses MoveFileEx on Windows (atomic on
    /// the same volume) and rename(2) on Unix. Path-injected so tests can write
    /// into a tempdir.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let parent = path.parent().expect("config path has a parent directory");
        paths::ensure_dir(parent)?;

        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        let toml_bytes = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        tmp.write_all(toml_bytes.as_bytes())?;
        tmp.flush()?;
        tmp.persist(path)
            .map_err(|e| std::io::Error::other(format!("persist failed: {e}")))?;
        Ok(())
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
            default_profile: None,
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
}
