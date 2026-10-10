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
//! something already exists there under that name** — it never overwrites
//! what the user has, it merges folders that exist on both sides entry by
//! entry, and a file it had to leave behind is reported, not lost. The one
//! exception is a file that holds only what Pluryprint writes for itself on a
//! first run (default settings, an empty queue — see
//! [`is_first_run_default`]): a first launch made while MaeclyPrint was still
//! running writes those before the migration can run, so they are set aside
//! as `<name>.first-run.bak` and the old file takes their place. Log files
//! keep their history: the old `maeclyprint-<date>.jsonl` names are renamed to
//! `pluryprint-…` so retention keeps pruning them.
//!
//! A move across volumes copies to a hidden temporary next to the destination
//! and renames it into place once whole, so a failed copy never leaves a
//! truncated file the next run would take for the user's.
//!
//! Who may move what, while it moves:
//!
//! - one process at a time: the migration takes `migration.lock` in the new
//!   data folder, waiting for another launch's migration to finish. The GUI
//!   keeps it until it holds its own instance lock, so a second launch never
//!   migrates into the folders of an app still starting;
//! - not under a running Pluryprint: its `instance.lock` held (or not
//!   checkable) defers the whole migration;
//! - not under a running MaeclyPrint: the migration holds the old
//!   `instance.lock` for the whole move, and a lock that is held or cannot be
//!   checked defers it. Moving `queue.json` from under a live app would lose
//!   its jobs.
//!
//! A migration that runs to the end without a failure writes
//! `legacy-migration.done` in the new data folder; later runs do nothing, so
//! the files it had to keep are reported once, not on every launch. A run
//! with failures writes no marker and is tried again next launch.
//!
//! The machine policy is not here: it lives in admin-only locations a user
//! process cannot move, so `settings::policy` reads the legacy locations as a
//! fallback instead. Deprecated `MAECLYPRINT_*` environment variables are read
//! through [`env_var`].

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
/// The settings file, in the config folder.
const CONFIG_FILE: &str = "config.toml";
/// The migration's own lock, in the new data folder.
const MIGRATION_LOCK_FILE: &str = "migration.lock";
/// The "migration finished" marker, in the new data folder.
pub const MARKER_FILE: &str = "legacy-migration.done";
/// Suffix a first-run default gets when old data replaces it.
const FIRST_RUN_BACKUP_SUFFIX: &str = ".first-run.bak";
/// How long a launch waits for another process' migration to finish.
const MIGRATION_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

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
    /// This app's `instance.lock`: held means a Pluryprint app is running.
    pub new_lock: PathBuf,
    /// Held by whichever process is migrating, so two never run at once.
    pub migration_lock: PathBuf,
    /// Written once a migration has run to the end; later runs do nothing.
    pub marker: PathBuf,
    /// The new `config.toml`, where an upgrade records its first run as done.
    pub new_config: PathBuf,
    /// The new log folder, where old log file names are renamed.
    pub new_log_dir: PathBuf,
}

