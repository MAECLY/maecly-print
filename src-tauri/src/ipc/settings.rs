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
//!
//! Every response also carries [`SettingsMeta`]: whether the config file had to
//! be degraded to defaults (FR-101) and the engine [`Bounds`] the write path
//! enforces, so the screen never has to guess either.

use std::path::Path;

use crate::settings::{
    AppCfg, AppSettings, Bounds, Config, EngineCfg, SettingsMeta, CONFIG_CORRUPT_MSG, CONFIG_LOCK,
    ENGINE_LIMITS,
};
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
/// degrades to defaults ([`Config::load_reporting`]) rather than erroring — but
/// the degradation is *reported* in [`SettingsMeta::config_unreadable`] so the
/// screen can say the values on show are defaults and that saving is refused
/// until the file is fixed (FR-101), instead of the user only finding out when
/// their first change bounces.
pub fn get_app_settings_at(config_path: &Path) -> Result<AppSettings, IpcError> {
    let load = Config::load_reporting(config_path);
    Ok(to_app_settings(
        load.config,
        meta(config_path, load.unreadable),
    ))
}

/// The read-only annotations attached to every settings response.
fn meta(config_path: &Path, config_unreadable: bool) -> SettingsMeta {
    SettingsMeta {
        config_unreadable,
        config_path: config_path.display().to_string(),
        limits: ENGINE_LIMITS,
    }
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
    // `settings.meta` is deliberately dropped: it is server-computed, and the
    // client only ever echoes back what a read handed it.
    cfg.save_to(config_path)?;
    // The file was just written from this very config, so it is readable again.
    Ok(to_app_settings(cfg, meta(config_path, false)))
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

/// Extract the wire subset of a [`Config`], with the server-computed `meta`.
fn to_app_settings(cfg: Config, meta: SettingsMeta) -> AppSettings {
    AppSettings {
        app: cfg.app,
        engine: cfg.engine,
        updater: cfg.updater,
        telemetry: cfg.telemetry,
        meta,
    }
}

/// One knob's rejection message, phrased from its own [`Bounds`] so the numbers
/// here can never drift from the ones that are enforced and shipped to the UI.
fn out_of_range(field: &str, bounds: &Bounds, value: u32) -> String {
    let range = if matches!(bounds.zero, crate::settings::ZeroPolicy::Disables) {
        format!("0 (disabled) or between {} and {}", bounds.min, bounds.max)
    } else {
        format!("between {} and {}", bounds.min, bounds.max)
    };
    format!("{field} must be {range}, got {value}")
}

/// Pure bounds check for the numeric/locale fields, split out so the matrix is
/// unit-testable without touching the filesystem. Every range comes from
/// [`ENGINE_LIMITS`] — the same constant `EngineCfg::sanitized` clamps with and
/// `get_app_settings` ships to the client in `meta.limits`.
fn validate_bounds(app: &AppCfg, engine: &EngineCfg) -> Result<(), String> {
    let checks = [
        (
            "max_concurrent_conversions",
            &ENGINE_LIMITS.max_concurrent_conversions,
            engine.max_concurrent_conversions,
        ),
        (
            "conversion_timeout_seconds",
            &ENGINE_LIMITS.conversion_timeout_seconds,
            engine.conversion_timeout_seconds,
        ),
        (
            "max_file_size_mb",
            &ENGINE_LIMITS.max_file_size_mb,
            engine.max_file_size_mb,
        ),
    ];
    for (field, bounds, value) in checks {
        if !bounds.accepts(value) {
            return Err(out_of_range(field, bounds, value));
        }
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

    // -- limits: one source, enforced and shipped ---------------------------

    #[test]
    fn nfr_engine_limits_are_the_single_source_of_truth() {
        // Contract: what the client is told (`meta.limits`), what the write
        // path rejects (`validate_bounds`) and what sanitization clamps to
        // (`EngineCfg::sanitized`) are all the same numbers. A future change to
        // ENGINE_LIMITS moves all three at once; nothing here restates them.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let shipped = get_app_settings_at(&path).unwrap().meta.limits;
        assert_eq!(shipped, ENGINE_LIMITS);

        let l = &ENGINE_LIMITS;
        for bounds in [
            &l.max_concurrent_conversions,
            &l.conversion_timeout_seconds,
            &l.max_file_size_mb,
        ] {
            // The advertised default and both ends of the range are accepted…
            assert!(bounds.accepts(bounds.default));
            assert!(bounds.accepts(bounds.min));
            assert!(bounds.accepts(bounds.max));
            // …and anything past either end is not (`0` included: the only
            // knob that accepts it is the timeout, whose min is well above 1).
            assert!(!bounds.accepts(bounds.max + 1));
            assert!(!bounds.accepts(bounds.min - 1));
            // Sanitization lands inside the advertised range.
            assert!(bounds.accepts(bounds.sanitize(0)));
            assert!(bounds.accepts(bounds.sanitize(u32::MAX)));
        }

        // The per-field boundaries validate_bounds enforces are exactly those.
        let (a, e) = cfg(
            l.max_concurrent_conversions.max,
            l.conversion_timeout_seconds.max,
            l.max_file_size_mb.max,
            "en-US",
        );
        assert!(validate_bounds(&a, &e).is_ok());
        let (a, e) = cfg(
            l.max_concurrent_conversions.max + 1,
            l.conversion_timeout_seconds.max,
            l.max_file_size_mb.max,
            "en-US",
        );
        let msg = validate_bounds(&a, &e).unwrap_err();
        assert!(
            msg.contains(&l.max_concurrent_conversions.max.to_string()),
            "the rejection quotes the enforced ceiling: {msg}"
        );
    }

    // -- FR-101: a corrupt config is reported, not swallowed -----------------

    #[test]
    fn fr_101_corrupt_config_degrades_and_is_reported_to_the_ui() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let profiles_path = dir.path().join("profiles.toml");
        std::fs::write(&config_path, b"this is = not valid = toml ][").unwrap();

        let got = get_app_settings_at(&config_path).unwrap();
        // Values fall back to defaults…
        assert_eq!(got.app, Config::default().app);
        // …and the response says so, naming the file the user has to fix.
        assert!(got.meta.config_unreadable);
        assert_eq!(got.meta.config_path, config_path.display().to_string());

        // The refusal on write is unchanged: the corrupt bytes stay put.
        let err = set_app_settings_at(&config_path, &profiles_path, got).unwrap_err();
        assert!(matches!(err, IpcError::Internal(_)), "got {err:?}");

        // A readable file reports healthy.
        Config::default().save_to(&config_path).unwrap();
        assert!(
            !get_app_settings_at(&config_path)
                .unwrap()
                .meta
                .config_unreadable
        );
    }

    #[test]
    fn missing_config_is_not_reported_as_unreadable() {
        // A fresh install has no file at all — that is the normal case, and
        // showing a scary banner for it would be a lie.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let got = get_app_settings_at(&path).unwrap();
        assert!(!got.meta.config_unreadable);
    }

    #[test]
    fn client_supplied_meta_is_ignored_and_never_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let profiles_path = dir.path().join("profiles.toml");

        let mut settings = get_app_settings_at(&config_path).unwrap();
        settings.meta = SettingsMeta {
            config_unreadable: true,
            config_path: "/somewhere/else.toml".to_string(),
            limits: ENGINE_LIMITS,
        };
        let saved = set_app_settings_at(&config_path, &profiles_path, settings).unwrap();

        // The server recomputes it from the file it just wrote…
        assert!(!saved.meta.config_unreadable);
        assert_eq!(saved.meta.config_path, config_path.display().to_string());
        // …and none of it leaks into config.toml.
        let raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(!raw.contains("meta"), "raw:\n{raw}");
        assert!(!raw.contains("somewhere/else"), "raw:\n{raw}");
    }

    // -- FR-100/FR-102: settings survive a restart --------------------------

    #[test]
    fn fr_100_settings_survive_a_restart() {
        // "Restart" = nothing but the file carries state between the two
        // halves: the second read shares no memory with the first write, which
        // is exactly what a relaunch does.
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let profiles_path = dir.path().join("profiles.toml");

        let mut first = get_app_settings_at(&config_path).unwrap();
        first.app.theme = Theme::Dark;
        first.app.density = Density::Compact;
        first.app.locale = "es-ES".to_string();
        first.app.launch_minimized = true;
        first.engine.max_concurrent_conversions = 5;
        first.engine.conversion_timeout_seconds = 0;
        first.engine.max_file_size_mb = 512;
        first.updater.channel = UpdateChannel::Beta;
        first.updater.check_on_startup = false;
        first.telemetry.crash_reports = true;
        set_app_settings_at(&config_path, &profiles_path, first.clone()).unwrap();

        let after_restart = get_app_settings_at(&config_path).unwrap();
        assert_eq!(after_restart.app, first.app);
        assert_eq!(after_restart.engine, first.engine);
        assert_eq!(after_restart.updater, first.updater);
        assert_eq!(after_restart.telemetry, first.telemetry);

        // The startup reader sees the same thing without any IPC.
        assert!(crate::settings::launch_minimized_at(&config_path));

        // A second session's change persists on top of the first.
        let mut second = after_restart;
        second.app.theme = Theme::Light;
        set_app_settings_at(&config_path, &profiles_path, second).unwrap();
        let third = get_app_settings_at(&config_path).unwrap();
        assert_eq!(third.app.theme, Theme::Light);
        assert_eq!(third.app.locale, "es-ES", "unrelated fields are untouched");
        assert_eq!(third.updater.channel, UpdateChannel::Beta);
    }

    #[test]
    fn launch_minimized_defaults_to_false_when_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(!crate::settings::launch_minimized_at(&path), "missing file");
        std::fs::write(&path, b"garbage ][").unwrap();
        assert!(!crate::settings::launch_minimized_at(&path), "corrupt file");
    }
}
