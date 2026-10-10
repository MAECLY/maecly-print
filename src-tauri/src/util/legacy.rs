//! What an install of MaeclyPrint (the product's former name) left behind,
//! and the one-time move into the Pluryprint locations (ADR-0009).
//!
//! The rename changed the `ProjectDirs` application segment from
//! `MaeclyPrint` to `Pluryprint`, so every per-user file moved:
//!
//! | Dir    | Windows (old → new)                                   | macOS (old → new)                                                   |
//! | ------ | ----------------------------------------------------- | ------------------------------------------------------------------- |
//! | config | `%APPDATA%\Maecly\MaeclyPrint\config\` → `…\Pluryprint\config\` | `~/Library/Application Support/com.Maecly.MaeclyPrint/` → `com.Maecly.Pluryprint/` |
//! | data   | `%LOCALAPPDATA%\Maecly\MaeclyPrint\data\` → `…\Pluryprint\data\` | (same folder as config)                                      |
//! | cache  | `%LOCALAPPDATA%\Maecly\MaeclyPrint\cache\` → `…\Pluryprint\cache\` | `~/Library/Caches/com.Maecly.MaeclyPrint/` → `com.Maecly.Pluryprint/` |
//! | logs   | inside data                                            | `~/Library/Logs/MaeclyPrint/` → `~/Library/Logs/Pluryprint/`        |
//!
//! [`migrate_current`] runs before logging starts, in the GUI and in the CLI
//! alike. It moves every entry of each old folder into the new one **unless
//! something already exists there under that name** — it never overwrites, it
//! merges folders that exist on both sides entry by entry, and a file it had
//! to leave behind is reported, not lost. Log files keep their history: the
//! old `maeclyprint-<date>.jsonl` names are renamed to `pluryprint-…` so
//! retention keeps pruning them.
//!
//! It is idempotent: whatever moved is gone from the old folder, so the next
//! run finds nothing to do. It does nothing at all while a MaeclyPrint app
//! still runs (its `instance.lock` is held): moving `queue.json` from under a
//! live app would lose its jobs.
//!
//! The machine policy is not here: it lives in admin-only locations a user
//! process cannot move, so `settings::policy` reads the legacy locations as a
//! fallback instead. Deprecated `MAECLYPRINT_*` environment variables are read
//! through [`env_var`].

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use directories::ProjectDirs;

use crate::cli::instance;

/// `ProjectDirs` qualifier and organization: unchanged by the rename.
const QUALIFIER: &str = "com";
const ORG: &str = "Maecly";
/// `ProjectDirs` application segment before the rename.
pub const LEGACY_APP: &str = "MaeclyPrint";
/// `ProjectDirs` application segment after the rename (`util::paths`).
const APP: &str = "Pluryprint";
/// Log file prefix before the rename.
pub const LEGACY_LOG_PREFIX: &str = "maeclyprint-";
/// Log file prefix after the rename (`logging::FILE_PREFIX`).
const LOG_PREFIX: &str = "pluryprint-";
/// Prefix of the environment variables before the rename.
const LEGACY_ENV_PREFIX: &str = "MAECLYPRINT_";
/// Prefix of the environment variables after the rename.
const ENV_PREFIX: &str = "PLURYPRINT_";

/// One old folder and the folder its entries move into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPair {
    pub old: PathBuf,
    pub new: PathBuf,
}

/// Where the old and new per-user folders are on one machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Folder pairs, migrated in order. Never two pairs with the same `old`.
    pub pairs: Vec<DirPair>,
    /// The old app's `instance.lock`: held means a MaeclyPrint app is running.
    pub old_lock: PathBuf,
    /// The new log folder, where old log file names are renamed.
    pub new_log_dir: PathBuf,
}

impl Layout {
    fn from_pairs(pairs: Vec<DirPair>, old_data: &Path, new_log_dir: PathBuf) -> Self {
        let mut unique: Vec<DirPair> = Vec::new();
        for pair in pairs {
            if !unique.iter().any(|p| p.old == pair.old) {
                unique.push(pair);
            }
        }
        Self {
            pairs: unique,
            old_lock: old_data.join(instance::LOCK_FILE),
            new_log_dir,
        }
    }

