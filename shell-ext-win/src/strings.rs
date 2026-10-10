//! The menu model, and the cache file the app writes for it.
//!
//! Explorer calls `GetTitle` on its UI thread, so the labels shown in the menu
//! cannot be computed by asking the app anything: by the time an answer came
//! back the menu would already have been drawn. Instead the app writes a small
//! already-localized cache (`<data_local_dir>/shell-menu.json`) whenever the
//! profile list or the UI language changes, and this module turns it into the
//! flat list of entries the handler enumerates. A missing, stale, oversized or
//! corrupt cache is not an error — it degrades to the built-in labels in the
//! Windows display language (English or Spanish), which is always a usable
//! menu.
//!
//! Two hard limits from the platform shape everything here:
//!
//! * a handler may contribute at most **16 items** (Explorer silently drops the
//!   rest), enforced by [`MAX_MENU_ITEMS`] and asserted in the tests;
//! * the Windows 11 top-level menu does **not** support a submenu inside a
//!   submenu, so the per-profile entries are flat, not a nested
//!   "Print with profile ▸", as `docs/05-features/context-menu-actions.md`
//!   describes.
//!
//! Portable and unit-testable on any host; nothing here touches Win32.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::handoff::Action;

/// Explorer's per-handler ceiling. Anything beyond this is dropped by the
/// shell, so we cut the list ourselves and keep the tail we choose.
pub const MAX_MENU_ITEMS: usize = 16;

/// How many profiles get their own entry. Chosen so the whole menu fits under
/// [`MAX_MENU_ITEMS`] with room for the fixed entries; the rest are reachable
/// through "Configure print job…".
pub const MAX_MENU_PROFILES: usize = 8;

/// Name of the cache file inside the app's local data directory.
pub const CACHE_FILE: &str = "shell-menu.json";

/// Cache files bigger than this are ignored outright. The real file is a few
/// hundred bytes; a large one means corruption or tampering, and reading it
/// happens while the user is waiting for a menu.
pub const MAX_CACHE_BYTES: u64 = 64 * 1024;

/// Cache schema version. A file with any other version is ignored rather than
/// guessed at, exactly like the handoff file.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// A profile the user can print with straight from the menu.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Profile {
    /// Profile id, written into the handoff's `profile_id` when chosen.
    pub id: String,
    /// Display name substituted into the "Print with …" label.
    pub name: String,
}

/// One entry contributed to the submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    /// A horizontal rule (`ECF_ISSEPARATOR`, empty title).
    Separator,
    /// A clickable entry.
    Command(MenuCommand),
}

/// A clickable entry and the invocation it produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuCommand {
    /// Localized label shown in the menu.
    pub title: String,
    /// What the app is asked to do when this entry is clicked.
    pub action: Action,
    /// Profile to print with, for a per-profile entry; `None` otherwise.
    pub profile_id: Option<String>,
    /// Rendered bold by Explorer (`ECS_DEFAULT` is not a thing for sub-items;
    /// this maps to `ECF_DEFAULT`).
    pub is_default: bool,
}

/// The label set. Every field has a compiled-in English default so a partial
/// cache file is still usable.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Labels {
    /// Title of the root entry in the context menu.
    pub root: String,
    /// Title of the default-profile entry when exactly one item is selected.
    pub print_one: String,
    /// Same, for two or more items. `{count}` is substituted.
    pub print_many: String,
    /// Per-profile entry. `{name}` is substituted.
    pub print_with_profile: String,
    /// Opens the print dialog.
    pub configure: String,
    /// Adds to the queue without printing.
    pub queue: String,
    /// Tooltip on the root entry.
    pub tooltip: String,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            root: "Pluryprint".into(),
            print_one: "Print 1 file".into(),
            print_many: "Print {count} files".into(),
            print_with_profile: "Print with \u{201c}{name}\u{201d}".into(),
            configure: "Configure print job\u{2026}".into(),
            queue: "Add to print queue".into(),
            tooltip: "Batch print the selected files with Pluryprint".into(),
        }
    }
}

impl Labels {
    /// The Spanish labels: `shellMenu` in `src/i18n/locales/es/common.json`,
    /// the catalog the app writes the cache from (a test keeps them equal).
    pub fn spanish() -> Self {
        Self {
            root: "Pluryprint".into(),
            print_one: "Imprimir 1 archivo".into(),
            print_many: "Imprimir {count} archivos".into(),
            print_with_profile: "Imprimir con \u{ab}{name}\u{bb}".into(),
            configure: "Configurar el trabajo de impresi\u{f3}n\u{2026}".into(),
            queue: "A\u{f1}adir a la cola de impresi\u{f3}n".into(),
            tooltip: "Imprime por lotes los archivos seleccionados con Pluryprint".into(),
        }
    }
}

/// The language subtag of the Windows display language (`GetUserDefaultUILanguage`),
/// or `en` where it cannot be asked. Cheap: no I/O, safe on the drawing path.
pub fn os_ui_language() -> &'static str {
    #[cfg(windows)]
    {
        // SAFETY: no arguments, no preconditions.
        let langid = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        language_of_langid(langid)
    }
    #[cfg(not(windows))]
    {
        "en"
    }
}

/// The language subtag of a Windows `LANGID` for the languages the menu has
/// labels in: its primary language (the low 10 bits) `LANG_SPANISH` (0x0a)
/// is `es`, anything else `en`. Pure.
pub fn language_of_langid(langid: u16) -> &'static str {
    match langid & 0x3ff {
        0x0a => "es",
        _ => "en",
    }
}

