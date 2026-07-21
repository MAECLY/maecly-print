//! Command-layer coverage for the settings/profiles IPC surface.
//!
//! The other integration suites drive the pure store functions; these drive the
//! path-injected cores *behind* the `#[tauri::command]` wrappers
//! (`*_at(&Path, …)`), so the degrade-vs-refuse decision logic each command owns
//! is exercised directly against a tempdir. They cover:
//!
//! - FIX 1: a corrupt `config.toml` makes `set_app_settings` refuse.
//! - FIX 2: an already-stored dangling default no longer bricks unrelated saves;
//!   a *changed* default to a nonexistent id is still rejected.
//! - FIX 3: a `profiles.toml` with duplicate names degrades on read but is
//!   refused (bytes untouched) on save/delete.
//! - FIX 4: read paths skip ghost rows (blank/`builtin-` prefixed ids).
//! - FIX 5: a corrupt `profiles.toml` never blocks switching the default to a
//!   built-in id.
//!
//! It also owns the drift gate for the generated frontend bindings — see
//! `bindings_ts_is_committed_in_sync`.

use maeclyprint_lib::ipc::profiles::{
    delete_profile_at, list_profiles_at, save_profile_at, set_default_profile_at,
};
use maeclyprint_lib::ipc::settings::set_app_settings_at;
use maeclyprint_lib::profiles::{
    self, Profile, ProfileDuplex, ProfilesFile, Quality, StoredProfile, BUILTIN_QUICK,
};
use maeclyprint_lib::queue::job::{ColorMode, Orientation, PaperSize, Scaling};
use maeclyprint_lib::settings::{AppSettings, Config, Theme};

const GARBAGE: &[u8] = b"not = valid = toml ][";

fn stored(id: &str, name: &str) -> StoredProfile {
    StoredProfile {
        id: id.to_string(),
        name: name.to_string(),
        printer: "<default>".to_string(),
        color_mode: ColorMode::Color,
        duplex: ProfileDuplex::Off,
        copies: 1,
        paper_size: PaperSize::A4,
        orientation: Orientation::Auto,
        scaling: Scaling::Fit,
        page_range: None,
        quality: Quality::Normal,
        tray: "auto".to_string(),
    }
}

fn new_profile(name: &str) -> Profile {
    stored("", name).to_profile(false, false)
}

fn app_settings_from(cfg: &Config) -> AppSettings {
    AppSettings {
        app: cfg.app.clone(),
        engine: cfg.engine.clone(),
        updater: cfg.updater.clone(),
        telemetry: cfg.telemetry.clone(),
    }
}

// ---------------------------------------------------------------------------
// FIX 8 + FIX 3 + FIX 4: profiles read degrades, mutation refuses.
// ---------------------------------------------------------------------------

#[test]
fn fr_regression_fix8_corrupt_profiles_list_degrades_to_builtins() {
    let dir = tempfile::tempdir().unwrap();
    let profiles_path = dir.path().join("profiles.toml");
    let config_path = dir.path().join("config.toml");
    std::fs::write(&profiles_path, GARBAGE).unwrap();

    let list = list_profiles_at(&profiles_path, &config_path).unwrap();
    // Exactly the three built-ins, no customs, and no error.
    assert_eq!(list.len(), 3);
    assert!(list.iter().all(|p| p.builtin));
}

#[test]
fn fr_regression_fix8_corrupt_profiles_save_refuses_bytes_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let profiles_path = dir.path().join("profiles.toml");
    let config_path = dir.path().join("config.toml");
    std::fs::write(&profiles_path, GARBAGE).unwrap();

    let result = save_profile_at(&profiles_path, &config_path, new_profile("New One"));
    assert!(result.is_err(), "save must refuse a corrupt profiles.toml");
    assert_eq!(std::fs::read(&profiles_path).unwrap(), GARBAGE);
}

#[test]
fn fr_regression_fix3_duplicate_names_degrade_read_refuse_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let profiles_path = dir.path().join("profiles.toml");
    let config_path = dir.path().join("config.toml");

    let mut first = stored("c-1", "Press A");
    first.copies = 3;
    let dup = stored("c-2", "press a"); // case-insensitive clash
    profiles::save(
        &profiles_path,
        &ProfilesFile {
            schema_version: 1,
            profile: vec![first, dup],
        },
    )
    .unwrap();
    let before = std::fs::read(&profiles_path).unwrap();

    // READ degrades: built-ins + exactly one of the duplicate-named customs.
    let list = list_profiles_at(&profiles_path, &config_path).unwrap();
    let customs: Vec<_> = list.iter().filter(|p| !p.builtin).collect();
    assert_eq!(customs.len(), 1, "one duplicate-named custom survives read");

    // MUTATION of an UNRELATED profile refuses, leaving the file byte-for-byte.
    let save = save_profile_at(
        &profiles_path,
        &config_path,
        new_profile("Totally Unrelated"),
    );
    assert!(save.is_err(), "save must refuse a duplicate-name file");
    let del = delete_profile_at(&profiles_path, &config_path, "c-1".to_string());
    assert!(del.is_err(), "delete must refuse a duplicate-name file");
    assert_eq!(
        std::fs::read(&profiles_path).unwrap(),
        before,
        "profiles.toml bytes must be untouched by a refused mutation"
    );
}

