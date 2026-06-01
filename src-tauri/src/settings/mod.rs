//! Global app settings (`config.toml`).
//!
//! Loads/saves the file with atomic writes. The `printers` section is exposed
//! to the frontend via the `get_printer_prefs` / `set_printer_prefs` IPC
//! commands (see `ipc::printers`), and holds MaeclyPrint-level printer state
//! that must survive restarts: the app default, disabled printers, file-type
//! routing rules, and per-printer tray/paper overrides.

use std::collections::BTreeMap;
use std::io::Write;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::util::paths;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub app: AppCfg,
    #[serde(default)]
    pub printers: PrinterPrefs,
}

/// MaeclyPrint-level printer preferences (never mutate the OS spooler).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct PrinterOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tray: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppCfg {
    pub locale: String,
    pub theme: Theme,
    pub launch_minimized: bool,
}

impl Default for AppCfg {
    fn default() -> Self {
        Self {
            locale: "en-US".into(),
            theme: Theme::System,
            launch_minimized: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    System,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            app: AppCfg::default(),
            printers: PrinterPrefs::default(),
        }
    }
}

impl Config {
    pub fn config_path() -> std::path::PathBuf {
        paths::config_dir().join("config.toml")
    }

    pub fn load_or_default() -> Self {
        let path = Self::config_path();
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
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

    /// Atomic write: tempfile in same directory, then rename.
    /// `tempfile::NamedTempFile::persist` uses MoveFileEx on Windows
    /// (atomic on the same volume) and rename(2) on Unix.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        paths::ensure_dir(path.parent().expect("config path has a parent"))?;

        let parent = path.parent().expect("config_path has a parent directory");
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        let toml_bytes = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        tmp.write_all(toml_bytes.as_bytes())?;
        tmp.flush()?;
        tmp.persist(&path)
            .map_err(|e| std::io::Error::other(format!("persist failed: {e}")))?;
        Ok(())
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
}
