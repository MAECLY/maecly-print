//! Print profiles: the three built-in presets, the on-disk custom profiles
//! (`profiles.toml`), and the pure logic the IPC layer drives (validation,
//! ordering, default resolution).
//!
//! A *profile* is a reusable bundle of print settings the user picks in the
//! print dialog. Three are built in ([`builtins`]) and cannot be edited or
//! deleted; the rest are user-created and persisted in `profiles.toml`. The wire
//! shape ([`Profile`]) carries two server-computed flags — `builtin` and
//! `default` — that are *ignored on input* and recomputed on every read, so a
//! client can never desync them. The on-disk shape ([`StoredProfile`]) drops
//! both flags: "which profile is default" lives in `config.toml`
//! (`engine.default_profile`), and "is built in" is intrinsic to the id.
//!
//! I/O here is path-injected ([`load`]/[`save`]) so integration tests point at a
//! tempdir. Writes are atomic (tempfile + rename) to satisfy FR-026. Reads
//! tolerate a corrupt file (the command layer degrades to builtins-only);
//! *mutations* refuse to touch a corrupt file so user data is never clobbered.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::queue::job::{ColorMode, Orientation, PaperSize, Scaling};
use crate::util::paths;

/// On-disk schema version for `profiles.toml`.
pub const PROFILES_SCHEMA_VERSION: u32 = 1;

/// Stable id of the built-in "Quick" profile — also the fallback default when
/// no valid `engine.default_profile` is configured.
pub const BUILTIN_QUICK: &str = "builtin-quick";
/// Stable id of the built-in "Duplex BW" profile.
pub const BUILTIN_DUPLEX_BW: &str = "builtin-duplex-bw";
/// Stable id of the built-in "Photo Color" profile.
pub const BUILTIN_PHOTO_COLOR: &str = "builtin-photo-color";

/// Serializes every read-modify-write of `profiles.toml` (mirrors
/// [`crate::settings::CONFIG_LOCK`] for the config file). Held only across the
/// synchronous load→mutate→save inside `spawn_blocking`, never across an
/// `.await`.
///
/// **Lock ordering:** when both this and [`crate::settings::CONFIG_LOCK`] are
/// needed ([`crate::ipc::profiles::set_default_profile`]) the order is always
/// PROFILES_LOCK → CONFIG_LOCK; no path takes them in the other order, so the
/// two can't deadlock.
pub(crate) static PROFILES_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Duplex mode as spelled in the profile UI vocabulary (distinct from the
/// engine's [`crate::queue::job::Duplex`], which uses `none`/`long`/`short`).
/// Wire strings: `"off"`, `"long-edge"`, `"short-edge"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileDuplex {
    #[default]
    Off,
    LongEdge,
    ShortEdge,
}

/// Print-quality preset. Wire strings: `"draft"`, `"normal"`, `"high"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Draft,
    #[default]
    Normal,
    High,
}

/// A print profile as seen by the frontend. `builtin` and `default` are
/// **server-computed**: whatever a client sends for them is ignored, and the
/// value returned always reflects the current truth (id is built in; id is the
/// effective default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// True for the three built-in presets. Server-computed; ignored on input.
    pub builtin: bool,
    /// True for the profile that is the current app default. Server-computed;
    /// ignored on input (change it via `set_default_profile`).
    pub default: bool,
    /// Printer id, or the `"<default>"` sentinel to defer to the app/OS default.
    pub printer: String,
    pub color_mode: ColorMode,
    pub duplex: ProfileDuplex,
    pub copies: u16,
    pub paper_size: PaperSize,
    pub orientation: Orientation,
    pub scaling: Scaling,
    /// Page-range expression (engine grammar), or `None` for all pages. Omitted
    /// from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_range: Option<String>,
    pub quality: Quality,
    pub tray: String,
}

/// On-disk profile: [`Profile`] minus the two server-computed flags. This is the
/// `[[profile]]` array-of-tables entry in `profiles.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProfile {
    pub id: String,
    pub name: String,
    pub printer: String,
    pub color_mode: ColorMode,
    pub duplex: ProfileDuplex,
    pub copies: u16,
    pub paper_size: PaperSize,
    pub orientation: Orientation,
    pub scaling: Scaling,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_range: Option<String>,
    pub quality: Quality,
    pub tray: String,
}