impl Layout {
    fn from_pairs(
        pairs: Vec<DirPair>,
        old_data: &Path,
        new_data: &Path,
        new_config_dir: &Path,
        new_log_dir: PathBuf,
    ) -> Self {
        let mut unique: Vec<DirPair> = Vec::new();
        for pair in pairs {
            if !unique.iter().any(|p| p.old == pair.old) {
                unique.push(pair);
            }
        }
        Self {
            pairs: unique,
            old_lock: old_data.join(instance::LOCK_FILE),
            new_lock: new_data.join(instance::LOCK_FILE),
            migration_lock: new_data.join(MIGRATION_LOCK_FILE),
            marker: new_data.join(MARKER_FILE),
            new_config: new_config_dir.join(CONFIG_FILE),
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
            &new_data,
            &roaming(APP).join("config"),
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
        let new_data = support(APP);
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
            &new_data,
            &new_data,
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
            new.data_local_dir(),
            new.config_dir(),
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
    /// New files that held only what a first run writes for itself, set aside
    /// (`from`, `backup`) so the old data could take their place.
    pub backed_up: Vec<(PathBuf, PathBuf)>,
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
            && self.backed_up.is_empty()
            && self.failed.is_empty()
            && self.deferred.is_none()
    }
}

/// A finished [`migrate_with`]: what it did, and the migration lock if it
/// took it. A GUI keeps the lock until it holds its own instance lock, so a
/// second launch cannot migrate into the folders of an app still starting.
#[derive(Debug, Default)]
pub struct Migration {
    pub report: Report,
    pub guard: Option<instance::InstanceLock>,
}

impl Migration {
    fn deferred(reason: String, guard: Option<instance::InstanceLock>) -> Self {
        Self {
            report: Report {
                deferred: Some(reason),
                ..Report::default()
            },
            guard,
        }
    }
}

/// Move what an old install left in `layout` into the new folders. Never
/// overwrites what the user has; see the module docs.
pub fn migrate(layout: &Layout) -> Report {
    migrate_with(layout, &REAL_OPS).report
}

fn migrate_with(layout: &Layout, ops: &Ops) -> Migration {
    // Done once, for good: an old app run since (its `.app` stays installed on
    // macOS) starts over from empty folders, and that is not the user's data.
    if layout.marker.exists() {
        return Migration::default();
    }
    // Nothing installed: no locks, no folders created.
    if !layout.pairs.iter().any(|p| p.old.is_dir()) {
        return Migration::default();
    }

    // One migration at a time. A second launch waits for the first, then
    // finds the marker.
    let guard = match instance::claim_within(&layout.migration_lock, MIGRATION_WAIT) {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            return Migration::deferred("another launch is migrating".into(), None);
        }
        Err(e) => {
            return Migration::deferred(format!("cannot lock the migration: {e}"), None);
        }
    };
    if layout.marker.exists() {
        return Migration {
            report: Report::default(),
            guard: Some(guard),
        };
    }

    // A Pluryprint already running owns the new folders (a context-menu launch
    // or a CLI call made while it runs). Unknown is not "not running".
    if layout.new_lock.exists() {
        match instance::state(&layout.new_lock) {
            instance::LockState::Free => {}
            instance::LockState::Held => {
                return Migration::deferred("Pluryprint is already running".into(), Some(guard));
            }
            instance::LockState::Unknown(e) => {
                return Migration::deferred(
                    format!("cannot tell whether Pluryprint is running: {e}"),
                    Some(guard),
                );
            }
        }
    }

    // An old app still running owns its files, and one starting while they
    // move must not get them back: hold its lock for the whole move. `claim`
    // creates the lock (and its folder), so only claim one whose folder exists.
    let old_guard = if layout.old_lock.parent().is_some_and(Path::is_dir) {
        match instance::claim(&layout.old_lock) {
            Ok(Some(lock)) => Some(lock),
            Ok(None) => {
                return Migration::deferred(
                    format!(
                        "{LEGACY_APP} is still running (its {} is held)",
                        layout.old_lock.display()
                    ),
                    Some(guard),
                );
            }
            Err(e) => {
                return Migration::deferred(
                    format!("cannot tell whether {LEGACY_APP} is running: {e}"),
                    Some(guard),
                );
            }
        }
    } else {
        None
    };

