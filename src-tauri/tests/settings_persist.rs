//! Integration tests for `config.toml` persistence (FR-100…FR-103).
//!
//! Path-injected `Config::load_from`/`save_to` are driven against a tempdir so
//! the real per-user config is never touched. These cover the human-readable
//! TOML shape, atomic writes, corrupt-file fallback, the embedded schema
//! version, and that a pre-settings (v1) file still loads with the new sections
//! defaulted.

use maeclyprint_lib::settings::{AppSettings, Config, Density, PrinterPrefs, Theme, UpdateChannel};

#[test]
fn fr_100_toml_shape_human_readable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let mut cfg = Config::default();
    cfg.app.theme = Theme::Dark;
    cfg.app.density = Density::Compact;
    cfg.updater.channel = UpdateChannel::Beta;
    cfg.engine.max_file_size_mb = 512;
    cfg.save_to(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    // Section headers and readable string values are present.
    assert!(raw.contains("[app]"), "raw:\n{raw}");
    assert!(raw.contains("[engine]"));
    assert!(raw.contains("[updater]"));
    assert!(raw.contains("[telemetry]"));
    assert!(raw.contains("theme = \"dark\""));
    assert!(raw.contains("density = \"compact\""));
    assert!(raw.contains("channel = \"beta\""));
    assert!(raw.contains("max_file_size_mb = 512"));
}

#[test]
fn fr_101_corrupt_config_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, b"this is not = valid = toml ][").unwrap();

    // A corrupt file degrades to defaults rather than erroring.
    let cfg = Config::load_from(&path);
    assert_eq!(cfg, Config::default());
}

#[test]
fn fr_102_atomic_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let mut cfg = Config::default();
    cfg.telemetry.crash_reports = true;
    cfg.save_to(&path).unwrap();

    // No temp residue: the directory holds exactly config.toml.
    let entries: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["config.toml".to_string()]);

    // Reload equals what was written.
    let reloaded = Config::load_from(&path);
    assert_eq!(reloaded, cfg);
}

#[test]
fn fr_103_schema_version_embedded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    Config::default().save_to(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        raw.contains("schema_version = 1"),
        "schema version embedded in the file:\n{raw}"
    );
}

#[test]
fn additive_sections_load_from_v1_file() {
    // A config.toml written before the engine/updater/telemetry sections (and
    // before app.density) existed must still load, filling the gaps with
    // defaults — no data loss, no parse error.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "schema_version = 1\n\
         [app]\n\
         locale = \"en-US\"\n\
         theme = \"dark\"\n\
         launch_minimized = true\n\
         [printers]\n\
         disabled = [\"Old_Fax\"]\n",
    )
    .unwrap();

    let cfg = Config::load_from(&path);
    // Preserved from the old file.
    assert_eq!(cfg.app.theme, Theme::Dark);
    assert!(cfg.app.launch_minimized);
    assert_eq!(cfg.printers.disabled, vec!["Old_Fax".to_string()]);
    // Defaulted new fields/sections.
    assert_eq!(cfg.app.density, Density::Comfortable);
    assert_eq!(cfg.engine, Default::default());
    assert_eq!(cfg.updater.channel, UpdateChannel::Stable);
    assert!(!cfg.telemetry.crash_reports);
}

// ---------------------------------------------------------------------------
// FIX 1 (regression): a CORRUPT config.toml must never be silently rewritten as
// defaults + patch by a mutator. Each of the three config mutators, driven at
// the command-core layer against a tempdir, must REFUSE and leave the file's
// bytes exactly as they were.
// ---------------------------------------------------------------------------

const GARBAGE_CONFIG: &[u8] = b"this is = not valid = toml ][ printers.default = ";

fn default_app_settings() -> AppSettings {
    let cfg = Config::default();
    AppSettings {
        app: cfg.app,
        engine: cfg.engine,
        updater: cfg.updater,
        telemetry: cfg.telemetry,
    }
}

#[test]
fn fr_regression_fix1_set_app_settings_refuses_corrupt_config() {
    use maeclyprint_lib::ipc::settings::set_app_settings_at;

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let profiles_path = dir.path().join("profiles.toml");
    std::fs::write(&config_path, GARBAGE_CONFIG).unwrap();

    let result = set_app_settings_at(&config_path, &profiles_path, default_app_settings());
    assert!(result.is_err(), "corrupt config.toml must be refused");
    assert_eq!(
        std::fs::read(&config_path).unwrap(),
        GARBAGE_CONFIG,
        "config.toml bytes must be untouched"
    );
}

#[test]
fn fr_regression_fix1_set_printer_prefs_refuses_corrupt_config() {
    use maeclyprint_lib::ipc::printers::set_printer_prefs_at;

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, GARBAGE_CONFIG).unwrap();

    let result = set_printer_prefs_at(&config_path, PrinterPrefs::default());
    assert!(result.is_err(), "corrupt config.toml must be refused");
    assert_eq!(
        std::fs::read(&config_path).unwrap(),
        GARBAGE_CONFIG,
        "config.toml bytes must be untouched"
    );
}

#[test]
fn fr_regression_fix1_set_default_profile_refuses_corrupt_config() {
    use maeclyprint_lib::ipc::profiles::set_default_profile_at;
    use maeclyprint_lib::profiles::BUILTIN_QUICK;

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let profiles_path = dir.path().join("profiles.toml");
    std::fs::write(&config_path, GARBAGE_CONFIG).unwrap();

    // A built-in id skips the profiles read entirely, so the only thing that can
    // fail is the config write — which must refuse rather than clobber.
    let result = set_default_profile_at(&profiles_path, &config_path, BUILTIN_QUICK.to_string());
    assert!(result.is_err(), "corrupt config.toml must be refused");
    assert_eq!(
        std::fs::read(&config_path).unwrap(),
        GARBAGE_CONFIG,
        "config.toml bytes must be untouched"
    );
}
