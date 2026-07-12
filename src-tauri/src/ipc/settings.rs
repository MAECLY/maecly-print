//! App-settings IPC commands.
//!
//! [`get_app_settings`] / [`set_app_settings`] expose the four wire sections of
//! `config.toml` (`app`, `engine`, `updater`, `telemetry`) as one
//! [`AppSettings`] bundle. The `printers` section is intentionally excluded —
//! it is owned by the Printers screen ([`crate::ipc::printers`]) — and
//! `schema_version` is server-managed, so a write preserves both.
//!
//! Engine changes (`max_concurrent_conversions`, `conversion_timeout_seconds`,
//! `max_file_size_mb`) take effect on the next app start: the queue reads them
//! once at boot. Telemetry flags are **consent-only** — this version transmits
//! nothing regardless of their value.

use std::path::Path;

use crate::settings::{AppCfg, AppSettings, Config, EngineCfg, CONFIG_CORRUPT_MSG, CONFIG_LOCK};
use crate::util::error::IpcError;

// As in `ipc::profiles`, each `#[tauri::command]` is a thin wrapper over a
// path-injected `*_at` core so the integration tests in `tests/` can drive the
// refuse/degrade logic against a tempdir. The `*_at` cores are `pub` for that
// reason only; they carry no `#[specta::specta]`, so `bindings.ts` is unchanged.

/// Read the current app settings (the four wire sections of `config.toml`).
#[tauri::command]
#[specta::specta]
pub async fn get_app_settings() -> Result<AppSettings, IpcError> {
    super::run_blocking(|| get_app_settings_at(&Config::config_path())).await
}

/// Path-injected core of [`get_app_settings`]. A read, so a corrupt config
/// degrades to defaults ([`Config::load_from`]) rather than erroring.
pub fn get_app_settings_at(config_path: &Path) -> Result<AppSettings, IpcError> {
    Ok(to_app_settings(Config::load_from(config_path)))
}

/// Validate and persist the app settings, returning the saved value.
///
/// Under [`CONFIG_LOCK`] this loads the full config, replaces exactly the four
/// wire sections (keeping `printers` and `schema_version`), and atomically
/// saves. Validation failures are returned as [`IpcError::InvalidRequest`]
/// naming the offending field; nothing is written when validation fails.
#[tauri::command]
#[specta::specta]
pub async fn set_app_settings(settings: AppSettings) -> Result<AppSettings, IpcError> {
    super::run_blocking(move || {
        set_app_settings_at(
            &Config::config_path(),
            &crate::profiles::profiles_path(),
            settings,
        )
    })
    .await
}

/// Path-injected core of [`set_app_settings`].
///
/// - The config is loaded through [`Config::load_strict`], so a
///   corrupt `config.toml` is *refused* — never silently rewritten as
///   defaults + this patch (which would erase printer prefs/routes/overrides).
/// - The `default_profile` existence check runs **only when the value
///   changed** from what is stored. An already-stored dangling default (from a
///   hand-edit or a race) therefore no longer bricks every theme/density/
///   telemetry save. When a *changed* default is validated and `profiles.toml`
///   is unreadable, the save is still refused, but with a message naming
///   `profiles.toml`.
pub fn set_app_settings_at(
    config_path: &Path,
    profiles_path: &Path,
    settings: AppSettings,
) -> Result<AppSettings, IpcError> {
    // Pure bounds first — no I/O, no lock.
    validate_bounds(&settings.app, &settings.engine).map_err(IpcError::InvalidRequest)?;

    let _guard = CONFIG_LOCK.lock().unwrap();
    let mut cfg = Config::load_strict(config_path)
        .map_err(|_| IpcError::Internal(CONFIG_CORRUPT_MSG.to_string()))?;

    // Only validate the default profile's existence when this call *changes* it.
    // An unchanged value — even a dangling one already on disk — passes through.
    if settings.engine.default_profile != cfg.engine.default_profile {
        if let Some(id) = settings.engine.default_profile.as_deref() {
            validate_default_profile_exists(id, profiles_path)?;
        }
    }

    cfg.app = settings.app;
    cfg.engine = settings.engine;
    cfg.updater = settings.updater;
    cfg.telemetry = settings.telemetry;
    cfg.save_to(config_path)?;
    Ok(to_app_settings(cfg))
}

