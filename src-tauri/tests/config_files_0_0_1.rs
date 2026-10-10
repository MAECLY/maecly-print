//! FR-100 / FR-026 — `config.toml` and `profiles.toml` exactly as 0.0.1 wrote
//! them still load.
//!
//! The fixtures in `tests/fixtures/0.0.1/` are the bytes `Config::save_to` and
//! `profiles::save` produced with the `toml` 0.8 serializer that shipped in
//! 0.0.1, every section and every optional key populated (string escapes,
//! quoted keys, nested tables, arrays of tables). A serializer or parser
//! upgrade must keep reading them field for field: these are files already on
//! users' disks, and an unreadable one means lost settings and profiles.

use std::path::{Path, PathBuf};

use pluryprint_lib::profiles::{self, ProfileDuplex, ProfilesFile, Quality, StoredProfile};
use pluryprint_lib::queue::job::{ColorMode, Orientation, PaperSize, Scaling};
use pluryprint_lib::settings::{Config, Density, PrinterOverride, Theme, UpdateChannel};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/0.0.1")
        .join(name)
}

/// What the `config.toml` fixture holds.
fn populated_config() -> Config {
    let mut c = Config::default();
    c.app.locale = "es-ES".into();
    c.app.theme = Theme::Dark;
    c.app.launch_minimized = true;
    c.app.density = Density::Compact;
    c.app.native_notifications = false;
    c.app.locale_detected = true;
    c.app.onboarding_complete = true;
    c.printers.default_printer = Some("HP LaserJet \"Oficina\" 2º".into());
    c.printers.disabled = vec!["Old_Fax".into(), r"\\server\Shared Printer".into()];
    c.printers.routes.insert(".pdf".into(), "Brother_HL".into());
    c.printers
        .routes
        .insert(".docx".into(), "HP LaserJet \"Oficina\" 2º".into());
    c.printers.overrides.insert(
        "Brother_HL".into(),
        PrinterOverride {
            tray: Some("Tray 1".into()),
            paper: Some("A4".into()),
        },
    );
    c.printers.overrides.insert(
        r"\\server\Shared Printer".into(),
        PrinterOverride {
            tray: None,
            paper: Some("Letter".into()),
        },
    );
    c.engine.max_concurrent_conversions = 4;
    c.engine.conversion_timeout_seconds = 0;
    c.engine.max_file_size_mb = 512;
    c.engine.print_timeout_seconds = 900;
    c.engine.default_profile = Some("builtin-photo-color".into());
    c.updater.channel = UpdateChannel::Beta;
    c.updater.check_on_startup = false;
    c.telemetry.crash_reports = true;
    c.telemetry.anonymous_usage_metrics = true;
    c
}

/// What the `profiles.toml` fixture holds.
fn populated_profiles() -> ProfilesFile {
    ProfilesFile {
        schema_version: 1,
        profile: vec![
            StoredProfile {
                id: "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b".into(),
                name: "Facturas \"dúplex\"".into(),
                printer: "<default>".into(),
                color_mode: ColorMode::Monochrome,
                duplex: ProfileDuplex::LongEdge,
                copies: 2,
                paper_size: PaperSize::A4,
                orientation: Orientation::Portrait,
                scaling: Scaling::Fit,
                page_range: Some("1-3, 5".into()),
                quality: Quality::Draft,
                tray: "auto".into(),
                collate: false,
            },
            StoredProfile {
                id: "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5c".into(),
                name: "Planos A3".into(),
                printer: r"\\server\Shared Printer".into(),
                color_mode: ColorMode::Color,
                duplex: ProfileDuplex::Off,
                copies: 1,
                paper_size: PaperSize::A3,
                orientation: Orientation::Landscape,
                scaling: Scaling::None,
                page_range: None,
                quality: Quality::High,
                tray: "Tray 2".into(),
                collate: true,
            },
        ],
    }
}

#[test]
fn fr_100_a_config_toml_written_by_0_0_1_loads_field_for_field() {
    let loaded = Config::load_strict(&fixture("config.toml"))
        .expect("the 0.0.1 config.toml parses with the current toml crate");
    assert_eq!(loaded, populated_config());
}

#[test]
fn fr_026_a_profiles_toml_written_by_0_0_1_loads_field_for_field() {
    let loaded = profiles::load(&fixture("profiles.toml"))
        .expect("the 0.0.1 profiles.toml parses with the current toml crate");
    assert_eq!(loaded, populated_profiles());
}

/// Saving over a 0.0.1 file rewrites it with the current serializer, and that
/// reads back to the same values, so a user who saves once after upgrading
/// keeps every setting and profile.
#[test]
fn fr_100_fr_026_resaving_a_0_0_1_file_keeps_every_value() {
    let dir = tempfile::tempdir().unwrap();

    let config = dir.path().join("config.toml");
    std::fs::copy(fixture("config.toml"), &config).unwrap();
    Config::load_strict(&config)
        .unwrap()
        .save_to(&config)
        .unwrap();
    assert_eq!(Config::load_strict(&config).unwrap(), populated_config());

    let profiles_path = dir.path().join("profiles.toml");
    std::fs::copy(fixture("profiles.toml"), &profiles_path).unwrap();
    profiles::save(&profiles_path, &profiles::load(&profiles_path).unwrap()).unwrap();
    assert_eq!(
        profiles::load(&profiles_path).unwrap(),
        populated_profiles()
    );
}
