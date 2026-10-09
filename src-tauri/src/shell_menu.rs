//! The Explorer context-menu label cache (`<data_dir>/shell-menu.json`).
//!
//! The Windows shell extension (`shell-ext-win`) draws its menu on Explorer's
//! UI thread and cannot ask the app anything while it does, so it reads a small
//! pre-localized cache instead (`shell-ext-win/src/strings.rs`). This module is
//! the writer: it renders the labels from the same i18n catalog the UI uses
//! (`src/i18n/locales/<lang>/common.json`, section `shellMenu`) and lists up to
//! [`MAX_MENU_PROFILES`] profiles — built-ins first, then customs by name, the
//! order `list_profiles` returns.
//!
//! [`refresh`] is called at startup and after every change that alters the
//! menu: profiles saved, deleted, imported or re-defaulted, and the UI language
//! changed. It is best effort — a failure is logged, never surfaced: the DLL
//! falls back to built-in English labels, which is still a working menu.
//!
//! The file is only written on Windows (nothing else reads it); building and
//! writing it are portable and tested everywhere. The shape is pinned against
//! the reader by `shell-ext-win/tests/fixtures/shell-menu.es.json`, which both
//! crates' tests read.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::profiles::Profile;

/// File name inside the app's local data directory. Must match
/// `shell_ext_win::strings::CACHE_FILE`.
pub const CACHE_FILE: &str = "shell-menu.json";

/// Must match `shell_ext_win::strings::CACHE_SCHEMA_VERSION`.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// Must match `shell_ext_win::strings::MAX_MENU_PROFILES`; more would be
/// dropped by the reader anyway.
pub const MAX_MENU_PROFILES: usize = 8;

/// The catalogs, compiled in so the writer needs no frontend at run time.
const EN_COMMON: &str = include_str!("../../src/i18n/locales/en/common.json");
const ES_COMMON: &str = include_str!("../../src/i18n/locales/es/common.json");

/// The cache document. Field names are the reader's contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShellMenuCache {
    pub schema_version: u32,
    /// Language subtag of the labels (`en`, `es`). Diagnostics only.
    pub lang: String,
    pub labels: Labels,
    pub profiles: Vec<MenuProfile>,
}

/// Labels in the reader's placeholder syntax (`{count}`, `{name}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Labels {
    pub root: String,
    pub print_one: String,
    pub print_many: String,
    pub print_with_profile: String,
    pub configure: String,
    pub queue: String,
    pub tooltip: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MenuProfile {
    pub id: String,
    pub name: String,
}

/// The catalog language for an app locale (`es-ES` → `es`); anything without
/// a catalog is English, the same fallback the UI uses.
fn catalog_lang(locale: &str) -> (&'static str, &'static str) {
    let lang = locale
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match lang.as_str() {
        "es" => ("es", ES_COMMON),
        _ => ("en", EN_COMMON),
    }
}

/// One `shellMenu.<key>` string, converted from i18n's `{{param}}` to the
/// reader's `{param}`. Falls back to English for a key missing in the catalog
/// (the parity test makes that impossible for shipped catalogs).
fn label(catalog: &str, key: &str) -> String {
    let lookup = |raw: &str| -> Option<String> {
        let doc: serde_json::Value = serde_json::from_str(raw).ok()?;
        doc.get("shellMenu")?
            .get(key)?
            .as_str()
            .map(|s| s.replace("{{", "{").replace("}}", "}"))
    };
    lookup(catalog)
        .or_else(|| lookup(EN_COMMON))
        .unwrap_or_default()
}

/// Build the cache for `locale` and the profiles in display order.
pub fn build(locale: &str, profiles: &[Profile]) -> ShellMenuCache {
    let (lang, catalog) = catalog_lang(locale);
    ShellMenuCache {
        schema_version: CACHE_SCHEMA_VERSION,
        lang: lang.to_string(),
        labels: Labels {
            // The product name is not translated.
            root: "MaeclyPrint".to_string(),
            print_one: label(catalog, "print_one"),
            print_many: label(catalog, "print_other"),
            print_with_profile: label(catalog, "printWithProfile"),
            configure: label(catalog, "configure"),
            queue: label(catalog, "queue"),
            tooltip: label(catalog, "tooltip"),
        },
        profiles: profiles
            .iter()
            .take(MAX_MENU_PROFILES)
            .map(|p| MenuProfile {
                id: p.id.clone(),
                name: p.name.clone(),
            })
            .collect(),
    }
}

/// Write `cache` into `dir` atomically (temp file + rename), so Explorer never
/// reads half a file.
///
/// The temp file has a **unique** name. The GUI (startup, profile and settings
/// commands) and the CLI (`profiles …`) refresh the cache from separate
/// processes, and with one fixed `shell-menu.json.tmp` a second writer's
/// truncate-and-write could land in the file the first was renaming, leaving
/// a mixed JSON document the shell extension rejects (English labels, no
/// profiles). Each writer now renames only its own complete file; the last
/// rename wins.
pub fn write_to(dir: &Path, cache: &ShellMenuCache) -> io::Result<PathBuf> {
    use std::io::Write as _;
    std::fs::create_dir_all(dir)?;
    let path = dir.join(CACHE_FILE);
    let bytes = serde_json::to_vec_pretty(cache).map_err(io::Error::other)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!("{CACHE_FILE}."))
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(&bytes)?;
    tmp.flush()?;
    // `persist` replaces an existing cache file on every platform; on failure
    // the temp file is removed when `tmp` drops.
    tmp.persist(&path).map_err(|e| e.error)?;
    Ok(path)
}