/// Confirm `id` names a known profile (built-in or a stored custom). A read of
/// `profiles.toml` is enough (config writes are atomic). Unlike a read command,
/// an unreadable `profiles.toml` here is *not* tolerated: the caller is changing
/// the default and we cannot confirm the target, so we refuse rather than accept
/// a possibly-dangling id — with a message that names the offending file.
fn validate_default_profile_exists(id: &str, profiles_path: &Path) -> Result<(), IpcError> {
    let customs = crate::profiles::load(profiles_path)
        .map_err(|_| {
            IpcError::Internal(
                "profiles.toml could not be parsed — fix or remove it; your settings were not modified"
                    .to_string(),
            )
        })?
        .profile;
    if !crate::profiles::is_known_id(id, &customs) {
        return Err(IpcError::InvalidRequest(format!(
            "default_profile '{id}' does not name a known profile"
        )));
    }
    Ok(())
}

/// Extract the wire subset of a [`Config`].
fn to_app_settings(cfg: Config) -> AppSettings {
    AppSettings {
        app: cfg.app,
        engine: cfg.engine,
        updater: cfg.updater,
        telemetry: cfg.telemetry,
    }
}

/// Pure bounds check for the numeric/locale fields, split out so the matrix is
/// unit-testable without touching the filesystem.
fn validate_bounds(app: &AppCfg, engine: &EngineCfg) -> Result<(), String> {
    if !(1..=8).contains(&engine.max_concurrent_conversions) {
        return Err(format!(
            "max_concurrent_conversions must be between 1 and 8, got {}",
            engine.max_concurrent_conversions
        ));
    }
    let timeout = engine.conversion_timeout_seconds;
    if timeout != 0 && !(10..=3600).contains(&timeout) {
        return Err(format!(
            "conversion_timeout_seconds must be 0 (disabled) or between 10 and 3600, got {timeout}"
        ));
    }
    if !(1..=4096).contains(&engine.max_file_size_mb) {
        return Err(format!(
            "max_file_size_mb must be between 1 and 4096, got {}",
            engine.max_file_size_mb
        ));
    }
    if app.locale.trim().is_empty() {
        return Err("locale must not be empty".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Density, Theme, UpdateChannel, UpdaterCfg};

    fn cfg(max_concurrent: u32, timeout: u32, max_mb: u32, locale: &str) -> (AppCfg, EngineCfg) {
        (
            AppCfg {
                locale: locale.to_string(),
                theme: Theme::System,
                launch_minimized: false,
                density: Density::Comfortable,
            },
            EngineCfg {
                max_concurrent_conversions: max_concurrent,
                conversion_timeout_seconds: timeout,
                max_file_size_mb: max_mb,
                default_profile: None,
            },
        )
    }

    #[test]
    fn bounds_accept_valid_matrix() {
        for (mc, to, mb) in [(1, 0, 1), (8, 10, 4096), (2, 60, 200), (4, 3600, 1024)] {
            let (a, e) = cfg(mc, to, mb, "en-US");
            assert!(
                validate_bounds(&a, &e).is_ok(),
                "expected ok for {mc}/{to}/{mb}"
            );
        }
    }

    #[test]
    fn bounds_reject_out_of_range() {
        let (a, e) = cfg(0, 60, 200, "en-US");
        assert!(validate_bounds(&a, &e).is_err(), "max_concurrent 0");
        let (a, e) = cfg(9, 60, 200, "en-US");
        assert!(validate_bounds(&a, &e).is_err(), "max_concurrent 9");
        let (a, e) = cfg(2, 5, 200, "en-US");
        assert!(
            validate_bounds(&a, &e).is_err(),
            "timeout 5 (below 10, non-zero)"
        );
        let (a, e) = cfg(2, 3601, 200, "en-US");
        assert!(validate_bounds(&a, &e).is_err(), "timeout 3601");
        let (a, e) = cfg(2, 60, 0, "en-US");
        assert!(validate_bounds(&a, &e).is_err(), "max_file_size 0");
        let (a, e) = cfg(2, 60, 4097, "en-US");
        assert!(validate_bounds(&a, &e).is_err(), "max_file_size 4097");
        let (a, e) = cfg(2, 60, 200, "   ");
        assert!(validate_bounds(&a, &e).is_err(), "empty locale");
    }

    #[test]
    fn timeout_zero_is_allowed() {
        let (a, e) = cfg(2, 0, 200, "en-US");
        assert!(validate_bounds(&a, &e).is_ok(), "0 disables the timeout");
    }

    #[test]
    fn updater_defaults_are_sane() {
        let u = UpdaterCfg::default();
        assert_eq!(u.channel, UpdateChannel::Stable);
        assert!(u.check_on_startup);
    }
}