/// Everything the handler needs to draw its menu.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct Strings {
    /// Cache schema version; anything but [`CACHE_SCHEMA_VERSION`] is rejected
    /// by [`parse`]. Present here so the field is consumed rather than ignored.
    pub schema_version: u32,
    /// BCP-47 tag of the language the labels are in. Diagnostics only.
    pub lang: String,
    /// The localized labels, already in `{count}`/`{name}` placeholder syntax.
    pub labels: Labels,
    /// Profiles offered as "Print with …" entries, in menu order.
    pub profiles: Vec<Profile>,
}

impl Strings {
    /// The built-in English menu with no profiles.
    pub fn defaults() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            lang: "en".into(),
            labels: Labels::default(),
            profiles: Vec::new(),
        }
    }

    /// The built-in menu, with no profiles, in `lang` (a language subtag or a
    /// BCP-47 tag: `es`, `es-ES`); English for any language without labels.
    pub fn defaults_for(lang: &str) -> Self {
        let primary = lang
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match primary.as_str() {
            "es" => Self {
                lang: "es".into(),
                labels: Labels::spanish(),
                ..Self::defaults()
            },
            _ => Self::defaults(),
        }
    }

    /// The built-in menu in the Windows display language: what the menu shows
    /// before the app ever ran (no cache yet), or when the cache is unusable.
    /// The app writes the cache in its own language as soon as it starts.
    pub fn os_defaults() -> Self {
        Self::defaults_for(os_ui_language())
    }

    /// Title of the "print now with the default profile" entry for a selection
    /// of `count` items. Pure string work — no I/O, no allocation beyond the
    /// result — because it runs while the menu is being drawn.
    pub fn print_title(&self, count: usize) -> String {
        if count == 1 {
            self.labels.print_one.clone()
        } else {
            self.labels
                .print_many
                .replace("{count}", &count.to_string())
        }
    }

    /// The full ordered entry list for a selection of `count` items.
    ///
    /// Guaranteed to be at most [`MAX_MENU_ITEMS`] long, and never to start or
    /// end with a separator.
    pub fn menu(&self, count: usize) -> Vec<MenuItem> {
        let mut items = Vec::with_capacity(MAX_MENU_ITEMS);

        items.push(MenuItem::Command(MenuCommand {
            title: self.print_title(count),
            action: Action::PrintDefault,
            profile_id: None,
            is_default: true,
        }));

        let profiles: Vec<&Profile> = self.profiles.iter().take(MAX_MENU_PROFILES).collect();
        if !profiles.is_empty() {
            items.push(MenuItem::Separator);
            for profile in profiles {
                items.push(MenuItem::Command(MenuCommand {
                    title: self
                        .labels
                        .print_with_profile
                        .replace("{name}", &profile.name),
                    action: Action::PrintDefault,
                    profile_id: Some(profile.id.clone()),
                    is_default: false,
                }));
            }
        }

        items.push(MenuItem::Separator);
        items.push(MenuItem::Command(MenuCommand {
            title: self.labels.configure.clone(),
            action: Action::Configure,
            profile_id: None,
            is_default: false,
        }));
        items.push(MenuItem::Command(MenuCommand {
            title: self.labels.queue.clone(),
            action: Action::Queue,
            profile_id: None,
            is_default: false,
        }));

        debug_assert!(items.len() <= MAX_MENU_ITEMS);
        items.truncate(MAX_MENU_ITEMS);
        // Truncation could in principle leave a trailing separator; a rule with
        // nothing under it looks like a bug in the menu.
        while matches!(items.last(), Some(MenuItem::Separator)) {
            items.pop();
        }
        items
    }
}

/// Parse a cache file. A wrong schema version or malformed JSON yields `None`;
/// the caller falls back to [`Strings::defaults`].
pub fn parse(raw: &str) -> Option<Strings> {
    let strings: Strings = serde_json::from_str(raw).ok()?;
    if strings.schema_version != CACHE_SCHEMA_VERSION {
        return None;
    }
    Some(strings)
}

/// Read the cache from `dir`, falling back to the defaults in the Windows
/// display language ([`Strings::os_defaults`]) on any problem.
///
/// The size check happens before the read, so a huge or truncated file costs a
/// `stat` and nothing more. This is the only disk access on the menu-building
/// path and it is deliberately *not* called from `GetTitle`; see
/// `command.rs`.
pub fn load_from_dir(dir: &Path) -> Strings {
    read_capped(&dir.join(CACHE_FILE))
        .ok()
        .as_deref()
        .and_then(parse)
        .unwrap_or_else(Strings::os_defaults)
}

fn read_capped(path: &Path) -> io::Result<String> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_CACHE_BYTES {
        return Err(io::Error::other("menu cache is larger than the limit"));
    }
    std::fs::read_to_string(path)
}

/// Directory the cache lives in: the app's local data dir, resolved exactly as
/// the app resolves it.
pub fn cache_dir() -> io::Result<PathBuf> {
    directories::ProjectDirs::from("com", "Maecly", "Pluryprint")
        .map(|d| d.data_local_dir().to_path_buf())
        .ok_or_else(|| io::Error::other("no home directory to resolve the menu cache in"))
}

/// Read the cache from the real location.
pub fn load() -> Strings {
    match cache_dir() {
        Ok(dir) => load_from_dir(&dir),
        Err(_) => Strings::os_defaults(),
    }
}