/// The whole `profiles.toml`: a schema version plus the custom profiles. The
/// field is named `profile` (singular) so TOML renders it as `[[profile]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilesFile {
    #[serde(default = "default_profiles_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub profile: Vec<StoredProfile>,
}

impl Default for ProfilesFile {
    fn default() -> Self {
        Self {
            schema_version: PROFILES_SCHEMA_VERSION,
            profile: Vec::new(),
        }
    }
}

fn default_profiles_schema_version() -> u32 {
    PROFILES_SCHEMA_VERSION
}

/// A `profiles.toml` that exists but can't be used (unreadable or unparseable).
/// A merely-absent file is **not** an error — [`load`] returns an empty
/// [`ProfilesFile`] for it.
#[derive(Debug, thiserror::Error)]
#[error("profiles.toml could not be read: {0}")]
pub struct LoadError(pub String);

/// Why a *mutation* refused to load `profiles.toml`. Distinct from [`LoadError`]
/// (used by read paths) because a mutation refuses for one more reason than I/O:
/// a file that parses but holds duplicate names or structurally invalid ids must
/// not be rewritten. The read path silently cleans those away (see
/// [`load`]/[`sanitize_for_read`]), so writing the cleaned list back would
/// permanently delete the user's entries — the exact data-loss this guards.
#[derive(Debug, thiserror::Error)]
pub enum MutationLoadError {
    /// The file could not be read or parsed at all.
    #[error("profiles.toml could not be read: {0}")]
    Unreadable(String),
    /// The file parsed but contains duplicate names or invalid stored ids
    /// (empty/whitespace, `builtin-` prefixed, or duplicated).
    #[error("profiles.toml contains duplicate or invalid entries")]
    InvalidEntries,
}

impl StoredProfile {
    /// Promote to the wire [`Profile`], stamping the two computed flags.
    pub fn to_profile(&self, builtin: bool, default: bool) -> Profile {
        Profile {
            id: self.id.clone(),
            name: self.name.clone(),
            builtin,
            default,
            printer: self.printer.clone(),
            color_mode: self.color_mode,
            duplex: self.duplex,
            copies: self.copies,
            paper_size: self.paper_size,
            orientation: self.orientation,
            scaling: self.scaling,
            page_range: self.page_range.clone(),
            quality: self.quality,
            tray: self.tray.clone(),
        }
    }
}

impl Profile {
    /// Drop the computed flags for on-disk storage.
    pub fn to_stored(&self) -> StoredProfile {
        StoredProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            printer: self.printer.clone(),
            color_mode: self.color_mode,
            duplex: self.duplex,
            copies: self.copies,
            paper_size: self.paper_size,
            orientation: self.orientation,
            scaling: self.scaling,
            page_range: self.page_range.clone(),
            quality: self.quality,
            tray: self.tray.clone(),
        }
    }
}

/// The three built-in profiles, in canonical (list) order. Values mirror
/// `src/lib/profiles.ts` (`p1`/`p2`/`p3`) exactly. The `default` flag is a
/// placeholder here (always `false`); the real value is computed per-request
/// from [`effective_default_id`].
pub fn builtins() -> Vec<Profile> {
    vec![
        Profile {
            id: BUILTIN_QUICK.to_string(),
            name: "Quick".to_string(),
            builtin: true,
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
        },
        Profile {
            id: BUILTIN_DUPLEX_BW.to_string(),
            name: "Duplex BW".to_string(),
            builtin: true,
            default: false,
            printer: "<default>".to_string(),
            color_mode: ColorMode::Monochrome,
            duplex: ProfileDuplex::LongEdge,
            copies: 1,
            paper_size: PaperSize::A4,
            orientation: Orientation::Auto,
            scaling: Scaling::Fit,
            page_range: None,
            quality: Quality::Normal,
            tray: "auto".to_string(),
        },
        Profile {
            id: BUILTIN_PHOTO_COLOR.to_string(),
            name: "Photo Color".to_string(),
            builtin: true,
            default: false,
            printer: "<default>".to_string(),
            color_mode: ColorMode::Color,
            duplex: ProfileDuplex::Off,
            copies: 1,
            paper_size: PaperSize::A4,
            orientation: Orientation::Auto,
            scaling: Scaling::None,
            page_range: None,
            quality: Quality::High,
            tray: "auto".to_string(),
        },
    ]
}