    let mut report = Report::default();
    for pair in &layout.pairs {
        if pair.old.is_dir() {
            merge_with(&pair.old, &pair.new, &mut report, ops);
        }
    }
    // Everything else has moved: release the old lock, delete it, and then the
    // emptied folders can go. The lock only detects an old app that is already
    // running; a pre-rename build starts even when it finds it held.
    drop(old_guard);
    let _ = std::fs::remove_file(&layout.old_lock);
    for pair in &layout.pairs {
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
    if !report.moved.is_empty() {
        record_upgrade(&layout.new_config, &mut report);
    }

    // A failure is retried next launch; anything else is final, including
    // the files kept in the old folder, which are reported this once.
    if report.failed.is_empty() {
        if let Err(e) = std::fs::write(&layout.marker, marker_text(&report)) {
            report.failed.push((layout.marker.clone(), e.to_string()));
        }
    }
    Migration {
        report,
        guard: Some(guard),
    }
}

/// What the old app's webview remembered is not visible to the new one (its
/// bundle identifier changed), so record it where the new app reads it: an
/// upgrader has seen the tour, and the locale of a migrated config.toml is
/// the user's (or the old detection's) answer, which detection must keep.
fn record_upgrade(config_path: &Path, report: &mut Report) {
    let config_moved = report.moved.iter().any(|(_, to)| to == config_path);
    // A config that does not parse is left alone: every writer refuses it.
    let mut config = match crate::settings::Config::load_strict(config_path) {
        Ok(config) => config,
        Err(_) => return,
    };
    if config.app.onboarding_complete && (config.app.locale_detected || !config_moved) {
        return;
    }
    config.app.onboarding_complete = true;
    if config_moved {
        config.app.locale_detected = true;
    }
    if let Err(e) = config.save_to(config_path) {
        report
            .failed
            .push((config_path.to_path_buf(), e.to_string()));
    }
}

fn marker_text(report: &Report) -> String {
    let mut text = format!(
        "{LEGACY_APP} data migrated to {APP}.\nmoved: {}\n",
        report.moved.len()
    );
    for path in &report.kept {
        text.push_str(&format!("kept in place: {}\n", path.display()));
    }
    for (from, backup) in &report.backed_up {
        text.push_str(&format!(
            "first-run file set aside: {} -> {}\n",
            from.display(),
            backup.display()
        ));
    }
    text
}

/// Whether the file at `path` holds only what Pluryprint writes for itself on
/// a first run — nothing the user chose. Such a file gives way to the old
/// data (a first launch made while MaeclyPrint was still running writes them
/// before the migration could run). Unknown names never qualify.
fn is_first_run_default(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    match name {
        "config.toml" => {
            toml::from_str::<crate::settings::Config>(&text).is_ok_and(|c| c.is_first_run_default())
        }
        "profiles.toml" => toml::from_str::<crate::profiles::ProfilesFile>(&text)
            .is_ok_and(|f| f.profile.is_empty()),
        "queue.json" => serde_json::from_str::<crate::queue::job::QueueSnapshot>(&text)
            .is_ok_and(|q| q.jobs.is_empty() && !q.queue_paused),
        "update-state.json" => serde_json::from_str::<crate::update::UpdateState>(&text)
            .is_ok_and(|u| u.skipped_version.is_none()),
        crate::shell_menu::CACHE_FILE => true,
        _ => false,
    }
}

/// A free name next to `path` to set a first-run default aside under.
fn backup_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut candidate = path.with_file_name(format!("{name}{FIRST_RUN_BACKUP_SUFFIX}"));
    let mut n = 2;
    while std::fs::symlink_metadata(&candidate).is_ok() {
        candidate = path.with_file_name(format!("{name}{FIRST_RUN_BACKUP_SUFFIX}.{n}"));
        n += 1;
    }
    candidate
}

/// Replace the first-run default at `dst` with the old file `src`: set it
/// aside, move `src` in, and put it back if the move fails.
fn replace_first_run_default(src: &Path, dst: &Path, report: &mut Report, ops: &Ops) {
    let backup = backup_path(dst);
    if let Err(e) = std::fs::rename(dst, &backup) {
        report.failed.push((src.to_path_buf(), e.to_string()));
        return;
    }
    match move_entry_with(src, dst, ops) {
        Ok(()) => {
            report.backed_up.push((dst.to_path_buf(), backup));
            report.moved.push((src.to_path_buf(), dst.to_path_buf()));
        }
        Err(e) => {
            let _ = std::fs::rename(&backup, dst);
            report.failed.push((src.to_path_buf(), e.to_string()));
        }
    }
}

