//! Profile IPC commands (CRUD, set-default, and TOML import/export).
//!
//! Thin wrappers over [`crate::profiles`]: they own the locking
//! ([`PROFILES_LOCK`], and [`CONFIG_LOCK`] where the config's default is
//! written) and the on-disk read/write, while the pure logic (validation,
//! ordering, default resolution) lives in the domain module.
//!
//! Read commands ([`list_profiles`]/[`get_profile`]) tolerate a corrupt
//! `profiles.toml` by degrading to built-ins only. Mutating commands refuse to
//! touch a corrupt file — returning [`IpcError::Internal`] — so a hand-broken
//! file is never silently clobbered.

use std::path::Path;

use crate::profiles::{
    self, effective_default_id, is_builtin_id, DeleteError, MutationLoadError, Profile,
    ProfilesFile, UpsertError, PROFILES_LOCK,
};
use crate::settings::{Config, CONFIG_CORRUPT_MSG, CONFIG_LOCK};
use crate::util::error::IpcError;

// Each `#[tauri::command]` below is a thin transport wrapper that resolves the
// real config-dir paths and defers to a path-injected `*_at` core. The cores are
// `pub` (not the usual `pub(crate)`) purely so the integration tests in
// `tests/` — a separate crate — can drive the degrade/refuse decision logic
// against a tempdir instead of the live per-user config. They are NOT part of
// the IPC/binding surface (no `#[specta::specta]`), so exposing them does not
// change `bindings.ts`.

/// List every profile: built-ins in canonical order, then customs sorted
/// case-insensitively by name. The `default` flag is computed from
/// `engine.default_profile`.
#[tauri::command]
#[specta::specta]
pub async fn list_profiles() -> Result<Vec<Profile>, IpcError> {
    super::run_blocking(|| list_profiles_at(&profiles::profiles_path(), &Config::config_path()))
        .await
}

/// Path-injected core of [`list_profiles`].
pub fn list_profiles_at(
    profiles_path: &Path,
    config_path: &Path,
) -> Result<Vec<Profile>, IpcError> {
    let file = load_for_read(profiles_path);
    let default_id = current_default_id(config_path, &file);
    Ok(profiles::list_all(&file.profile, &default_id))
}

/// Fetch one profile by id (built-in or custom), with its computed flags.
#[tauri::command]
#[specta::specta]
pub async fn get_profile(id: String) -> Result<Profile, IpcError> {
    super::run_blocking(move || {
        get_profile_at(&profiles::profiles_path(), &Config::config_path(), id)
    })
    .await
}

/// Path-injected core of [`get_profile`].
pub fn get_profile_at(
    profiles_path: &Path,
    config_path: &Path,
    id: String,
) -> Result<Profile, IpcError> {
    let file = load_for_read(profiles_path);
    let default_id = current_default_id(config_path, &file);

    if let Some(mut b) = profiles::builtins().into_iter().find(|b| b.id == id) {
        b.default = b.id == default_id;
        return Ok(b);
    }
    if let Some(c) = file.profile.iter().find(|c| c.id == id) {
        return Ok(c.to_profile(false, c.id == default_id));
    }
    Err(IpcError::ProfileNotFound(id))
}

/// Create or update a custom profile (upsert), returning the saved wire profile
/// with computed flags.
///
/// Id semantics:
/// - empty id → **create** with a fresh UUIDv7;
/// - id of a built-in → [`IpcError::InvalidRequest`] (built-ins are immutable);
/// - id of a known custom → **update** it;
/// - unknown non-empty id → **create preserving that id** (this is what makes a
///   client-side "undo delete" restore the same id).
///
/// The input's `builtin`/`default` flags are ignored; validation always runs.
#[tauri::command]
#[specta::specta]
pub async fn save_profile(profile: Profile) -> Result<Profile, IpcError> {
    super::run_blocking(move || {
        save_profile_at(&profiles::profiles_path(), &Config::config_path(), profile)
    })
    .await
}

/// Path-injected core of [`save_profile`].
pub fn save_profile_at(
    profiles_path: &Path,
    config_path: &Path,
    profile: Profile,
) -> Result<Profile, IpcError> {
    let _guard = PROFILES_LOCK.lock().unwrap();
    let mut file = load_for_mutation(profiles_path)?;

    let mut normalized = profiles::upsert(&mut file, profile).map_err(|e| match e {
        UpsertError::Builtin => {
            IpcError::InvalidRequest("built-in profiles cannot be modified".to_string())
        }
        UpsertError::IdTooLong => {
            IpcError::InvalidRequest("profile id must be 100 characters or fewer".to_string())
        }
        UpsertError::Invalid(msg) => IpcError::InvalidRequest(msg),
    })?;

    profiles::save(profiles_path, &file)?;

    // `upsert` already cleared the computed flags; stamp the real default.
    let default_id = current_default_id(config_path, &file);
    normalized.default = normalized.id == default_id;
    tracing::info!(event = "profile_saved", profile_id = %normalized.id);
    Ok(normalized)
}