/// True iff `id` names a built-in profile.
pub fn is_builtin_id(id: &str) -> bool {
    builtins().iter().any(|b| b.id == id)
}

/// True iff `id` names a known profile — built-in or one of `customs`.
pub fn is_known_id(id: &str, customs: &[StoredProfile]) -> bool {
    is_builtin_id(id) || customs.iter().any(|c| c.id == id)
}

/// Resolve the effective default profile id: the configured id when it still
/// names a known profile (built-in or custom), otherwise the built-in "Quick".
/// A configured id that was deleted out from under the config falls back rather
/// than dangling.
pub fn effective_default_id(cfg_default: Option<&str>, customs: &[StoredProfile]) -> String {
    match cfg_default {
        Some(id) if is_known_id(id, customs) => id.to_string(),
        _ => BUILTIN_QUICK.to_string(),
    }
}

/// The full profile list for `list_profiles`: built-ins in canonical order,
/// then customs sorted case-insensitively by name. Each entry's `default` flag
/// is stamped from `default_id`.
pub fn list_all(customs: &[StoredProfile], default_id: &str) -> Vec<Profile> {
    let mut out: Vec<Profile> = builtins()
        .into_iter()
        .map(|mut b| {
            b.default = b.id == default_id;
            b
        })
        .collect();

    let mut sorted: Vec<&StoredProfile> = customs.iter().collect();
    sorted.sort_by_key(|c| c.name.to_lowercase());
    out.extend(
        sorted
            .into_iter()
            .map(|c| c.to_profile(false, c.id == default_id)),
    );
    out
}

/// Validate an incoming profile against the existing customs and return a
/// normalized copy (trimmed name, empty printer/tray filled with sentinels,
/// blank page-range collapsed to `None`). `self_id` is the id being updated, so
/// its own name is excluded from the uniqueness check (a rename keeps working);
/// pass `None` when creating.
///
/// Errors (all → `InvalidRequest`): empty/over-long name, a name that collides
/// case-insensitively with a built-in or another custom, out-of-range copies, or
/// a syntactically invalid page range. The `id` field is not validated here —
/// the command layer owns id semantics (builtin-rejection, length, generation).
pub fn validate_and_normalize(
    mut p: Profile,
    existing: &[StoredProfile],
    self_id: Option<&str>,
) -> Result<Profile, String> {
    let name = p.name.trim();
    if name.is_empty() {
        return Err("profile name must not be empty".to_string());
    }
    if name.chars().count() > 100 {
        return Err("profile name must be 100 characters or fewer".to_string());
    }
    let name_lc = name.to_lowercase();
    let clashes_builtin = builtins().iter().any(|b| b.name.to_lowercase() == name_lc);
    let clashes_custom = existing
        .iter()
        .any(|e| Some(e.id.as_str()) != self_id && e.name.trim().to_lowercase() == name_lc);
    if clashes_builtin || clashes_custom {
        return Err(format!("a profile named \"{name}\" already exists"));
    }
    p.name = name.to_string();

    if !(1..=999).contains(&p.copies) {
        return Err(format!(
            "copies must be between 1 and 999, got {}",
            p.copies
        ));
    }

    // A blank/whitespace range means "all pages" (None); a non-blank one must
    // parse against the engine grammar.
    p.page_range = match p.page_range.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            crate::engine::pagerange::parse(s)?;
            Some(s.trim().to_string())
        }
        _ => None,
    };

    if p.printer.trim().is_empty() {
        p.printer = "<default>".to_string();
    }
    if p.tray.trim().is_empty() {
        p.tray = "auto".to_string();
    }

    Ok(p)
}

/// Why a [`upsert`] was rejected. The command layer maps each to an
/// [`crate::util::error::IpcError`].
#[derive(Debug, PartialEq, Eq)]
pub enum UpsertError {
    /// The id names a built-in profile, which cannot be modified.
    Builtin,
    /// A provided (non-empty) id is longer than 100 characters.
    IdTooLong,
    /// Validation failed; the message is user-facing.
    Invalid(String),
}

