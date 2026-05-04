//! Global app settings (`config.toml`).
//!
//! At this milestone the module only knows how to load and save the file with
//! atomic writes. It is **not yet exposed via IPC** — the frontend persists
//! its UI state to localStorage in this scaffold. M1 will wire the read path
//! to a `get_settings`/`update_settings` command pair per
//! `docs/04-architecture/ipc-and-cli.md` §2.1.

use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::util::paths;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub app: AppCfg,
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

    fn _assert_atomic_save_signature() {
        // Compile-time check: save() returns std::io::Result<()>.
        fn _t(_: fn(&Config) -> std::io::Result<()>) {}
        _t(Config::save);
    }
}
