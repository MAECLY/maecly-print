//! First-run language from the operating system (FR-093), on the backend.
//!
//! The UI settles the language on its first run from `navigator.language`
//! (`src/i18n/I18nProvider.tsx`). But on Windows the Explorer menu and the
//! built-in profile names are produced before any window opens — the menu
//! cache is written at startup, the CLI lists profiles, the shell extension
//! reads the cache — so a user who installs and right-clicks, or prints from
//! the CLI, would get English everywhere until the window was opened once.
//!
//! [`settle_first_run`] closes that gap: at the start of the app and of the
//! CLI, while `app.locale_detected` is still false, it takes the Windows
//! display language (`GetUserDefaultUILanguage`), narrows it to a supported
//! locale with the same rule as the UI's `matchLocale`, and writes it with
//! `locale_detected = true`. A language the user already picked, or one an
//! upgrade brought over (`util::legacy`), is never changed: both set the flag.
//! Off Windows the OS is not asked ([`os_ui_locale`] is `None`) and the UI
//! keeps doing it as before.

use std::path::Path;

use super::Config;

/// The locales the UI has catalogs for. Mirrors `SUPPORTED_LOCALES` in
/// `src/i18n/index.ts`; the first one is the default (`AppCfg::locale`).
pub const SUPPORTED_LOCALES: [&str; 2] = ["en-US", "es-ES"];

/// Narrow a BCP-47 tag (or a Windows locale name) onto a supported locale:
/// the exact tag, else its language (`es`, `es-MX`, `es_419` → `es-ES`).
/// Mirrors `matchLocale` in `src/i18n/index.ts`. Pure.
pub fn match_locale(tag: &str) -> Option<&'static str> {
    if let Some(exact) = SUPPORTED_LOCALES.iter().find(|l| **l == tag) {
        return Some(exact);
    }
    let language = tag.split(['-', '_']).next()?.to_ascii_lowercase();
    if language.is_empty() {
        return None;
    }
    SUPPORTED_LOCALES
        .iter()
        .find(|l| l.split('-').next() == Some(language.as_str()))
        .copied()
}

/// The Windows display language as a locale name (`es-ES`), or `None` off
/// Windows or when it cannot be read.
pub fn os_ui_locale() -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::{
            GetUserDefaultUILanguage, LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES,
        };
        // LOCALE_NAME_MAX_LENGTH
        let mut buf = [0u16; 85];
        // SAFETY: no preconditions; `buf` is a valid output buffer.
        let len = unsafe {
            let langid = GetUserDefaultUILanguage();
            LCIDToLocaleName(
                u32::from(langid),
                Some(&mut buf),
                LOCALE_ALLOW_NEUTRAL_NAMES,
            )
        };
        let len = usize::try_from(len).ok()?.checked_sub(1)?;
        let name = String::from_utf16(buf.get(..len)?).ok()?;
        (!name.is_empty()).then_some(name)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// What the first-run rule does to `config` given the OS language `os`:
/// `Some(locale)` to write (with the flag), `None` when nothing changes.
/// Pure.
///
/// - already settled (`locale_detected`): nothing;
/// - no OS answer: nothing (the UI still detects on its own);
/// - the config still holds the default and the OS language is another
///   supported one: that locale;
/// - otherwise (the user's choice, or an OS language that is the default or
///   unsupported): the current locale, only to settle the flag.
pub fn first_run_locale(config: &Config, os: Option<&str>) -> Option<String> {
    if config.app.locale_detected {
        return None;
    }
    let os = os?;
    let default = SUPPORTED_LOCALES[0];
    match match_locale(os) {
        Some(found) if config.app.locale == default => Some(found.to_string()),
        _ => Some(config.app.locale.clone()),
    }
}