    /// The Windows shape (what `ProjectDirs` gives there), rooted at the given
    /// `%APPDATA%` and `%LOCALAPPDATA%`. Pure, so tests can build it in a temp
    /// folder on any OS.
    pub fn windows(appdata: &Path, local_appdata: &Path) -> Self {
        let roaming = |app: &str| appdata.join(ORG).join(app);
        let local = |app: &str| local_appdata.join(ORG).join(app);
        let old_data = local(LEGACY_APP).join("data");
        let new_data = local(APP).join("data");
        Self::from_pairs(
            vec![
                DirPair {
                    old: roaming(LEGACY_APP).join("config"),
                    new: roaming(APP).join("config"),
                },
                DirPair {
                    old: old_data.clone(),
                    new: new_data.clone(),
                },
                DirPair {
                    old: local(LEGACY_APP).join("cache"),
                    new: local(APP).join("cache"),
                },
            ],
            &old_data,
            new_data.join("logs"),
        )
    }

    /// The macOS shape (what `ProjectDirs` gives there, plus the
    /// `~/Library/Logs/<app>` log folder), rooted at `home`. Pure.
    pub fn macos(home: &Path) -> Self {
        let support = |app: &str| {
            home.join("Library/Application Support")
                .join(format!("{QUALIFIER}.{ORG}.{app}"))
        };
        let caches = |app: &str| {
            home.join("Library/Caches")
                .join(format!("{QUALIFIER}.{ORG}.{app}"))
        };
        let logs = |app: &str| home.join("Library/Logs").join(app);
        let old_data = support(LEGACY_APP);
        Self::from_pairs(
            vec![
                DirPair {
                    old: old_data.clone(),
                    new: support(APP),
                },
                DirPair {
                    old: caches(LEGACY_APP),
                    new: caches(APP),
                },
                DirPair {
                    old: logs(LEGACY_APP),
                    new: logs(APP),
                },
            ],
            &old_data,
            logs(APP),
        )
    }

    /// This machine's layout, from the same `ProjectDirs` calls
    /// `util::paths` makes. `None` when no home folder can be resolved.
    pub fn current() -> Option<Self> {
        let old = ProjectDirs::from(QUALIFIER, ORG, LEGACY_APP)?;
        let new = ProjectDirs::from(QUALIFIER, ORG, APP)?;
        let mut pairs = vec![
            DirPair {
                old: old.config_dir().to_path_buf(),
                new: new.config_dir().to_path_buf(),
            },
            DirPair {
                old: old.data_local_dir().to_path_buf(),
                new: new.data_local_dir().to_path_buf(),
            },
            DirPair {
                old: old.cache_dir().to_path_buf(),
                new: new.cache_dir().to_path_buf(),
            },
        ];
        if cfg!(target_os = "macos") {
            if let Some(home) = directories::UserDirs::new() {
                let logs = home.home_dir().join("Library/Logs");
                pairs.push(DirPair {
                    old: logs.join(LEGACY_APP),
                    new: logs.join(APP),
                });
            }
        }
        Some(Self::from_pairs(
            pairs,
            old.data_local_dir(),
            crate::util::paths::log_dir(),
        ))
    }
}

/// What a migration did, for the log.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// Entries moved, `(from, to)`.
    pub moved: Vec<(PathBuf, PathBuf)>,
    /// Old entries left where they were because the new location already had
    /// something under that name.
    pub kept: Vec<PathBuf>,
    /// Entries that could not be moved, with the reason.
    pub failed: Vec<(PathBuf, String)>,
    /// Set when the whole migration was skipped this run, with the reason.
    pub deferred: Option<String>,
}

impl Report {
    /// Whether there is nothing to report.
    pub fn is_empty(&self) -> bool {
        self.moved.is_empty()
            && self.kept.is_empty()
            && self.failed.is_empty()
            && self.deferred.is_none()
    }
}