/// Rebuild the cache from the real config and profiles. Windows only; a no-op
/// elsewhere. Never fails: problems are logged.
pub fn refresh() {
    #[cfg(windows)]
    refresh_at(
        &crate::util::paths::data_dir(),
        &crate::settings::Config::config_path(),
        &crate::profiles::profiles_path(),
    );
}

/// [`refresh`] with injected paths: reads the locale from `config_path` and
/// the profiles from `profiles_path`, writes into `data_dir`.
pub fn refresh_at(data_dir: &Path, config_path: &Path, profiles_path: &Path) {
    let locale = crate::settings::Config::load_strict(config_path)
        .map(|c| c.app.locale)
        .unwrap_or_default();
    let profiles = match crate::ipc::profiles::list_profiles_at(profiles_path, config_path) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(event = "shell_menu_cache_failed", error = %e);
            return;
        }
    };
    match write_to(data_dir, &build(&locale, &profiles)) {
        Ok(path) => tracing::debug!(event = "shell_menu_cache_written", path = %path.display()),
        Err(e) => tracing::warn!(event = "shell_menu_cache_failed", error = %e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles(n: usize) -> Vec<Profile> {
        let all = crate::profiles::builtins();
        let base = all.first().expect("there are built-in profiles").clone();
        (0..n)
            .map(|i| Profile {
                id: format!("p{i}"),
                name: format!("Profile {i}"),
                ..base.clone()
            })
            .collect()
    }

    /// FR-001/FR-092: Spanish labels come from the shared catalog, in the
    /// reader's placeholder syntax.
    #[test]
    fn fr_001_spanish_labels_from_the_catalog() {
        let c = build("es-ES", &[]);
        assert_eq!(c.lang, "es");
        assert_eq!(c.labels.print_one, "Imprimir 1 archivo");
        assert_eq!(c.labels.print_many, "Imprimir {count} archivos");
        assert_eq!(c.labels.print_with_profile, "Imprimir con «{name}»");
        assert!(!c.labels.configure.contains("{{"));
    }

    #[test]
    fn fr_001_unknown_locales_fall_back_to_english() {
        for locale in ["en-US", "fr-FR", ""] {
            let c = build(locale, &[]);
            assert_eq!(c.lang, "en", "{locale}");
            assert_eq!(c.labels.print_many, "Print {count} files");
        }
    }

    /// FR-027: the menu lists at most eight profiles, in the order given.
    #[test]
    fn fr_027_profiles_are_capped_and_ordered() {
        let c = build("en-US", &profiles(11));
        assert_eq!(c.profiles.len(), MAX_MENU_PROFILES);
        assert_eq!(c.profiles[0].id, "p0");
        assert_eq!(c.profiles[7].name, "Profile 7");
    }

    #[test]
    fn write_is_atomic_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_to(dir.path(), &build("es-ES", &profiles(2))).unwrap();
        assert_eq!(path, dir.path().join(CACHE_FILE));
        assert!(!dir.path().join(format!("{CACHE_FILE}.tmp")).exists());
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["schema_version"], CACHE_SCHEMA_VERSION);
        assert_eq!(v["profiles"].as_array().unwrap().len(), 2);
    }

    /// The writer's output for a fixed input is byte-for-byte the fixture the
    /// shell extension's own tests parse, so a field renamed on either side
    /// fails one of the two crates.
    #[test]
    fn fr_001_matches_the_fixture_the_shell_extension_reads() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../shell-ext-win/tests/fixtures/shell-menu.es.json");
        let expected = std::fs::read_to_string(&fixture).expect("fixture exists");
        let actual = serde_json::to_string_pretty(&build("es-ES", &profiles(2))).unwrap();
        assert_eq!(actual.trim_end(), expected.trim_end());
    }

    /// The end-to-end path with injected files: locale from config.toml,
    /// profiles from profiles.toml (built-ins only here).
    #[test]
    fn refresh_at_reads_locale_and_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let mut cfg = crate::settings::Config::default();
        cfg.app.locale = "es-ES".into();
        cfg.save_to(&config).unwrap();
        refresh_at(dir.path(), &config, &dir.path().join("profiles.toml"));
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(CACHE_FILE)).unwrap())
                .unwrap();
        assert_eq!(v["lang"], "es");
        let builtins = crate::profiles::builtins();
        assert_eq!(
            v["profiles"].as_array().unwrap().len(),
            builtins.len().min(MAX_MENU_PROFILES)
        );
        assert_eq!(v["profiles"][0]["id"], builtins[0].id.as_str());
    }

    /// GUI and CLI refresh the cache concurrently: every write must leave a
    /// complete, parseable file (never a mix of two writers), and no temp file.
    #[test]
    fn fr_027_concurrent_writers_never_leave_a_mixed_file() {
        let dir = tempfile::tempdir().unwrap();
        let caches: Vec<ShellMenuCache> = (1..=6)
            .map(|n| build(if n % 2 == 0 { "es" } else { "en" }, &profiles(n)))
            .collect();
        std::thread::scope(|scope| {
            for cache in &caches {
                let dir = dir.path();
                scope.spawn(move || {
                    for _ in 0..40 {
                        // On Windows a replace can lose a race to another
                        // writer's rename; the next attempt wins.
                        let _ = write_to(dir, cache);
                    }
                });
            }
        });
        let text = std::fs::read_to_string(dir.path().join(CACHE_FILE)).unwrap();
        let written: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let profile_count = written["profiles"].as_array().unwrap().len();
        assert!(
            caches.iter().any(|c| c.profiles.len() == profile_count),
            "the file is one writer's whole document"
        );
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name() != CACHE_FILE)
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }
}