/// Create or update a custom profile in `file` (upsert), returning the
/// normalized profile with its final id. The two computed flags are **not**
/// stamped here (the caller knows the effective default). Pure — no I/O — so the
/// command (locking + persistence) and the integration tests share this logic.
///
/// Id semantics: empty → create with a fresh UUIDv7; built-in id → [`UpsertError::Builtin`];
/// known custom id → update; unknown non-empty id → create preserving that id.
pub fn upsert(file: &mut ProfilesFile, profile: Profile) -> Result<Profile, UpsertError> {
    let requested_id = profile.id.trim().to_string();
    if !requested_id.is_empty() {
        if is_builtin_id(&requested_id) {
            return Err(UpsertError::Builtin);
        }
        if requested_id.chars().count() > 100 {
            return Err(UpsertError::IdTooLong);
        }
    }

    let self_id = (!requested_id.is_empty()).then_some(requested_id.as_str());
    let mut normalized =
        validate_and_normalize(profile, &file.profile, self_id).map_err(UpsertError::Invalid)?;

    // The two computed flags are server-owned and ignored on input: a saved
    // custom is never built-in, and `default` is stamped by the caller (which
    // knows the effective default).
    normalized.builtin = false;
    normalized.default = false;

    let final_id = if requested_id.is_empty() {
        uuid::Uuid::now_v7().to_string()
    } else {
        requested_id
    };
    normalized.id = final_id.clone();

    let stored = normalized.to_stored();
    match file.profile.iter_mut().find(|e| e.id == final_id) {
        Some(existing) => *existing = stored,
        None => file.profile.push(stored),
    }
    Ok(normalized)
}

/// Why a [`delete`] was rejected.
#[derive(Debug, PartialEq, Eq)]
pub enum DeleteError {
    /// The id names a built-in profile, which cannot be deleted.
    Builtin,
    /// The id is the current effective default; switch the default first.
    IsDefault,
    /// No custom profile with that id exists.
    NotFound,
}

/// Remove a custom profile from `file`. `default_id` is the current effective
/// default, whose deletion is refused. Pure — no I/O.
pub fn delete(file: &mut ProfilesFile, id: &str, default_id: &str) -> Result<(), DeleteError> {
    if is_builtin_id(id) {
        return Err(DeleteError::Builtin);
    }
    if !file.profile.iter().any(|e| e.id == id) {
        return Err(DeleteError::NotFound);
    }
    if default_id == id {
        return Err(DeleteError::IsDefault);
    }
    file.profile.retain(|e| e.id != id);
    Ok(())
}

/// Absolute path of `profiles.toml` in the config dir.
pub fn profiles_path() -> PathBuf {
    paths::config_dir().join("profiles.toml")
}

/// Load `profiles.toml` from `path` for a **read**.
///
/// - Missing file → empty [`ProfilesFile`] (not an error).
/// - Unreadable / unparseable → [`LoadError`].
/// - Otherwise the parsed list is cleaned by [`sanitize_for_read`]: entries the
///   UI could never edit or delete (blank/`builtin-`/duplicate id, duplicate
///   name) are dropped with a warning, so a read never surfaces a ghost row.
///
/// This is the loader for read-only commands and existence checks. A **mutation**
/// must instead use [`load_for_mutation`], which *refuses* such a file rather
/// than cleaning it — otherwise the very next save would persist the truncated
/// list and silently delete the user's data.
pub fn load(path: &Path) -> Result<ProfilesFile, LoadError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProfilesFile::default());
        }
        Err(e) => return Err(LoadError(e.to_string())),
    };
    let mut file: ProfilesFile = toml::from_str(&raw).map_err(|e| LoadError(e.to_string()))?;
    sanitize_for_read(&mut file.profile);
    Ok(file)
}

/// Load `profiles.toml` from `path` for a **mutation** (save/delete/set-default
/// of a custom id).
///
/// - Missing file → empty [`ProfilesFile`] (not an error).
/// - Unreadable / unparseable → [`MutationLoadError::Unreadable`].
/// - Parsed but holding duplicate names or invalid stored ids →
///   [`MutationLoadError::InvalidEntries`].
///
/// Unlike [`load`] it never silently cleans the list: a mutation writes the
/// whole file back, so cleaning here would delete exactly the entries the read
/// path merely hid. Refusing keeps the on-disk data intact until the user fixes
/// (or removes) the file.
pub fn load_for_mutation(path: &Path) -> Result<ProfilesFile, MutationLoadError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProfilesFile::default());
        }
        Err(e) => return Err(MutationLoadError::Unreadable(e.to_string())),
    };
    let file: ProfilesFile =
        toml::from_str(&raw).map_err(|e| MutationLoadError::Unreadable(e.to_string()))?;
    if has_duplicate_or_invalid_entries(&file.profile) {
        return Err(MutationLoadError::InvalidEntries);
    }
    Ok(file)
}