/// Move what an old install left in `layout` into the new folders. Never
/// overwrites; see the module docs.
pub fn migrate(layout: &Layout) -> Report {
    let mut report = Report::default();

    // An old app still running owns these files. `state` would create the
    // lock (and its folder) to probe it, so only probe one that exists.
    if layout.old_lock.exists() && instance::state(&layout.old_lock) == instance::LockState::Held {
        report.deferred = Some(format!(
            "{LEGACY_APP} is still running (its {} is held)",
            layout.old_lock.display()
        ));
        return report;
    }

    for pair in &layout.pairs {
        if !pair.old.is_dir() {
            continue;
        }
        merge(&pair.old, &pair.new, &mut report);
        // Best effort: drop the emptied old folder, then the `MaeclyPrint`
        // folder that held it on Windows (`…\Maecly\MaeclyPrint\config`).
        if std::fs::remove_dir(&pair.old).is_ok() {
            if let Some(parent) = pair.old.parent() {
                if parent.file_name().is_some_and(|n| n == LEGACY_APP) {
                    let _ = std::fs::remove_dir(parent);
                }
            }
        }
    }
    rename_legacy_logs(&layout.new_log_dir, &mut report);
    report
}

/// Move every entry of `old` into `new`, recursing into folders present on
/// both sides; leave (and report) anything whose name is already taken.
fn merge(old: &Path, new: &Path, report: &mut Report) {
    let entries = match std::fs::read_dir(old) {
        Ok(entries) => entries,
        Err(e) => {
            report.failed.push((old.to_path_buf(), e.to_string()));
            return;
        }
    };
    if let Err(e) = std::fs::create_dir_all(new) {
        report.failed.push((old.to_path_buf(), e.to_string()));
        return;
    }
    for entry in entries.flatten() {
        let name = entry.file_name();
        // The old app's lock is not state: it has no meaning for the new app,
        // which keeps its own next to its own queue.json.
        if name == instance::LOCK_FILE {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        let src = entry.path();
        let dst = new.join(&name);
        if std::fs::symlink_metadata(&dst).is_ok() {
            if src.is_dir() && dst.is_dir() {
                merge(&src, &dst, report);
                let _ = std::fs::remove_dir(&src);
            } else {
                report.kept.push(src);
            }
            continue;
        }
        match move_entry(&src, &dst) {
            Ok(()) => report.moved.push((src, dst)),
            Err(e) => report.failed.push((src, e.to_string())),
        }
    }
}

/// `rename`, falling back to copy-then-delete when the two folders are on
/// different volumes (a redirected `%APPDATA%`, for instance).
fn move_entry(src: &Path, dst: &Path) -> io::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_recursive(src, dst)?;
    if src.is_dir() {
        std::fs::remove_dir_all(src)
    } else {
        std::fs::remove_file(src)
    }
}

fn copy_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// Rename `maeclyprint-<date>.jsonl` logs to `pluryprint-<date>.jsonl`, so
/// retention (which matches by prefix) keeps pruning them. Never overwrites.
fn rename_legacy_logs(dir: &Path, report: &mut Report) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name
            .to_str()
            .and_then(|n| n.strip_prefix(LEGACY_LOG_PREFIX))
        else {
            continue;
        };
        let src = entry.path();
        let dst = dir.join(format!("{LOG_PREFIX}{rest}"));
        if dst.exists() {
            report.kept.push(src);
            continue;
        }
        match std::fs::rename(&src, &dst) {
            Ok(()) => report.moved.push((src, dst)),
            Err(e) => report.failed.push((src, e.to_string())),
        }
    }
}

/// Set to `1` to skip the migration: for a developer machine whose real
/// MaeclyPrint data must stay where a pre-rename build still reads it, and for
/// tests that start the real binaries.
pub const SKIP_ENV: &str = "PLURYPRINT_SKIP_MIGRATION";