/// Delete a custom profile. Rejects deleting a built-in or the current effective
/// default (the user must switch the default first), and reports
/// [`IpcError::ProfileNotFound`] for an unknown id.
#[tauri::command]
#[specta::specta]
pub async fn delete_profile(id: String) -> Result<(), IpcError> {
    super::run_blocking(move || {
        delete_profile_at(&profiles::profiles_path(), &Config::config_path(), id)
    })
    .await
}

/// Path-injected core of [`delete_profile`].
pub fn delete_profile_at(
    profiles_path: &Path,
    config_path: &Path,
    id: String,
) -> Result<(), IpcError> {
    // Reject built-ins before touching the file, so the attempt never even
    // reads (or risks a corrupt-file error masking) the real reason.
    if is_builtin_id(&id) {
        return Err(IpcError::InvalidRequest(
            "built-in profiles cannot be deleted".to_string(),
        ));
    }

    let _guard = PROFILES_LOCK.lock().unwrap();
    let mut file = load_for_mutation(profiles_path)?;
    let default_id = current_default_id(config_path, &file);

    profiles::delete(&mut file, &id, &default_id).map_err(|e| match e {
        DeleteError::NotFound => IpcError::ProfileNotFound(id.clone()),
        DeleteError::IsDefault => {
            IpcError::InvalidRequest("set another profile as default first".to_string())
        }
        DeleteError::Builtin => {
            IpcError::InvalidRequest("built-in profiles cannot be deleted".to_string())
        }
    })?;

    profiles::save(profiles_path, &file)?;
    tracing::info!(event = "profile_deleted", profile_id = %id);
    Ok(())
}

/// Set the app's default profile. The id must name a known profile (built-in or
/// custom); the choice is written to `config.toml`'s `engine.default_profile`
/// under [`CONFIG_LOCK`].
#[tauri::command]
#[specta::specta]
pub async fn set_default_profile(id: String) -> Result<(), IpcError> {
    super::run_blocking(move || {
        set_default_profile_at(&profiles::profiles_path(), &Config::config_path(), id)
    })
    .await
}

/// Path-injected core of [`set_default_profile`].
///
/// A **built-in** id is checked against the built-in set *first* and needs no
/// `profiles.toml` read, so a corrupt `profiles.toml` never closes the
/// only UI escape from a dangling default. A **custom** id must exist in
/// `profiles.toml`, which the mutation loader parses (refusing a corrupt or
/// duplicate/invalid file). Either way the write goes through
/// [`Config::load_strict`] so a corrupt `config.toml` is refused, never
/// overwritten with defaults.
pub fn set_default_profile_at(
    profiles_path: &Path,
    config_path: &Path,
    id: String,
) -> Result<(), IpcError> {
    if is_builtin_id(&id) {
        let _cguard = CONFIG_LOCK.lock().unwrap();
        return persist_default(config_path, &id);
    }

    // Custom id: it must exist. Lock order is PROFILES_LOCK → CONFIG_LOCK (see
    // the statics' docs) so this can't deadlock against the other config writers.
    let _pguard = PROFILES_LOCK.lock().unwrap();
    let file = load_for_mutation(profiles_path)?;
    if !profiles::is_known_id(&id, &file.profile) {
        return Err(IpcError::ProfileNotFound(id));
    }

    let _cguard = CONFIG_LOCK.lock().unwrap();
    persist_default(config_path, &id)
}

/// Export one profile — built-in or custom — to a standalone TOML document at
/// `path`, so it can be shared across machines (FR-024).
///
/// The document holds the profile's *settings* only: the two computed flags are
/// not part of the on-disk shape, so an exported file can never carry "I am the
/// default" or "I am built in" onto another machine. The write is atomic, like
/// every other profile write (FR-026).
#[tauri::command]
#[specta::specta]
pub async fn export_profile(id: String, path: String) -> Result<(), IpcError> {
    super::run_blocking(move || {
        export_profile_at(
            &profiles::profiles_path(),
            &Config::config_path(),
            id,
            Path::new(&path),
        )
    })
    .await
}