#[test]
fn fr_regression_fix4_ghost_rows_skipped_on_read() {
    let dir = tempfile::tempdir().unwrap();
    let profiles_path = dir.path().join("profiles.toml");
    let config_path = dir.path().join("config.toml");

    profiles::save(
        &profiles_path,
        &ProfilesFile {
            schema_version: 1,
            profile: vec![
                stored("builtin-quick", "Ghost Builtin"),
                stored("", "Ghost Empty"),
                stored("c-1", "Real Custom"),
            ],
        },
    )
    .unwrap();

    let list = list_profiles_at(&profiles_path, &config_path).unwrap();
    // 3 built-ins + only the one addressable custom.
    assert_eq!(list.len(), 4);
    let customs: Vec<_> = list.iter().filter(|p| !p.builtin).collect();
    assert_eq!(customs.len(), 1);
    assert_eq!(customs[0].id, "c-1");
    assert_eq!(customs[0].name, "Real Custom");
}

// ---------------------------------------------------------------------------
// FIX 2: dangling default no longer bricks unrelated saves.
// ---------------------------------------------------------------------------

#[test]
fn fr_regression_fix2_dangling_default_still_allows_theme_save() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let profiles_path = dir.path().join("profiles.toml");

    // Config already stores a default pointing at a profile that doesn't exist.
    let mut cfg = Config::default();
    cfg.app.theme = Theme::Light;
    cfg.engine.default_profile = Some("ghost-id-123".to_string());
    cfg.save_to(&config_path).unwrap();

    // A theme change that leaves default_profile untouched must succeed even
    // though the stored default dangles.
    let mut settings = app_settings_from(&Config::load_from(&config_path));
    settings.app.theme = Theme::Dark;
    let saved = set_app_settings_at(&config_path, &profiles_path, settings)
        .expect("unrelated save must not be bricked by a dangling default");
    assert_eq!(saved.app.theme, Theme::Dark);

    // Persisted: theme changed, the (still dangling) default preserved verbatim.
    let reloaded = Config::load_from(&config_path);
    assert_eq!(reloaded.app.theme, Theme::Dark);
    assert_eq!(
        reloaded.engine.default_profile.as_deref(),
        Some("ghost-id-123")
    );
}

#[test]
fn fr_regression_fix2_changed_default_to_nonexistent_rejects() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let profiles_path = dir.path().join("profiles.toml");

    Config::default().save_to(&config_path).unwrap();

    // Now *change* the default to an id that names no profile → rejected.
    let mut settings = app_settings_from(&Config::load_from(&config_path));
    settings.engine.default_profile = Some("does-not-exist".to_string());
    let result = set_app_settings_at(&config_path, &profiles_path, settings);
    assert!(
        result.is_err(),
        "changing the default to a nonexistent id must be rejected"
    );
    // The rejected write left the stored default at its original (None) value.
    assert!(Config::load_from(&config_path)
        .engine
        .default_profile
        .is_none());
}

// ---------------------------------------------------------------------------
// FIX 5: a corrupt profiles.toml must not block escaping a dangling default via
// a built-in id.
// ---------------------------------------------------------------------------

#[test]
fn fr_regression_fix5_set_default_builtin_survives_corrupt_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let profiles_path = dir.path().join("profiles.toml");

    // A dangling default in config, and a corrupt profiles.toml that a custom-id
    // check would choke on.
    let mut cfg = Config::default();
    cfg.engine.default_profile = Some("ghost-id".to_string());
    cfg.save_to(&config_path).unwrap();
    std::fs::write(&profiles_path, GARBAGE).unwrap();

    // Switching to a built-in never reads profiles.toml, so it succeeds.
    set_default_profile_at(&profiles_path, &config_path, BUILTIN_QUICK.to_string())
        .expect("built-in default must be settable despite corrupt profiles.toml");

    assert_eq!(
        Config::load_from(&config_path)
            .engine
            .default_profile
            .as_deref(),
        Some(BUILTIN_QUICK)
    );
}

// ---------------------------------------------------------------------------
// Generated-bindings drift gate.
// ---------------------------------------------------------------------------

/// `src/ipc/bindings.ts` as **committed**: `include_str!` freezes the file at
/// compile time, i.e. before a single test has run. Cargo tracks it as an input
/// of this target, so editing the file forces a rebuild and refreshes this
/// constant.
const COMMITTED_BINDINGS: &str = include_str!("../../src/ipc/bindings.ts");

/// Path of the same file on disk, read back *after* the run has regenerated it.
fn bindings_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/ipc/bindings.ts")
}