/// [`migrate`] on this machine's [`Layout::current`], at most once per
/// process (the GUI and the CLI both call it on the way in). Skipped when
/// [`SKIP_ENV`] is `1`.
pub fn migrate_current() -> Report {
    static DONE: OnceLock<Report> = OnceLock::new();
    DONE.get_or_init(|| {
        if std::env::var(SKIP_ENV).as_deref() == Ok("1") {
            return Report {
                deferred: Some(format!("{SKIP_ENV}=1")),
                ..Report::default()
            };
        }
        Layout::current().map(|l| migrate(&l)).unwrap_or_default()
    })
    .clone()
}

/// Write a migration [`Report`] to the log. Call once logging is up: the
/// migration itself runs before, so the log files it moves are not open.
pub fn log_report(report: &Report) {
    if let Some(reason) = &report.deferred {
        tracing::info!(event = "legacy_migration_deferred", reason = %reason);
    }
    for (from, to) in &report.moved {
        tracing::info!(
            event = "legacy_migrated",
            from = %from.display(),
            to = %to.display(),
            "moved MaeclyPrint data to its Pluryprint location"
        );
    }
    for path in &report.kept {
        tracing::warn!(
            event = "legacy_migration_kept",
            path = %path.display(),
            "left MaeclyPrint data in place: the Pluryprint location already has it"
        );
    }
    for (path, error) in &report.failed {
        tracing::warn!(
            event = "legacy_migration_failed",
            path = %path.display(),
            error = %error,
        );
    }
}

/// The deprecated `MAECLYPRINT_*` name of a `PLURYPRINT_*` variable.
fn legacy_env_name(name: &str) -> Option<String> {
    name.strip_prefix(ENV_PREFIX)
        .map(|rest| format!("{LEGACY_ENV_PREFIX}{rest}"))
}