/// Path-injected core of [`export_profile`].
pub fn export_profile_at(
    profiles_path: &Path,
    config_path: &Path,
    id: String,
    dest: &Path,
) -> Result<(), IpcError> {
    let profile = get_profile_at(profiles_path, config_path, id)?;
    profiles::save(dest, &profiles::export_document(&profile))?;
    tracing::info!(event = "profile_exported", profile_id = %profile.id);
    Ok(())
}

/// Import profiles from a TOML document (FR-024), returning the newly created
/// profiles with their computed flags.
///
/// The file is **untrusted input**: it is size-capped, parsed and validated in
/// full before `profiles.toml` is even opened, and a single invalid entry
/// rejects the whole document, so a rejected import can never leave a partial
/// merge behind.
///
/// Collision policy (see [`profiles::merge_imported`]): every imported profile
/// gets a **fresh id**, so an import only ever *adds* — it can neither
/// overwrite an existing custom nor touch the immutable built-ins — and a name
/// that collides case-insensitively is renamed `"{name} 2"`, `"{name} 3"`… the
/// same way Clone does.
#[tauri::command]
#[specta::specta]
pub async fn import_profiles(path: String) -> Result<Vec<Profile>, IpcError> {
    super::run_blocking(move || {
        import_profiles_at(
            &profiles::profiles_path(),
            &Config::config_path(),
            Path::new(&path),
        )
    })
    .await
}

/// Path-injected core of [`import_profiles`].
pub fn import_profiles_at(
    profiles_path: &Path,
    config_path: &Path,
    src: &Path,
) -> Result<Vec<Profile>, IpcError> {
    // Parse and structurally validate before taking the lock: a bad file must
    // cost the user nothing but an error message.
    let doc = profiles::read_document(src).map_err(|e| IpcError::InvalidRequest(e.to_string()))?;

    let _guard = PROFILES_LOCK.lock().unwrap();
    let mut file = load_for_mutation(profiles_path)?;

    // `merge_imported` mutates `file` in place, so an error must return before
    // `save` — the on-disk file is only ever written after every entry passed.
    let imported = profiles::merge_imported(&mut file, &doc.profile)
        .map_err(|e| IpcError::InvalidRequest(e.to_string()))?;
    profiles::save(profiles_path, &file)?;

    let default_id = current_default_id(config_path, &file);
    let imported: Vec<Profile> = imported
        .into_iter()
        .map(|mut p| {
            p.default = p.id == default_id;
            p
        })
        .collect();
    tracing::info!(event = "profiles_imported", count = imported.len());
    Ok(imported)
}

/// Write `engine.default_profile = id` to `config.toml`. The caller must already
/// hold [`CONFIG_LOCK`]. Uses [`Config::load_strict`] so a corrupt config is
/// refused rather than silently rewritten as defaults + this one field.
fn persist_default(config_path: &Path, id: &str) -> Result<(), IpcError> {
    let mut cfg = Config::load_strict(config_path)
        .map_err(|_| IpcError::Internal(CONFIG_CORRUPT_MSG.to_string()))?;
    cfg.engine.default_profile = Some(id.to_string());
    cfg.save_to(config_path)?;
    tracing::info!(event = "default_profile_set", profile_id = %id);
    Ok(())
}

/// Load `profiles.toml` for a read-only command: a corrupt file degrades to
/// built-ins only (empty customs) with a warning, never failing the command.
fn load_for_read(path: &Path) -> ProfilesFile {
    match profiles::load(path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(
                event = "profiles_read_degraded",
                error = %e,
                "profiles.toml unreadable; showing built-in profiles only"
            );
            ProfilesFile::default()
        }
    }
}

/// Load `profiles.toml` for a mutating command: an unreadable/corrupt file, or
/// one holding duplicate names or invalid stored ids, is an error so we never
/// overwrite (and lose) data we could not safely round-trip.
fn load_for_mutation(path: &Path) -> Result<ProfilesFile, IpcError> {
    profiles::load_for_mutation(path).map_err(|e| match e {
        MutationLoadError::Unreadable(_) => IpcError::Internal(
            "profiles.toml could not be parsed — fix or remove it".to_string(),
        ),
        MutationLoadError::InvalidEntries => IpcError::Internal(
            "profiles.toml contains duplicate or invalid entries — fix or remove it; nothing was modified"
                .to_string(),
        ),
    })
}