/// First line where the two revisions disagree, rendered for a panic message.
fn first_difference(committed: &str, regenerated: &str) -> String {
    for (i, (old, new)) in committed.lines().zip(regenerated.lines()).enumerate() {
        if old != new {
            return format!(
                "first difference at line {}:\n  committed:   {old}\n  regenerated: {new}",
                i + 1
            );
        }
    }
    format!(
        "line counts differ: committed {}, regenerated {}",
        committed.lines().count(),
        regenerated.lines().count()
    )
}

/// The committed `src/ipc/bindings.ts` must already match the Rust command
/// surface; a stale one fails the build instead of being silently rewritten.
///
/// How it works: `bindings_sync::bindings_are_current` (src/lib.rs) regenerates
/// the file from the live `specta_builder()` earlier in the same `cargo test`
/// run — cargo runs the lib's unit tests before the integration targets. So the
/// bytes now on disk are the *regenerated* ones, while `COMMITTED_BINDINGS`
/// still holds what was committed. They differ exactly when the checked-in file
/// was out of date. (Running this target alone — `cargo test --test
/// ipc_surface` — skips that regeneration, so the check only bites on a full
/// `cargo test`, which is what CI runs.)
///
/// Regenerating on purpose: the refreshed file is already in the working tree
/// when this fails, so the fix is to review and commit it. To refresh without
/// failing the run, set `UPDATE_BINDINGS=1` (the only supported way to treat a
/// rewrite as intentional):
///
/// ```text
/// UPDATE_BINDINGS=1 cargo test --manifest-path src-tauri/Cargo.toml
/// ```
#[test]
fn bindings_ts_is_committed_in_sync() {
    let path = bindings_path();
    let regenerated = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    if regenerated == COMMITTED_BINDINGS {
        return;
    }

    if std::env::var_os("UPDATE_BINDINGS").is_some() {
        eprintln!(
            "UPDATE_BINDINGS set: {} was refreshed from the Rust command surface — commit it.",
            path.display()
        );
        return;
    }

    panic!(
        "src/ipc/bindings.ts is out of sync with the Rust command surface.\n\
         {}\n\n\
         The regenerated file is already in your working tree — review and commit it.\n\
         To regenerate without failing the run:\n  \
         UPDATE_BINDINGS=1 cargo test --manifest-path src-tauri/Cargo.toml\n  \
         (PowerShell: $env:UPDATE_BINDINGS=1; cargo test --manifest-path src-tauri/Cargo.toml)",
        first_difference(COMMITTED_BINDINGS, &regenerated)
    );
}

/// Every `#[tauri::command]` in the tree must be listed in the `invoke_handler`
/// in `src/lib.rs`.
///
/// An unregistered command compiles, passes its own unit tests and looks
/// finished in review, but never reaches `src/ipc/bindings.ts`, so the frontend
/// has no way to call it. This campaign shipped that bug twice —
/// `expand_folders` (folder support implemented and unreachable) and
/// `classify_paths` (dead on arrival) — which is why it is a gate rather than a
/// habit.
///
/// The check is textual on purpose: it needs to see the *source*, since an
/// unregistered command leaves no trace in the compiled artifact to inspect.
#[test]
fn every_tauri_command_is_registered_in_the_invoke_handler() {
    use std::collections::BTreeSet;

    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let lib_rs = std::fs::read_to_string(src.join("lib.rs")).expect("read lib.rs");

    // The `collect_commands![...]` list, which is what actually gets wired up.
    let registered: BTreeSet<String> = lib_rs
        .split_once("collect_commands![")
        .map(|(_, rest)| rest.split_once(']').map(|(list, _)| list).unwrap_or(""))
        .unwrap_or("")
        .split(',')
        .filter_map(|entry| entry.trim().rsplit("::").next())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    assert!(
        !registered.is_empty(),
        "could not parse collect_commands! out of lib.rs — has the macro moved?"
    );

    let mut declared: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read source");
            // Match the attribute only when it is the whole line, so prose that
            // *mentions* `#[tauri::command]` is not mistaken for one. The
            // function follows within a few lines, after `#[specta::specta]`
            // and any doc comments.
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.trim() != "#[tauri::command]" {
                    continue;
                }
                let name = lines[i + 1..]
                    .iter()
                    .find_map(|l| l.split_whitespace().skip_while(|t| *t != "fn").nth(1))
                    .map(|tok| tok.split(['(', '<']).next().unwrap_or(tok));
                if let Some(name) = name {
                    declared.insert(name.to_owned());
                }
            }
        }
    }
    assert!(
        !declared.is_empty(),
        "found no #[tauri::command] functions — has the scan broken?"
    );

    let missing: Vec<&String> = declared.difference(&registered).collect();
    assert!(
        missing.is_empty(),
        "these #[tauri::command] functions are not in the collect_commands! list \
         in src/lib.rs, so the frontend cannot call them: {missing:?}\n\
         Either register them or drop the attribute if they are not IPC."
    );
}