/// [`env_var`] against any lookup, for tests. Returns the value and, when it
/// came from the deprecated name, that name.
fn env_var_with(
    name: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<(String, Option<String>)> {
    if let Some(value) = lookup(name) {
        return Some((value, None));
    }
    let legacy = legacy_env_name(name)?;
    lookup(&legacy).map(|value| (value, Some(legacy)))
}

/// Read the `PLURYPRINT_*` variable `name`, falling back to its deprecated
/// `MAECLYPRINT_*` spelling. The first fallback per name logs one warning.
pub fn env_var(name: &str) -> Option<String> {
    static WARNED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let (value, legacy) = env_var_with(name, |n| std::env::var(n).ok())?;
    if let Some(legacy) = legacy {
        let first = WARNED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(legacy.clone());
        if first {
            tracing::warn!(
                event = "deprecated_env_var",
                deprecated = %legacy,
                replacement = %name,
                "{legacy} is deprecated; set {name} instead"
            );
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    /// An old Windows install: config in Roaming, data (with logs and
    /// handoff) and cache in Local.
    #[test]
    fn windows_layout_moves_everything_and_renames_logs() {
        let root = tempfile::tempdir().unwrap();
        let appdata = root.path().join("Roaming");
        let local = root.path().join("Local");
        let old_cfg = appdata.join("Maecly/MaeclyPrint/config");
        let old_data = local.join("Maecly/MaeclyPrint/data");
        let old_cache = local.join("Maecly/MaeclyPrint/cache");
        write(&old_cfg.join("config.toml"), "cfg");
        write(&old_cfg.join("profiles.toml"), "profiles");
        write(&old_data.join("queue.json"), "[]");
        write(&old_data.join("instance.lock"), "");
        write(&old_data.join("handoff/sel-1.json"), "{}");
        write(&old_data.join("logs/maeclyprint-2026-07-20.jsonl"), "log");
        write(&old_data.join("logs/shellext.log"), "ext");
        write(&old_cache.join("conversions/a.pdf"), "%PDF");

        let layout = Layout::windows(&appdata, &local);
        let report = migrate(&layout);

        let new_cfg = appdata.join("Maecly/Pluryprint/config");
        let new_data = local.join("Maecly/Pluryprint/data");
        let new_cache = local.join("Maecly/Pluryprint/cache");
        assert_eq!(read(&new_cfg.join("config.toml")), "cfg");
        assert_eq!(read(&new_cfg.join("profiles.toml")), "profiles");
        assert_eq!(read(&new_data.join("queue.json")), "[]");
        assert_eq!(read(&new_data.join("handoff/sel-1.json")), "{}");
        assert_eq!(
            read(&new_data.join("logs/pluryprint-2026-07-20.jsonl")),
            "log"
        );
        assert_eq!(read(&new_data.join("logs/shellext.log")), "ext");
        assert_eq!(read(&new_cache.join("conversions/a.pdf")), "%PDF");
        assert!(
            !new_data.join("instance.lock").exists(),
            "the old lock is not state"
        );
        assert!(
            !local.join("Maecly/MaeclyPrint").exists(),
            "the emptied old folders are removed"
        );
        assert!(!appdata.join("Maecly/MaeclyPrint").exists());
        assert!(
            report.kept.is_empty() && report.failed.is_empty(),
            "{report:?}"
        );
        assert!(report.deferred.is_none());
        assert!(!report.moved.is_empty());

        // Idempotent: a second run finds nothing to do.
        assert!(migrate(&layout).is_empty());
    }

    /// An old macOS install: config and data share one folder; caches and
    /// logs have their own.
    #[test]
    fn macos_layout_moves_everything_and_renames_logs() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let old_support = h.join("Library/Application Support/com.Maecly.MaeclyPrint");
        write(&old_support.join("config.toml"), "cfg");
        write(&old_support.join("profiles.toml"), "profiles");
        write(&old_support.join("queue.json"), "[]");
        write(&old_support.join("update-state.json"), "{}");
        write(
            &h.join("Library/Caches/com.Maecly.MaeclyPrint/conversions/a.pdf"),
            "%PDF",
        );
        write(
            &h.join("Library/Logs/MaeclyPrint/maeclyprint-2026-07-20.2.jsonl"),
            "log",
        );

        let layout = Layout::macos(h);
        assert_eq!(layout.pairs.len(), 3, "config and data are one pair");
        let report = migrate(&layout);

        let new_support = h.join("Library/Application Support/com.Maecly.Pluryprint");
        assert_eq!(read(&new_support.join("config.toml")), "cfg");
        assert_eq!(read(&new_support.join("profiles.toml")), "profiles");
        assert_eq!(read(&new_support.join("queue.json")), "[]");
        assert_eq!(read(&new_support.join("update-state.json")), "{}");
        assert_eq!(
            read(&h.join("Library/Caches/com.Maecly.Pluryprint/conversions/a.pdf")),
            "%PDF"
        );
        assert_eq!(
            read(&h.join("Library/Logs/Pluryprint/pluryprint-2026-07-20.2.jsonl")),
            "log"
        );
        assert!(!old_support.exists());
        assert!(!h.join("Library/Logs/MaeclyPrint").exists());
        assert!(
            report.kept.is_empty() && report.failed.is_empty(),
            "{report:?}"
        );
        assert!(migrate(&layout).is_empty(), "idempotent");
    }

    /// Never overwrite: what the new install already has wins, the old copy
    /// stays where it was and is reported. Folders on both sides merge.
    #[test]
    fn existing_new_files_are_never_overwritten() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let old = h.join("Library/Application Support/com.Maecly.MaeclyPrint");
        let new = h.join("Library/Application Support/com.Maecly.Pluryprint");
        write(&old.join("config.toml"), "old");
        write(&old.join("profiles.toml"), "old profiles");
        write(&old.join("handoff/a.json"), "a");
        write(&new.join("config.toml"), "new");
        write(&new.join("handoff/b.json"), "b");
        write(
            &h.join("Library/Logs/MaeclyPrint/maeclyprint-2026-07-20.jsonl"),
            "old log",
        );
        write(
            &h.join("Library/Logs/Pluryprint/pluryprint-2026-07-20.jsonl"),
            "new log",
        );

        let report = migrate(&Layout::macos(h));

        assert_eq!(read(&new.join("config.toml")), "new");
        assert_eq!(read(&old.join("config.toml")), "old", "kept, not lost");
        assert_eq!(read(&new.join("profiles.toml")), "old profiles");
        assert_eq!(read(&new.join("handoff/a.json")), "a");
        assert_eq!(read(&new.join("handoff/b.json")), "b");
        assert_eq!(
            read(&h.join("Library/Logs/Pluryprint/pluryprint-2026-07-20.jsonl")),
            "new log"
        );
        assert!(report.kept.contains(&old.join("config.toml")), "{report:?}");
        assert!(report.failed.is_empty(), "{report:?}");
    }

    /// A MaeclyPrint app still running owns its files: nothing moves.
    #[test]
    fn a_running_old_app_defers_the_migration() {
        let root = tempfile::tempdir().unwrap();
        let appdata = root.path().join("Roaming");
        let local = root.path().join("Local");
        let layout = Layout::windows(&appdata, &local);
        let queue = local.join("Maecly/MaeclyPrint/data/queue.json");
        write(&queue, "[]");
        let lock = instance::claim(&layout.old_lock).unwrap().expect("lock");

        let report = migrate(&layout);
        assert!(report.deferred.is_some(), "{report:?}");
        assert!(report.moved.is_empty());
        assert!(queue.exists());

        drop(lock);
        let report = migrate(&layout);
        assert!(report.deferred.is_none());
        assert!(local.join("Maecly/Pluryprint/data/queue.json").exists());
    }

    /// Nothing installed: nothing to do, and no old folder gets created.
    #[test]
    fn a_clean_machine_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let report = migrate(&Layout::macos(home.path()));
        assert!(report.is_empty(), "{report:?}");
        assert!(!home
            .path()
            .join("Library/Application Support/com.Maecly.MaeclyPrint")
            .exists());
    }

    /// The pure layouts mirror what `ProjectDirs` really resolves to here.
    #[test]
    fn current_layout_targets_the_live_paths() {
        let layout = Layout::current().expect("home resolvable");
        let news: Vec<_> = layout.pairs.iter().map(|p| p.new.clone()).collect();
        assert!(news.contains(&crate::util::paths::config_dir()), "{news:?}");
        assert!(news.contains(&crate::util::paths::data_dir()), "{news:?}");
        assert!(news.contains(&crate::util::paths::cache_dir()), "{news:?}");
        assert_eq!(layout.new_log_dir, crate::util::paths::log_dir());
        for pair in &layout.pairs {
            let old = pair.old.to_string_lossy().to_lowercase();
            assert!(old.contains("maeclyprint"), "{old}");
        }
    }

    #[test]
    fn deprecated_env_vars_are_still_read() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |n: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == n)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            env_var_with(
                "PLURYPRINT_UPDATE_ENDPOINT",
                env(&[("PLURYPRINT_UPDATE_ENDPOINT", "new")])
            ),
            Some(("new".into(), None))
        );
        assert_eq!(
            env_var_with(
                "PLURYPRINT_UPDATE_ENDPOINT",
                env(&[("MAECLYPRINT_UPDATE_ENDPOINT", "old")])
            ),
            Some(("old".into(), Some("MAECLYPRINT_UPDATE_ENDPOINT".into())))
        );
        assert_eq!(
            env_var_with(
                "PLURYPRINT_UPDATE_ENDPOINT",
                env(&[
                    ("PLURYPRINT_UPDATE_ENDPOINT", "new"),
                    ("MAECLYPRINT_UPDATE_ENDPOINT", "old")
                ])
            ),
            Some(("new".into(), None)),
            "the new name wins"
        );
        assert_eq!(env_var_with("PLURYPRINT_X", env(&[])), None);
        assert_eq!(
            env_var_with("OTHER", env(&[("MAECLYPRINT_OTHER", "x")])),
            None
        );
    }
}