/// [`first_run_locale`] applied to the config file at `path`: the locale it
/// now holds when it was written, `Ok(None)` when nothing had to change. A
/// config that cannot be parsed is never touched (FR-101).
pub fn settle_first_run_at(path: &Path, os: Option<&str>) -> Result<Option<String>, String> {
    let _guard = super::CONFIG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut config = Config::load_strict(path)?;
    let Some(locale) = first_run_locale(&config, os) else {
        return Ok(None);
    };
    config.app.locale = locale.clone();
    config.app.locale_detected = true;
    config.save_to(path).map_err(|e| e.to_string())?;
    Ok(Some(locale))
}

/// [`settle_first_run_at`] on the real config with the Windows display
/// language. A no-op off Windows. Never fails: problems are logged.
pub fn settle_first_run() {
    let Some(os) = os_ui_locale() else {
        return;
    };
    match settle_first_run_at(&Config::config_path(), Some(&os)) {
        Ok(Some(locale)) => tracing::info!(event = "locale_detected", os = %os, locale = %locale),
        Ok(None) => {}
        Err(e) => tracing::warn!(event = "locale_detect_failed", error = %e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_093_match_locale_mirrors_the_ui() {
        assert_eq!(match_locale("es-ES"), Some("es-ES"));
        assert_eq!(match_locale("es-MX"), Some("es-ES"));
        assert_eq!(match_locale("es_419"), Some("es-ES"));
        assert_eq!(match_locale("ES"), Some("es-ES"));
        assert_eq!(match_locale("en-GB"), Some("en-US"));
        assert_eq!(match_locale("en-US"), Some("en-US"));
        assert_eq!(match_locale("fr-FR"), None);
        assert_eq!(match_locale(""), None);
        // The UI's list and this one are the same.
        let ts = include_str!("../../../src/i18n/index.ts");
        assert!(
            ts.contains("export const SUPPORTED_LOCALES = ['en-US', 'es-ES'] as const;"),
            "src/i18n/index.ts changed its locales: update SUPPORTED_LOCALES here"
        );
    }

    /// FR-093: a Spanish Windows makes a fresh config Spanish before any
    /// window opens; a choice already made is never overridden.
    #[test]
    fn fr_093_first_run_takes_the_windows_display_language() {
        let fresh = Config::default();
        assert_eq!(
            first_run_locale(&fresh, Some("es-ES")).as_deref(),
            Some("es-ES")
        );
        assert_eq!(
            first_run_locale(&fresh, Some("es-MX")).as_deref(),
            Some("es-ES")
        );
        // An OS language without a catalog, or English: settle on English.
        assert_eq!(
            first_run_locale(&fresh, Some("fr-FR")).as_deref(),
            Some("en-US")
        );
        assert_eq!(first_run_locale(&fresh, None), None, "no OS answer");

        let mut chosen = Config::default();
        chosen.app.locale_detected = true;
        assert_eq!(first_run_locale(&chosen, Some("es-ES")), None);

        // English picked before the flag existed, on a Spanish OS: the UI
        // rule keeps a non-default value, and so does this one.
        let mut picked = Config::default();
        picked.app.locale = "es-ES".into();
        assert_eq!(
            first_run_locale(&picked, Some("en-US")).as_deref(),
            Some("es-ES")
        );
    }

    #[test]
    fn fr_093_settle_writes_once_and_never_touches_a_corrupt_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(
            settle_first_run_at(&path, Some("es-ES"))
                .unwrap()
                .as_deref(),
            Some("es-ES")
        );
        let c = Config::load_from(&path);
        assert_eq!(c.app.locale, "es-ES");
        assert!(c.app.locale_detected);
        // Settled: a later OS language change does not move it.
        assert_eq!(settle_first_run_at(&path, Some("en-US")).unwrap(), None);
        assert_eq!(Config::load_from(&path).app.locale, "es-ES");
        // Still a first-run config for the data migration (util::legacy).
        assert!(c.is_first_run_default());

        std::fs::write(&path, "app = [not toml").unwrap();
        assert!(settle_first_run_at(&path, Some("es-ES")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "app = [not toml");
    }
}
