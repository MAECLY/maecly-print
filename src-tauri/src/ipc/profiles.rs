//! Profile IPC commands (CRUD + set-default).
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
