//! Filesystem path resolution.
//!
//! Centralizes every hardcoded directory name so the rest of the codebase
//! never spells out "MaeclyPrint" or "Maecly". On Windows this resolves to
//! `%APPDATA%\MaeclyPrint\` (config), `%LOCALAPPDATA%\MaeclyPrint\` (data,
//! cache, logs); on macOS it follows the `~/Library/...` convention.
//!
//! See `docs/04-architecture/data-model.md` §1 for the full table.
//!
//! It also owns [`canonical_input`], the single gate every path that arrives
//! from outside the process (IPC, drag-and-drop, CLI) goes through (NFR-025).

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::util::error::IpcError;

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
pub fn ensure_dir(p: &Path) -> std::io::Result<()> {
    if !p.exists() {
        std::fs::create_dir_all(p)?;
    }
    Ok(())
}

/// Resolve a path that arrived from outside the process (NFR-025).
///
/// Everything the frontend, a drop or the CLI hands us is run through here
/// before it is stored or printed:
///
/// - a **relative** path is refused, never resolved: it would silently land on
///   the process' working directory, which is wherever the OS happened to
///   launch the app from (the Explorer window, `/`, the shell extension's cwd)
///   and is never what the user pointed at;
/// - `.`/`..` segments and symlinks are resolved, so the same file always gets
///   the same identity in the queue no matter how it was named;
/// - a path that does not exist is refused up front, rather than failing later
///   inside a converter with a worse message.
///
/// Symlinks resolving to their target is deliberate: a link to a real document
/// prints that document. The caller keeps the original string for display when
/// it needs to.
pub fn canonical_input(raw: &str) -> Result<PathBuf, IpcError> {
    if raw.trim().is_empty() {
        return Err(IpcError::InvalidRequest("empty path".to_string()));
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(IpcError::InvalidRequest(format!(
            "{raw} is not an absolute path"
        )));
    }
    match path.canonicalize() {
        Ok(p) => Ok(simplified(p)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(IpcError::FileNotFound(raw.to_string()))
        }
        Err(e) => Err(IpcError::Internal(format!("cannot resolve {raw}: {e}"))),
    }
}

/// Windows: `canonicalize` returns the verbatim `\\?\C:\…` form, which the
/// external tools we shell out to (SumatraPDF, the shell) and the UI both
/// dislike, so drive-letter paths are put back in their ordinary form. UNC
/// verbatim paths (`\\?\UNC\…`) are left alone — there the prefix carries
/// meaning.
#[cfg(windows)]
fn simplified(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest)
            if rest.len() >= 2
                && rest.as_bytes()[0].is_ascii_alphabetic()
                && rest.as_bytes()[1] == b':' =>
        {
            PathBuf::from(rest)
        }
        _ => p,
    }
}

/// Non-Windows: `canonicalize` already returns a plain absolute path.
#[cfg(not(windows))]
fn simplified(p: PathBuf) -> PathBuf {
    p
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
    fn nfr_025_relative_paths_are_refused() {
        let err = canonical_input("docs/report.pdf").unwrap_err();
        assert!(
            matches!(err, IpcError::InvalidRequest(ref m) if m.contains("absolute")),
            "expected an absolute-path rejection, got {err:?}"
        );
        assert!(matches!(
            canonical_input("   ").unwrap_err(),
            IpcError::InvalidRequest(_)
        ));
    }

    #[test]
    fn nfr_025_missing_paths_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.pdf");
        assert!(matches!(
            canonical_input(&missing.to_string_lossy()).unwrap_err(),
            IpcError::FileNotFound(_)
        ));
    }

    #[test]
    fn nfr_025_dot_segments_collapse_to_one_identity() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let file = sub.join("a.pdf");
        std::fs::write(&file, b"%PDF-1.4\n").unwrap();

        let direct = canonical_input(&file.to_string_lossy()).unwrap();
        let winding = canonical_input(&sub.join("..").join("sub/a.pdf").to_string_lossy()).unwrap();
        assert_eq!(direct, winding);
        assert!(direct.is_absolute());
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