/// The effective default id given the config at `config_path` and the loaded
/// customs. Reads keep the degrade-to-default semantics
/// ([`Config::load_from`]) — only *mutations* refuse a corrupt config.
fn current_default_id(config_path: &Path, file: &ProfilesFile) -> String {
    let cfg_default = Config::load_from(config_path).engine.default_profile;
    effective_default_id(cfg_default.as_deref(), &file.profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::{StoredProfile, BUILTIN_PHOTO_COLOR, BUILTIN_QUICK};
    use crate::queue::job::{ColorMode, Orientation, PaperSize, Scaling};

    /// A tempdir standing in for the config dir. Nothing here reads the real
    /// per-user config: both paths are injected, and an absent `config.toml`
    /// means "no configured default", which resolves to `builtin-quick`.
    struct Env {
        _dir: tempfile::TempDir,
        profiles: std::path::PathBuf,
        config: std::path::PathBuf,
    }

    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        Env {
            profiles: dir.path().join("profiles.toml"),
            config: dir.path().join("config.toml"),
            _dir: dir,
        }
    }

    fn stored(id: &str, name: &str) -> StoredProfile {
        StoredProfile {
            id: id.to_string(),
            name: name.to_string(),
            printer: "<default>".to_string(),
            color_mode: ColorMode::Color,
            duplex: crate::profiles::ProfileDuplex::Off,
            copies: 1,
            paper_size: PaperSize::A4,
            orientation: Orientation::Auto,
            scaling: Scaling::Fit,
            page_range: None,
            quality: crate::profiles::Quality::Normal,
            tray: "auto".to_string(),
            collate: true,
        }
    }

    fn write_customs(env: &Env, customs: Vec<StoredProfile>) {
        crate::profiles::save(
            &env.profiles,
            &ProfilesFile {
                schema_version: 1,
                profile: customs,
            },
        )
        .unwrap();
    }

    // -- get_profile --------------------------------------------------------

    #[test]
    fn get_profile_returns_a_builtin_with_the_default_flag_computed() {
        let env = env();
        let quick = get_profile_at(&env.profiles, &env.config, BUILTIN_QUICK.to_string()).unwrap();
        assert!(quick.builtin);
        assert!(quick.default, "Quick is the fallback default");

        let photo =
            get_profile_at(&env.profiles, &env.config, BUILTIN_PHOTO_COLOR.to_string()).unwrap();
        assert!(photo.builtin);
        assert!(!photo.default);
    }

    #[test]
    fn get_profile_returns_a_custom_and_never_marks_it_builtin() {
        let env = env();
        write_customs(&env, vec![stored("c-1", "Press A")]);

        let p = get_profile_at(&env.profiles, &env.config, "c-1".to_string()).unwrap();
        assert_eq!(p.name, "Press A");
        assert!(!p.builtin);
        assert!(!p.default);
    }

    #[test]
    fn get_profile_reports_profile_not_found_for_an_unknown_id() {
        let env = env();
        let err = get_profile_at(&env.profiles, &env.config, "ghost".to_string()).unwrap_err();
        assert!(
            matches!(err, IpcError::ProfileNotFound(ref id) if id == "ghost"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn get_profile_degrades_to_builtins_when_profiles_toml_is_corrupt() {
        let env = env();
        std::fs::write(&env.profiles, b"not = valid = toml ][").unwrap();
        // A read still answers for built-ins…
        assert!(get_profile_at(&env.profiles, &env.config, BUILTIN_QUICK.to_string()).is_ok());
        // …and a custom simply isn't there.
        assert!(matches!(
            get_profile_at(&env.profiles, &env.config, "c-1".to_string()),
            Err(IpcError::ProfileNotFound(_))
        ));
    }

    // -- export / import (FR-024) -------------------------------------------

    #[test]
    fn fr_024_export_then_import_round_trips_a_custom_profile() {
        let env = env();
        let mut custom = stored("c-1", "Press A");
        custom.copies = 7;
        custom.page_range = Some("1-3".to_string());
        write_customs(&env, vec![custom]);

        let dest = env.profiles.with_file_name("Press A.maecly-profile.toml");
        export_profile_at(&env.profiles, &env.config, "c-1".to_string(), &dest).unwrap();
        assert!(dest.exists());

        let imported = import_profiles_at(&env.profiles, &env.config, &dest).unwrap();
        assert_eq!(imported.len(), 1);
        // Same settings, new identity, de-duplicated name.
        assert_eq!(imported[0].copies, 7);
        assert_eq!(imported[0].page_range.as_deref(), Some("1-3"));
        assert_eq!(imported[0].name, "Press A 2");
        assert_ne!(imported[0].id, "c-1");

        let all = list_profiles_at(&env.profiles, &env.config).unwrap();
        assert_eq!(all.len(), 5, "3 built-ins + the original + the import");
    }

    #[test]
    fn fr_024_exporting_a_builtin_produces_an_importable_custom() {
        let env = env();
        let dest = env.profiles.with_file_name("quick.maecly-profile.toml");
        export_profile_at(&env.profiles, &env.config, BUILTIN_QUICK.to_string(), &dest).unwrap();

        let imported = import_profiles_at(&env.profiles, &env.config, &dest).unwrap();
        assert_eq!(imported[0].name, "Quick 2");
        assert!(
            !imported[0].builtin,
            "an imported profile is always a custom"
        );
        assert!(!imported[0].id.starts_with("builtin-"));
    }

    #[test]
    fn fr_024_export_rejects_an_unknown_id_without_writing() {
        let env = env();
        let dest = env.profiles.with_file_name("ghost.toml");
        assert!(matches!(
            export_profile_at(&env.profiles, &env.config, "ghost".to_string(), &dest),
            Err(IpcError::ProfileNotFound(_))
        ));
        assert!(!dest.exists());
    }

    #[test]
    fn fr_024_import_of_a_corrupt_file_leaves_the_store_untouched() {
        let env = env();
        write_customs(&env, vec![stored("c-1", "Press A")]);
        let before = std::fs::read(&env.profiles).unwrap();

        let bad = env.profiles.with_file_name("bad.toml");
        std::fs::write(&bad, b"<<<not a profile document>>>").unwrap();

        let err = import_profiles_at(&env.profiles, &env.config, &bad).unwrap_err();
        assert!(
            matches!(err, IpcError::InvalidRequest(_)),
            "unexpected error: {err:?}"
        );
        assert_eq!(
            std::fs::read(&env.profiles).unwrap(),
            before,
            "nothing was written"
        );
    }

    #[test]
    fn fr_024_import_rejects_the_whole_document_when_one_entry_is_invalid() {
        let env = env();
        write_customs(&env, vec![stored("c-1", "Press A")]);
        let before = std::fs::read(&env.profiles).unwrap();

        let mut bad = stored("b", "Bad One");
        bad.copies = 0;
        let doc = ProfilesFile {
            schema_version: 1,
            profile: vec![stored("a", "Good One"), bad],
        };
        let src = env.profiles.with_file_name("bundle.toml");
        std::fs::write(&src, toml::to_string_pretty(&doc).unwrap()).unwrap();

        assert!(matches!(
            import_profiles_at(&env.profiles, &env.config, &src),
            Err(IpcError::InvalidRequest(_))
        ));
        assert_eq!(
            std::fs::read(&env.profiles).unwrap(),
            before,
            "an all-or-nothing import wrote nothing"
        );
    }

    #[test]
    fn fr_024_import_cannot_overwrite_an_existing_custom_or_a_builtin() {
        let env = env();
        let mut mine = stored("c-1", "Press A");
        mine.copies = 3;
        write_customs(&env, vec![mine]);

        // A hostile document reusing a local id and a reserved built-in id.
        let mut hijack_custom = stored("c-1", "Hijack A");
        hijack_custom.copies = 99;
        let doc = ProfilesFile {
            schema_version: 1,
            profile: vec![hijack_custom, stored("builtin-quick", "Hijack Quick")],
        };
        let src = env.profiles.with_file_name("hostile.toml");
        std::fs::write(&src, toml::to_string_pretty(&doc).unwrap()).unwrap();

        let imported = import_profiles_at(&env.profiles, &env.config, &src).unwrap();
        assert_eq!(imported.len(), 2);

        let all = list_profiles_at(&env.profiles, &env.config).unwrap();
        let original = all.iter().find(|p| p.id == "c-1").expect("still there");
        assert_eq!(original.copies, 3, "the local profile was not overwritten");
        let quick = all
            .iter()
            .find(|p| p.id == BUILTIN_QUICK)
            .expect("built-in intact");
        assert_eq!(quick.name, "Quick");
        assert!(quick.builtin);
    }

    #[test]
    fn fr_024_import_refuses_to_touch_a_corrupt_profiles_toml() {
        let env = env();
        std::fs::write(&env.profiles, b"not = valid = toml ][").unwrap();
        let before = std::fs::read(&env.profiles).unwrap();

        let src = env.profiles.with_file_name("ok.toml");
        std::fs::write(
            &src,
            toml::to_string_pretty(&ProfilesFile {
                schema_version: 1,
                profile: vec![stored("a", "Fine")],
            })
            .unwrap(),
        )
        .unwrap();

        assert!(matches!(
            import_profiles_at(&env.profiles, &env.config, &src),
            Err(IpcError::Internal(_))
        ));
        assert_eq!(std::fs::read(&env.profiles).unwrap(), before);
    }
}
