//! Filesystem path resolution.
//!
//! Centralizes every hardcoded directory name so the rest of the codebase
//! never spells out "MaeclyPrint" or "Maecly". On Windows this resolves to
//! `%APPDATA%\MaeclyPrint\` (config), `%LOCALAPPDATA%\MaeclyPrint\` (data,
//! cache, logs); on macOS it follows the `~/Library/...` convention.
//!
//! See `docs/04-architecture/data-model.md` §1 for the full table.

use std::path::PathBuf;

use directories::ProjectDirs;

const QUALIFIER: &str = "com";
const ORG: &str = "Maecly";
const APP: &str = "MaeclyPrint";

fn dirs() -> ProjectDirs {
    ProjectDirs::from(QUALIFIER, ORG, APP).expect("home directory must be resolvable")
}

/// Where `config.toml` and `profiles.toml` live.
/// Windows: `%APPDATA%\MaeclyPrint\`
/// macOS:   `~/Library/Application Support/MaeclyPrint/`
pub fn config_dir() -> PathBuf {
    dirs().config_dir().to_path_buf()
}

/// Where `queue.json` and other live state live.
/// Windows: `%LOCALAPPDATA%\MaeclyPrint\`
/// macOS:   `~/Library/Application Support/MaeclyPrint/`
pub fn data_dir() -> PathBuf {
    dirs().data_local_dir().to_path_buf()
}

/// Where converted PDFs and updater staging live.
/// Windows: `%LOCALAPPDATA%\MaeclyPrint\`
/// macOS:   `~/Library/Caches/MaeclyPrint/`
pub fn cache_dir() -> PathBuf {
    dirs().cache_dir().to_path_buf()
}

/// Where rotating JSONL logs are written.
/// Windows: `%LOCALAPPDATA%\MaeclyPrint\logs\`
/// macOS:   `~/Library/Logs/MaeclyPrint/`
pub fn log_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = directories::UserDirs::new() {
            return home.home_dir().join("Library/Logs").join(APP);
        }
        data_dir().join("logs")
    }
    #[cfg(not(target_os = "macos"))]
    {
        data_dir().join("logs")
    }
}

/// Ensure a directory exists; create it (and parents) if missing.
pub fn ensure_dir(p: &std::path::Path) -> std::io::Result<()> {
    if !p.exists() {
        std::fs::create_dir_all(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirs_are_resolvable() {
        let c = config_dir();
        let d = data_dir();
        let l = log_dir();
        assert!(c.is_absolute(), "config_dir not absolute: {c:?}");
        assert!(d.is_absolute(), "data_dir not absolute: {d:?}");
        assert!(l.is_absolute(), "log_dir not absolute: {l:?}");
    }

    #[test]
    fn dirs_have_app_segment() {
        // All resolved dirs should contain the literal "MaeclyPrint" segment somewhere.
        for p in [config_dir(), data_dir(), cache_dir()] {
            let s = p.to_string_lossy().to_lowercase();
            assert!(
                s.contains("maeclyprint"),
                "expected MaeclyPrint segment in {p:?}"
            );
        }
    }
}