/// The filesystem calls a move makes, so tests can make them fail.
struct Ops<'a> {
    rename: &'a dyn Fn(&Path, &Path) -> io::Result<()>,
    copy_file: &'a dyn Fn(&Path, &Path) -> io::Result<()>,
}

fn rename(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::rename(src, dst)
}

fn copy_file(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::copy(src, dst).map(|_| ())
}

const REAL_OPS: Ops<'static> = Ops {
    rename: &rename,
    copy_file: &copy_file,
};

/// Move every entry of `old` into `new`, recursing into folders present on
/// both sides. A name already taken keeps what is there, and the old entry
/// is left (and reported) — unless what is there is a first-run default
/// ([`is_first_run_default`]), which is set aside for the old file.
fn merge_with(old: &Path, new: &Path, report: &mut Report, ops: &Ops) {
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
        // which keeps its own next to its own queue.json. It is left in place
        // here and removed only after the move, once its guard is dropped:
        // deleting it now would let an old app starting mid-move take a fresh
        // lock while queue.json is still being moved (NTFS lists it first).
        if name == instance::LOCK_FILE {
            continue;
        }
        let src = entry.path();
        let dst = new.join(&name);
        if std::fs::symlink_metadata(&dst).is_ok() {
            if src.is_dir() && dst.is_dir() {
                merge_with(&src, &dst, report, ops);
                let _ = std::fs::remove_dir(&src);
            } else if src.is_file() && dst.is_file() && is_first_run_default(&dst) {
                replace_first_run_default(&src, &dst, report, ops);
            } else {
                report.kept.push(src);
            }
            continue;
        }
        match move_entry_with(&src, &dst, ops) {
            Ok(()) => report.moved.push((src, dst)),
            Err(e) => report.failed.push((src, e.to_string())),
        }
    }
}

/// Suffix of the temporary a cross-volume copy is written under.
const TEMP_SUFFIX: &str = ".pluryprint-migrating";

/// The temporary name `dst` is copied under before it is renamed into place:
/// a hidden sibling, so the final rename stays on one volume.
fn temp_name(dst: &Path) -> Option<std::ffi::OsString> {
    let mut name = std::ffi::OsString::from(".");
    name.push(dst.file_name()?);
    name.push(TEMP_SUFFIX);
    Some(name)
}

fn remove_any(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

/// `rename`, falling back to copy-then-delete when the two folders are on
/// different volumes (a redirected `%APPDATA%`, for instance).
///
/// The copy goes to a temporary sibling of `dst` and is renamed into place
/// only once it is whole, so `dst` either does not exist or is complete: a
/// copy that fails part-way (disk full, an I/O error) leaves nothing a later
/// run would take for the user's file, and the source stays where it was.
fn move_entry_with(src: &Path, dst: &Path, ops: &Ops) -> io::Result<()> {
    if (ops.rename)(src, dst).is_ok() {
        return Ok(());
    }
    let tmp_name = temp_name(dst)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let tmp = dst.with_file_name(tmp_name);
    // A run that died mid-copy left this behind; it is never the user's.
    remove_any(&tmp);
    if let Err(e) = copy_recursive(src, &tmp, ops) {
        remove_any(&tmp);
        return Err(e);
    }
    if std::fs::symlink_metadata(dst).is_ok() {
        // Something took the name while we copied: never overwrite it.
        remove_any(&tmp);
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} appeared during the move", dst.display()),
        ));
    }
    if let Err(e) = std::fs::rename(&tmp, dst) {
        remove_any(&tmp);
        return Err(e);
    }
    // `dst` is complete. A source that will not go away is only a copy now.
    if src.is_dir() {
        let _ = std::fs::remove_dir_all(src);
    } else {
        let _ = std::fs::remove_file(src);
    }
    Ok(())
}

