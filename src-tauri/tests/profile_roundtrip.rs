//! Integration tests for profile persistence (FR-020…FR-027).
//!
//! These drive the public `maeclyprint_lib::profiles` API against a tempdir —
//! never the real config dir — exercising the same pure CRUD core (`upsert`,
//! `delete`) the IPC commands use, plus the atomic `load`/`save` round-trip.

use maeclyprint_lib::profiles::{
    self, delete, upsert, DeleteError, Profile, ProfileDuplex, ProfilesFile, Quality, UpsertError,
    BUILTIN_QUICK,
};
use maeclyprint_lib::queue::job::{ColorMode, Orientation, PaperSize, Scaling};

/// A minimal valid custom profile with an empty id (create semantics).
fn profile(name: &str) -> Profile {
    Profile {
        id: String::new(),
        name: name.to_string(),
        builtin: false,
        default: false,
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
        collate: true,
    }
}

#[test]
fn fr_020_crud_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles.toml");

    // CREATE
    let mut file = profiles::load(&path).unwrap();
    let mut p = profile("Press A");
    p.copies = 3;
    let created = upsert(&mut file, p).unwrap();
    profiles::save(&path, &file).unwrap();
    let id = created.id.clone();
    assert!(!id.is_empty());

    // READ (reload from disk)
    let reloaded = profiles::load(&path).unwrap();
    assert_eq!(reloaded.profile.len(), 1);
    assert_eq!(reloaded.profile[0].name, "Press A");
    assert_eq!(reloaded.profile[0].copies, 3);

    // UPDATE
    let mut file = profiles::load(&path).unwrap();
    let mut edit = profile("Press A Revised");
    edit.id = id.clone();
    edit.copies = 9;
    edit.duplex = ProfileDuplex::LongEdge;
    upsert(&mut file, edit).unwrap();
    profiles::save(&path, &file).unwrap();

    let reloaded = profiles::load(&path).unwrap();
    assert_eq!(reloaded.profile.len(), 1, "update replaced, not appended");
    assert_eq!(reloaded.profile[0].name, "Press A Revised");
    assert_eq!(reloaded.profile[0].copies, 9);
    assert_eq!(reloaded.profile[0].duplex, ProfileDuplex::LongEdge);

    // DELETE
    let mut file = profiles::load(&path).unwrap();
    delete(&mut file, &id, BUILTIN_QUICK).unwrap();
    profiles::save(&path, &file).unwrap();
    let reloaded = profiles::load(&path).unwrap();
    assert!(reloaded.profile.is_empty(), "profile removed from disk");
}

#[test]
fn fr_021_builtins_immutable_but_cloneable() {
    // There are exactly three built-ins, all flagged built-in.
    let b = profiles::builtins();
    assert_eq!(b.len(), 3);
    assert!(b.iter().all(|p| p.builtin));

    let mut file = ProfilesFile::default();

    // Editing a built-in (by its id) is refused.
    let mut hijack = profile("Quick Hijack");
    hijack.id = BUILTIN_QUICK.to_string();
    assert_eq!(upsert(&mut file, hijack), Err(UpsertError::Builtin));
    assert!(file.profile.is_empty());

    // "Cloning" a built-in = saving its values under a new (empty) id: allowed,
    // and the result is a normal editable custom.
    let quick = profiles::builtins().into_iter().next().unwrap();
    let mut clone = quick.clone();
    clone.id = String::new();
    clone.name = "My Quick Copy".to_string();
    let saved = upsert(&mut file, clone).unwrap();
    assert!(!saved.id.is_empty());
    assert_ne!(saved.id, BUILTIN_QUICK);
    assert!(!saved.builtin);
    assert_eq!(file.profile.len(), 1);
}

#[test]
fn fr_022_delete_default_rejected_and_set_default_switches() {
    use maeclyprint_lib::settings::Config;

    let dir = tempfile::tempdir().unwrap();
    let profiles_path = dir.path().join("profiles.toml");
    let config_path = dir.path().join("config.toml");

    // Create a custom and make it the configured default (config side).
    let mut file = profiles::load(&profiles_path).unwrap();
    let created = upsert(&mut file, profile("Press A")).unwrap();
    profiles::save(&profiles_path, &file).unwrap();

    let mut cfg = Config::default();
    cfg.engine.default_profile = Some(created.id.clone());
    cfg.save_to(&config_path).unwrap();

    // The configured default persists and resolves to the custom.
    let loaded_cfg = Config::load_from(&config_path);
    let default_id =
        profiles::effective_default_id(loaded_cfg.engine.default_profile.as_deref(), &file.profile);
    assert_eq!(default_id, created.id);

    // Deleting the current default is refused.
    assert_eq!(
        delete(&mut file, &created.id, &default_id),
        Err(DeleteError::IsDefault)
    );

    // Switch the default away (to the built-in Quick), then the delete succeeds.
    let mut cfg = Config::load_from(&config_path);
    cfg.engine.default_profile = Some(BUILTIN_QUICK.to_string());
    cfg.save_to(&config_path).unwrap();
    let new_default = profiles::effective_default_id(
        Config::load_from(&config_path)
            .engine
            .default_profile
            .as_deref(),
        &file.profile,
    );
    assert_eq!(new_default, BUILTIN_QUICK);
    delete(&mut file, &created.id, &new_default).unwrap();
    profiles::save(&profiles_path, &file).unwrap();
    assert!(profiles::load(&profiles_path).unwrap().profile.is_empty());
}

#[test]
fn fr_026_atomic_save_no_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles.toml");

    let mut file = ProfilesFile::default();
    for i in 0..5 {
        upsert(&mut file, profile(&format!("Profile {i}"))).unwrap();
    }
    profiles::save(&path, &file).unwrap();

    // The directory holds exactly profiles.toml — no leftover temp file.
    let entries: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["profiles.toml".to_string()],
        "no temp residue"
    );

    // Reload equals what was saved.
    let reloaded = profiles::load(&path).unwrap();
    assert_eq!(reloaded, file);
}

#[test]
fn fr_027_five_hundred_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles.toml");

    let mut file = ProfilesFile::default();
    for i in 0..500 {
        upsert(&mut file, profile(&format!("Profile {i:03}"))).unwrap();
    }
    assert_eq!(file.profile.len(), 500);
    profiles::save(&path, &file).unwrap();

    let reloaded = profiles::load(&path).unwrap();
    assert_eq!(reloaded.profile.len(), 500);
    // Names survived the round-trip (unique, so all kept).
    assert!(reloaded.profile.iter().any(|p| p.name == "Profile 000"));
    assert!(reloaded.profile.iter().any(|p| p.name == "Profile 499"));
    assert_eq!(reloaded, file);
}