/// Atomically write `file` to `path` (tempfile in the same dir, then rename), so
/// a crash mid-write never leaves a partial `profiles.toml` (FR-026).
pub fn save(path: &Path, file: &ProfilesFile) -> std::io::Result<()> {
    let parent = path.parent().expect("profiles path has a parent directory");
    paths::ensure_dir(parent)?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    let toml_bytes = toml::to_string_pretty(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    tmp.write_all(toml_bytes.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path)
        .map_err(|e| std::io::Error::other(format!("persist failed: {e}")))?;
    Ok(())
}

/// True iff `id` is one a mutation must reject: a custom profile can never own a
/// blank/whitespace id (unaddressable) nor a `builtin-` prefixed id (reserved
/// for the built-in presets — such a row can never be edited or deleted).
fn is_invalid_stored_id(id: &str) -> bool {
    let id = id.trim();
    id.is_empty() || id.starts_with("builtin-")
}

/// Clean a freshly-parsed custom list for a **read**: drop every entry the UI
/// could never edit or delete — a blank/`builtin-` prefixed id, a
/// duplicate id, or a duplicate (case-insensitive) name — keeping the first of
/// any clash and logging each drop. Read paths must never surface such a ghost
/// row; mutation paths refuse the whole file instead (see [`load_for_mutation`]).
fn sanitize_for_read(profiles: &mut Vec<StoredProfile>) {
    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_names = std::collections::HashSet::new();
    profiles.retain(|p| {
        if is_invalid_stored_id(&p.id) {
            tracing::warn!(
                event = "profiles_invalid_id_skipped",
                id = %p.id,
                name = %p.name,
                "skipping profile with an unusable id from profiles.toml"
            );
            return false;
        }
        if !seen_ids.insert(p.id.trim().to_string()) {
            tracing::warn!(
                event = "profiles_duplicate_id_skipped",
                id = %p.id,
                "skipping duplicate profile id from profiles.toml"
            );
            return false;
        }
        if !seen_names.insert(p.name.trim().to_lowercase()) {
            tracing::warn!(
                event = "profiles_duplicate_name_dropped",
                name = %p.name,
                "dropping duplicate profile name from profiles.toml"
            );
            return false;
        }
        true
    });
}

/// True iff `profiles` holds any entry a mutation must refuse to rewrite: a
/// blank/`builtin-` prefixed id, a duplicate id, or a duplicate
/// (case-insensitive) name. This is the mirror of [`sanitize_for_read`]: exactly
/// the entries that function would silently drop are the ones that make a
/// mutation unsafe.
fn has_duplicate_or_invalid_entries(profiles: &[StoredProfile]) -> bool {
    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_names = std::collections::HashSet::new();
    for p in profiles {
        if is_invalid_stored_id(&p.id) {
            return true;
        }
        if !seen_ids.insert(p.id.trim().to_string()) {
            return true;
        }
        if !seen_names.insert(p.name.trim().to_lowercase()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_stored(id: &str, name: &str) -> StoredProfile {
        StoredProfile {
            id: id.to_string(),
            name: name.to_string(),
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
        }
    }

    fn sample_profile(name: &str) -> Profile {
        sample_stored("", name).to_profile(false, false)
    }

    #[test]
    fn builtins_mirror_frontend_presets() {
        let b = builtins();
        assert_eq!(b.len(), 3);

        assert_eq!(b[0].id, "builtin-quick");
        assert_eq!(b[0].name, "Quick");
        assert!(b[0].builtin);
        assert_eq!(b[0].printer, "<default>");
        assert_eq!(b[0].color_mode, ColorMode::Color);
        assert_eq!(b[0].duplex, ProfileDuplex::Off);
        assert_eq!(b[0].scaling, Scaling::Fit);
        assert_eq!(b[0].quality, Quality::Normal);
        assert_eq!(b[0].tray, "auto");

        assert_eq!(b[1].id, "builtin-duplex-bw");
        assert_eq!(b[1].name, "Duplex BW");
        assert_eq!(b[1].color_mode, ColorMode::Monochrome);
        assert_eq!(b[1].duplex, ProfileDuplex::LongEdge);
        assert_eq!(b[1].scaling, Scaling::Fit);
        assert_eq!(b[1].quality, Quality::Normal);

        assert_eq!(b[2].id, "builtin-photo-color");
        assert_eq!(b[2].name, "Photo Color");
        assert_eq!(b[2].color_mode, ColorMode::Color);
        assert_eq!(b[2].duplex, ProfileDuplex::Off);
        assert_eq!(b[2].scaling, Scaling::None);
        assert_eq!(b[2].quality, Quality::High);

        // All built-ins share the default settings the frontend seeds.
        for p in &b {
            assert_eq!(p.copies, 1);
            assert_eq!(p.paper_size, PaperSize::A4);
            assert_eq!(p.orientation, Orientation::Auto);
            assert!(p.page_range.is_none());
        }
    }

    #[test]
    fn duplex_and_quality_wire_strings() {
        assert_eq!(serde_json::to_value(ProfileDuplex::Off).unwrap(), "off");
        assert_eq!(
            serde_json::to_value(ProfileDuplex::LongEdge).unwrap(),
            "long-edge"
        );
        assert_eq!(
            serde_json::to_value(ProfileDuplex::ShortEdge).unwrap(),
            "short-edge"
        );
        assert_eq!(serde_json::to_value(Quality::Draft).unwrap(), "draft");
        assert_eq!(serde_json::to_value(Quality::High).unwrap(), "high");
    }

    #[test]
    fn stored_profile_toml_round_trip() {
        let mut sp = sample_stored("c-1", "Press A");
        sp.duplex = ProfileDuplex::LongEdge;
        sp.printer = "<default>".to_string();
        let file = ProfilesFile {
            schema_version: 1,
            profile: vec![sp.clone()],
        };
        let toml = toml::to_string_pretty(&file).unwrap();
        assert!(
            toml.contains("long-edge"),
            "kebab duplex on the wire: {toml}"
        );
        assert!(
            toml.contains("<default>"),
            "printer sentinel verbatim: {toml}"
        );
        assert!(
            !toml.contains("page_range"),
            "None page_range must be omitted: {toml}"
        );
        let back: ProfilesFile = toml::from_str(&toml).unwrap();
        assert_eq!(back.profile, vec![sp]);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        let f = load(&path).expect("missing file is not an error");
        assert!(f.profile.is_empty());
        assert_eq!(f.schema_version, PROFILES_SCHEMA_VERSION);
    }

    #[test]
    fn load_corrupt_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        std::fs::write(&path, b"this is = not valid = toml ][").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn load_drops_duplicate_names_keeping_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        let mut first = sample_stored("c-1", "Press A");
        first.copies = 3;
        let mut dup = sample_stored("c-2", "press a"); // case-insensitive clash
        dup.copies = 7;
        save(
            &path,
            &ProfilesFile {
                schema_version: 1,
                profile: vec![first, dup],
            },
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.profile.len(), 1, "duplicate name dropped");
        assert_eq!(loaded.profile[0].id, "c-1", "first occurrence kept");
        assert_eq!(loaded.profile[0].copies, 3);
    }

    #[test]
    fn load_read_skips_invalid_id_entries_keeping_valid_customs() {
        // A stored entry with a `builtin-` prefixed id or an empty id can
        // never be edited/deleted through the UI, so the READ path drops it (with
        // a warning) rather than surfacing a ghost row. Valid customs survive.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        save(
            &path,
            &ProfilesFile {
                schema_version: 1,
                profile: vec![
                    sample_stored("builtin-quick", "Ghost Builtin"),
                    sample_stored("", "Ghost Empty"),
                    sample_stored("   ", "Ghost Whitespace"),
                    sample_stored("c-1", "Real Custom"),
                ],
            },
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.profile.len(), 1, "only the valid custom remains");
        assert_eq!(loaded.profile[0].id, "c-1");
        assert_eq!(loaded.profile[0].name, "Real Custom");
    }

    #[test]
    fn load_read_skips_duplicate_ids_keeping_first() {
        // Two entries sharing an id — the second is unaddressable, so the
        // read path keeps the first and drops the rest.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        let mut first = sample_stored("dup", "First");
        first.copies = 2;
        let mut second = sample_stored("dup", "Second");
        second.copies = 9;
        save(
            &path,
            &ProfilesFile {
                schema_version: 1,
                profile: vec![first, second],
            },
        )
        .unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.profile.len(), 1);
        assert_eq!(loaded.profile[0].name, "First");
        assert_eq!(loaded.profile[0].copies, 2);
    }

    #[test]
    fn load_for_mutation_refuses_duplicate_names() {
        // The mutation loader must NOT clean the file (that would delete a
        // duplicate-named profile on the next save); it refuses instead.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        let before = ProfilesFile {
            schema_version: 1,
            profile: vec![
                sample_stored("c-1", "Press A"),
                sample_stored("c-2", "press a"), // case-insensitive clash
            ],
        };
        save(&path, &before).unwrap();
        let bytes = std::fs::read(&path).unwrap();

        assert!(matches!(
            load_for_mutation(&path),
            Err(MutationLoadError::InvalidEntries)
        ));
        // Nothing was rewritten.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn load_for_mutation_refuses_invalid_ids_but_accepts_clean_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");

        // A `builtin-` prefixed id → refused.
        save(
            &path,
            &ProfilesFile {
                schema_version: 1,
                profile: vec![sample_stored("builtin-hax", "Sneaky")],
            },
        )
        .unwrap();
        assert!(matches!(
            load_for_mutation(&path),
            Err(MutationLoadError::InvalidEntries)
        ));

        // A clean file loads fine.
        save(
            &path,
            &ProfilesFile {
                schema_version: 1,
                profile: vec![sample_stored("c-1", "Fine")],
            },
        )
        .unwrap();
        let ok = load_for_mutation(&path).unwrap();
        assert_eq!(ok.profile.len(), 1);
        assert_eq!(ok.profile[0].id, "c-1");
    }

    #[test]
    fn load_for_mutation_missing_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.toml");
        // Missing → empty, not an error.
        assert!(load_for_mutation(&path).unwrap().profile.is_empty());
        // Corrupt → Unreadable.
        std::fs::write(&path, b"not = valid = toml ][").unwrap();
        assert!(matches!(
            load_for_mutation(&path),
            Err(MutationLoadError::Unreadable(_))
        ));
    }

    #[test]
    fn validate_rejects_empty_and_overlong_names() {
        let mut p = sample_profile("   ");
        assert!(validate_and_normalize(p.clone(), &[], None).is_err());
        p.name = "x".repeat(101);
        assert!(validate_and_normalize(p, &[], None).is_err());
    }

    #[test]
    fn validate_rejects_name_clash_with_builtin_case_insensitive() {
        let p = sample_profile("quICK"); // clashes with built-in "Quick"
        assert!(validate_and_normalize(p, &[], None).is_err());
    }

    #[test]
    fn validate_rejects_name_clash_with_other_custom_but_allows_self() {
        let existing = vec![
            sample_stored("c-1", "Press A"),
            sample_stored("c-2", "Labels"),
        ];
        // New profile clashing with an existing custom → rejected.
        let clash = sample_profile("press a");
        assert!(validate_and_normalize(clash, &existing, None).is_err());
        // Updating c-1 keeping its own name → allowed (self excluded).
        let mut same = sample_profile("Press A");
        same.id = "c-1".to_string();
        assert!(validate_and_normalize(same, &existing, Some("c-1")).is_ok());
    }

    #[test]
    fn validate_copies_and_range_bounds() {
        let mut p = sample_profile("Range Test");
        p.copies = 0;
        assert!(validate_and_normalize(p.clone(), &[], None).is_err());
        p.copies = 1000;
        assert!(validate_and_normalize(p.clone(), &[], None).is_err());
        p.copies = 1;
        p.page_range = Some("5-1".to_string()); // descending → invalid
        assert!(validate_and_normalize(p, &[], None).is_err());
    }

    #[test]
    fn validate_normalizes_blanks_to_sentinels() {
        let mut p = sample_profile("Normalize");
        p.printer = "   ".to_string();
        p.tray = "".to_string();
        p.page_range = Some("   ".to_string());
        let out = validate_and_normalize(p, &[], None).unwrap();
        assert_eq!(out.printer, "<default>");
        assert_eq!(out.tray, "auto");
        assert!(out.page_range.is_none(), "blank range collapses to None");
    }

    #[test]
    fn effective_default_falls_back_to_quick() {
        let customs = vec![sample_stored("c-1", "Press A")];
        // Configured custom that exists → itself.
        assert_eq!(effective_default_id(Some("c-1"), &customs), "c-1");
        // Configured built-in → itself.
        assert_eq!(
            effective_default_id(Some(BUILTIN_PHOTO_COLOR), &customs),
            BUILTIN_PHOTO_COLOR
        );
        // Unknown / missing → Quick.
        assert_eq!(effective_default_id(Some("ghost"), &customs), BUILTIN_QUICK);
        assert_eq!(effective_default_id(None, &customs), BUILTIN_QUICK);
    }

    #[test]
    fn upsert_empty_id_creates_with_generated_id() {
        let mut file = ProfilesFile::default();
        let saved = upsert(&mut file, sample_profile("Press A")).unwrap();
        assert!(!saved.id.is_empty(), "generated a fresh id");
        assert_ne!(saved.id, "");
        assert_eq!(file.profile.len(), 1);
        assert_eq!(file.profile[0].id, saved.id);
    }

    #[test]
    fn upsert_unknown_id_is_preserved_on_create() {
        // The delete-then-undo flow re-saves with the same (now-unknown) id.
        let mut file = ProfilesFile::default();
        let mut p = sample_profile("Restored");
        p.id = "prev-id-123".to_string();
        let saved = upsert(&mut file, p).unwrap();
        assert_eq!(saved.id, "prev-id-123", "unknown id preserved");
        assert_eq!(file.profile.len(), 1);
    }

    #[test]
    fn upsert_known_id_updates_in_place() {
        let mut file = ProfilesFile {
            schema_version: 1,
            profile: vec![sample_stored("c-1", "Press A")],
        };
        let mut p = sample_profile("Press A Renamed");
        p.id = "c-1".to_string();
        p.copies = 5;
        let saved = upsert(&mut file, p).unwrap();
        assert_eq!(saved.id, "c-1");
        assert_eq!(file.profile.len(), 1, "updated, not appended");
        assert_eq!(file.profile[0].name, "Press A Renamed");
        assert_eq!(file.profile[0].copies, 5);
    }

    #[test]
    fn upsert_rejects_builtin_id() {
        let mut file = ProfilesFile::default();
        let mut p = sample_profile("Hijack");
        p.id = BUILTIN_QUICK.to_string();
        assert_eq!(upsert(&mut file, p), Err(UpsertError::Builtin));
        assert!(file.profile.is_empty(), "nothing written");
    }

    #[test]
    fn delete_rejects_builtin_default_and_unknown() {
        let mut file = ProfilesFile {
            schema_version: 1,
            profile: vec![
                sample_stored("c-1", "Press A"),
                sample_stored("c-2", "Labels"),
            ],
        };
        assert_eq!(
            delete(&mut file, BUILTIN_QUICK, BUILTIN_QUICK),
            Err(DeleteError::Builtin)
        );
        assert_eq!(
            delete(&mut file, "ghost", "c-1"),
            Err(DeleteError::NotFound)
        );
        assert_eq!(delete(&mut file, "c-1", "c-1"), Err(DeleteError::IsDefault));
        // A non-default custom deletes cleanly.
        assert!(delete(&mut file, "c-2", "c-1").is_ok());
        assert_eq!(file.profile.len(), 1);
        assert_eq!(file.profile[0].id, "c-1");
    }

    #[test]
    fn list_all_orders_builtins_then_sorted_customs() {
        let customs = vec![
            sample_stored("c-1", "Zebra"),
            sample_stored("c-2", "apple"),
            sample_stored("c-3", "Mango"),
        ];
        let list = list_all(&customs, BUILTIN_QUICK);
        let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Quick",
                "Duplex BW",
                "Photo Color",
                "apple",
                "Mango",
                "Zebra"
            ]
        );
        // The default flag is stamped on exactly the effective default.
        assert!(list[0].default, "Quick is the default here");
        assert_eq!(list.iter().filter(|p| p.default).count(), 1);
    }
}