fn copy_recursive(src: &Path, dst: &Path, ops: &Ops) -> io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()), ops)?;
        }
        Ok(())
    } else {
        (ops.copy_file)(src, dst)
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
/// process (the GUI and the CLI both call it on the way in; a second call
/// returns an empty [`Migration`]). Skipped when [`SKIP_ENV`] is `1`.
///
/// The GUI keeps the returned [`Migration::guard`] until it holds its own
/// instance lock; the CLI drops it once the migration is logged.
pub fn migrate_current() -> Migration {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Migration::default();
    }
    if std::env::var(SKIP_ENV).as_deref() == Ok("1") {
        return Migration::deferred(format!("{SKIP_ENV}=1"), None);
    }
    Layout::current()
        .map(|l| migrate_with(&l, &REAL_OPS))
        .unwrap_or_default()
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
    for (from, backup) in &report.backed_up {
        tracing::info!(
            event = "legacy_migration_first_run_set_aside",
            from = %from.display(),
            backup = %backup.display(),
            "set aside a first-run default so the MaeclyPrint data could take its place"
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

    /// What a copy across volumes sees: `rename` always fails.
    fn no_rename(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::CrossesDevices,
            "other volume",
        ))
    }

    /// A copy that writes half of `queue.json` and then fails (disk full),
    /// copying every other file whole.
    fn failing_copy(src: &Path, dst: &Path) -> io::Result<()> {
        if src.file_name().is_some_and(|n| n == "queue.json") {
            std::fs::write(dst, "[{\"id\":")?;
            return Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"));
        }
        std::fs::copy(src, dst).map(|_| ())
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A copy that fails part-way leaves nothing at the destination — no
    /// truncated file a later run would mistake for the user's — and the
    /// source untouched, so the next run tries again.
    #[test]
    fn a_failed_copy_leaves_no_truncated_file_behind() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("old/queue.json");
        let new = root.path().join("new");
        write(&src, "[{\"id\":1}]");
        std::fs::create_dir_all(&new).unwrap();
        let dst = new.join("queue.json");

        let err = move_entry_with(
            &src,
            &dst,
            &Ops {
                rename: &no_rename,
                copy_file: &failing_copy,
            },
        );
        assert!(err.is_err());
        assert!(!dst.exists(), "no truncated queue.json");
        assert!(
            entries(&new).is_empty(),
            "no temporary left: {:?}",
            entries(&new)
        );
        assert_eq!(read(&src), "[{\"id\":1}]", "the source is intact");

        // The next run (the disk has room again) moves it whole.
        move_entry_with(
            &src,
            &dst,
            &Ops {
                rename: &no_rename,
                copy_file: &copy_file,
            },
        )
        .unwrap();
        assert_eq!(read(&dst), "[{\"id\":1}]");
        assert!(!src.exists());
    }

    /// The same for a folder: one failing file and the whole folder stays
    /// where it was.
    #[test]
    fn a_failed_folder_copy_leaves_no_partial_folder_behind() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("old/data");
        write(&src.join("a.json"), "a");
        write(&src.join("queue.json"), "[]");
        let new = root.path().join("new");
        std::fs::create_dir_all(&new).unwrap();
        let dst = new.join("data");

        assert!(move_entry_with(
            &src,
            &dst,
            &Ops {
                rename: &no_rename,
                copy_file: &failing_copy
            }
        )
        .is_err());
        assert!(!dst.exists());
        assert!(entries(&new).is_empty(), "{:?}", entries(&new));
        assert_eq!(entries(&src), ["a.json", "queue.json"]);

        // A temporary left by a run that died mid-copy is replaced, not merged.
        write(&new.join(temp_name(&dst).unwrap()).join("stale"), "x");
        move_entry_with(
            &src,
            &dst,
            &Ops {
                rename: &no_rename,
                copy_file: &copy_file,
            },
        )
        .unwrap();
        assert_eq!(entries(&dst), ["a.json", "queue.json"]);
        assert_eq!(entries(&new), ["data"]);
        assert!(!src.exists());
    }

    /// Through `migrate`: a failed copy is reported as failed, and the next
    /// run moves the real file rather than "keeping" it next to a stub.
    #[test]
    fn a_failed_copy_is_retried_on_the_next_run() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        write(&old.join("queue.json"), "[1]");
        let mut report = Report::default();
        merge_with(
            &old,
            &new,
            &mut report,
            &Ops {
                rename: &no_rename,
                copy_file: &failing_copy,
            },
        );
        assert_eq!(report.failed.len(), 1, "{report:?}");
        assert!(!new.join("queue.json").exists());

        let mut report = Report::default();
        merge_with(
            &old,
            &new,
            &mut report,
            &Ops {
                rename: &no_rename,
                copy_file: &copy_file,
            },
        );
        assert!(
            report.kept.is_empty() && report.failed.is_empty(),
            "{report:?}"
        );
        assert_eq!(read(&new.join("queue.json")), "[1]");
    }

    /// An old lock that cannot be checked is not evidence MaeclyPrint is
    /// closed: nothing moves.
    #[test]
    fn an_unknown_old_lock_defers_the_migration() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::windows(&root.path().join("Roaming"), &root.path().join("Local"));
        let queue = layout.old_lock.with_file_name("queue.json");
        write(&queue, "[]");
        // A folder where the lock file should be cannot be locked.
        std::fs::create_dir_all(&layout.old_lock).unwrap();

        let report = migrate(&layout);
        assert!(report.deferred.is_some(), "{report:?}");
        assert!(report.moved.is_empty());
        assert!(queue.exists());
    }

    /// The old lock stays held while files move: a MaeclyPrint starting in
    /// the middle cannot take its queue.json back.
    #[test]
    fn the_old_lock_is_held_for_the_whole_move() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::windows(&root.path().join("Roaming"), &root.path().join("Local"));
        // A stale lock left on disk, listed before queue.json as NTFS would.
        write(&layout.old_lock, "");
        write(&layout.old_lock.with_file_name("queue.json"), "[]");
        write(&layout.pairs[0].old.join("config.toml"), "cfg");
        let old_lock = layout.old_lock.clone();
        let seen = std::cell::RefCell::new(Vec::new());
        let probe = |src: &Path, dst: &Path| {
            seen.borrow_mut().push(instance::state(&old_lock));
            std::fs::rename(src, dst)
        };

        let report = migrate_with(
            &layout,
            &Ops {
                rename: &probe,
                copy_file: &copy_file,
            },
        )
        .report;
        assert!(!report.moved.is_empty(), "{report:?}");
        let seen = seen.into_inner();
        assert!(!seen.is_empty());
        assert!(
            seen.iter().all(|s| *s == instance::LockState::Held),
            "{seen:?}"
        );
        assert!(!old_lock.exists(), "the old lock goes with its folder");
        assert!(!root.path().join("Local/Maecly/MaeclyPrint").exists());
    }

    /// A Pluryprint already running owns the new folders: a context-menu
    /// launch or a CLI call must not move files under it.
    #[test]
    fn a_running_pluryprint_defers_the_migration() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::windows(&root.path().join("Roaming"), &root.path().join("Local"));
        let queue = layout.old_lock.with_file_name("queue.json");
        write(&queue, "[]");
        let running = instance::claim(&layout.new_lock).unwrap().expect("lock");

        let report = migrate(&layout);
        assert!(report.deferred.is_some(), "{report:?}");
        assert!(queue.exists());
        assert!(!layout.marker.exists());

        drop(running);
        assert!(migrate(&layout).deferred.is_none());
        assert!(!queue.exists());
    }

    /// Two first launches at once: the second waits while the first holds
    /// the migration lock (for the GUI, until it holds its instance lock),
    /// and never moves files at the same time.
    #[test]
    fn concurrent_migrations_run_one_after_the_other() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::windows(&root.path().join("Roaming"), &root.path().join("Local"));
        write(&layout.old_lock.with_file_name("queue.json"), "[]");

        // Another launch is mid-migration.
        let other = instance::claim(&layout.migration_lock)
            .unwrap()
            .expect("lock");
        let (tx, rx) = std::sync::mpsc::channel();
        let second_layout = layout.clone();
        let second = std::thread::spawn(move || {
            let report = migrate(&second_layout);
            tx.send(()).unwrap();
            report
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(200))
                .is_err(),
            "the second run waits while the first holds the lock"
        );
        assert!(layout.old_lock.with_file_name("queue.json").exists());
        drop(other);
        let report = second.join().unwrap();
        assert!(!report.moved.is_empty(), "{report:?}");

        // A finished migration hands its lock to the caller, and later runs
        // find the marker.
        let done = migrate_with(&layout, &REAL_OPS);
        assert!(done.report.is_empty(), "{:?}", done.report);
    }

    /// The run that migrates returns its lock, for the GUI to keep.
    #[test]
    fn the_migrating_run_hands_back_its_lock() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::windows(&root.path().join("Roaming"), &root.path().join("Local"));
        write(&layout.old_lock.with_file_name("queue.json"), "[]");
        let first = migrate_with(&layout, &REAL_OPS);
        assert!(!first.report.moved.is_empty());
        assert!(first.guard.is_some());
        assert_eq!(
            instance::state(&layout.migration_lock),
            instance::LockState::Held
        );
        drop(first);
        assert_eq!(
            instance::state(&layout.migration_lock),
            instance::LockState::Free
        );
    }

    /// A first launch deferred because MaeclyPrint was running wrote fresh
    /// defaults. The real data still wins later: the defaults are set aside,
    /// the old files move in.
    #[test]
    fn first_run_defaults_give_way_to_the_old_data() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let layout = Layout::macos(h);
        let old = h.join("Library/Application Support/com.Maecly.MaeclyPrint");
        let new = h.join("Library/Application Support/com.Maecly.Pluryprint");
        write(
            &old.join("config.toml"),
            "[app]\nlocale = \"en-US\"\ntheme = \"dark\"\nlaunch_minimized = true\n",
        );
        write(&old.join("profiles.toml"), "old profiles");
        write(&old.join("queue.json"), "old queue");
        write(
            &old.join("update-state.json"),
            r#"{"skipped_version":"v1.2.0"}"#,
        );
        write(&old.join("notes.txt"), "old notes");

        let running = instance::claim(&layout.old_lock).unwrap().expect("lock");
        assert!(migrate(&layout).deferred.is_some());
        drop(running);

        // What the deferred first run of Pluryprint wrote for itself.
        let mut first_run = crate::settings::Config::default();
        first_run.app.locale = "es-ES".into();
        write(
            &new.join("config.toml"),
            &toml::to_string(&first_run).unwrap(),
        );
        write(
            &new.join("profiles.toml"),
            &toml::to_string(&crate::profiles::ProfilesFile::default()).unwrap(),
        );
        write(
            &new.join("queue.json"),
            &serde_json::to_string(&crate::queue::job::QueueSnapshot::default()).unwrap(),
        );
        write(
            &new.join("update-state.json"),
            r#"{"last_check_at":"2026-10-09T00:00:00Z"}"#,
        );
        write(&new.join("notes.txt"), "new notes");

        let report = migrate(&layout);
        assert!(
            report.deferred.is_none() && report.failed.is_empty(),
            "{report:?}"
        );
        assert!(read(&new.join("config.toml")).contains("dark"));
        assert_eq!(read(&new.join("profiles.toml")), "old profiles");
        assert_eq!(read(&new.join("queue.json")), "old queue");
        assert!(read(&new.join("update-state.json")).contains("v1.2.0"));
        assert_eq!(report.backed_up.len(), 4, "{report:?}");
        for (_, backup) in &report.backed_up {
            assert!(backup.exists(), "{backup:?}");
        }
        assert!(read(&new.join("config.toml.first-run.bak")).contains("es-ES"));
        // Not something the app writes for itself: the new one stays.
        assert_eq!(read(&new.join("notes.txt")), "new notes");
        assert_eq!(report.kept, vec![old.join("notes.txt")]);
    }

    /// A setting the user changed in Pluryprint is theirs: it is never set
    /// aside for the old one.
    #[test]
    fn an_edited_new_config_is_kept() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let old = h.join("Library/Application Support/com.Maecly.MaeclyPrint");
        let new = h.join("Library/Application Support/com.Maecly.Pluryprint");
        write(&old.join("config.toml"), "old");
        let mut edited = crate::settings::Config::default();
        edited.app.launch_minimized = true;
        let edited = toml::to_string(&edited).unwrap();
        write(&new.join("config.toml"), &edited);

        let report = migrate(&Layout::macos(h));
        assert_eq!(read(&new.join("config.toml")), edited);
        assert_eq!(read(&old.join("config.toml")), "old");
        assert!(report.backed_up.is_empty());
        assert_eq!(report.kept, vec![old.join("config.toml")]);
    }

    /// Once a migration has run to the end, later launches do nothing and
    /// say nothing — not even about the files it had to keep.
    #[test]
    fn a_finished_migration_is_not_repeated_or_re_reported() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let layout = Layout::macos(h);
        let old = h.join("Library/Application Support/com.Maecly.MaeclyPrint");
        let new = h.join("Library/Application Support/com.Maecly.Pluryprint");
        write(&old.join("notes.txt"), "old");
        write(&new.join("notes.txt"), "new");

        let report = migrate(&layout);
        assert_eq!(report.kept.len(), 1);
        assert!(layout.marker.exists());
        assert!(migrate(&layout).is_empty());
        // Even something new in the old folder (an old app run afterwards).
        write(&old.join("queue.json"), "[]");
        assert!(migrate(&layout).is_empty());
    }

    /// A run that could not move something is retried: no marker.
    #[test]
    fn a_failed_migration_is_retried() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::windows(&root.path().join("Roaming"), &root.path().join("Local"));
        write(&layout.old_lock.with_file_name("queue.json"), "[1]");
        let report = migrate_with(
            &layout,
            &Ops {
                rename: &no_rename,
                copy_file: &failing_copy,
            },
        )
        .report;
        assert_eq!(report.failed.len(), 1, "{report:?}");
        assert!(!layout.marker.exists());

        let report = migrate(&layout);
        assert!(report.failed.is_empty(), "{report:?}");
        assert!(layout.marker.exists());
    }

    /// The webview's storage does not survive the new bundle identifier, so
    /// an upgrade records in config.toml what it used to remember: the tour
    /// was seen, and the language in the migrated config is settled — never
    /// to be overwritten by OS detection.
    #[test]
    fn an_upgrade_records_the_first_run_as_done() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let old = h.join("Library/Application Support/com.Maecly.MaeclyPrint");
        let new = h.join("Library/Application Support/com.Maecly.Pluryprint");
        write(
            &old.join("config.toml"),
            "[app]\nlocale = \"en-US\"\ntheme = \"dark\"\nlaunch_minimized = false\n\n[printers]\ndefault_printer = \"HP\"\n",
        );

        let report = migrate(&Layout::macos(h));
        assert!(report.failed.is_empty(), "{report:?}");
        let c = crate::settings::Config::load_from(&new.join("config.toml"));
        assert!(c.app.locale_detected, "the chosen English stays");
        assert!(c.app.onboarding_complete);
        assert_eq!(c.app.locale, "en-US");
        assert_eq!(c.app.theme, crate::settings::Theme::Dark);
        assert_eq!(c.printers.default_printer.as_deref(), Some("HP"));
    }

    /// An upgrade without a config.toml (no setting was ever changed) still
    /// skips the tour, but leaves the language to detection.
    #[test]
    fn an_upgrade_without_a_config_only_skips_the_tour() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        write(
            &h.join("Library/Application Support/com.Maecly.MaeclyPrint/queue.json"),
            "[]",
        );
        migrate(&Layout::macos(h));
        let c = crate::settings::Config::load_from(
            &h.join("Library/Application Support/com.Maecly.Pluryprint/config.toml"),
        );
        assert!(c.app.onboarding_complete);
        assert!(!c.app.locale_detected);
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
