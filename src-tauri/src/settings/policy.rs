//! Organization printer policy.
//!
//! An admin-managed setting that lets an organization pin Pluryprint
//! behavior. Today it carries a single field — the enforced default printer —
//! which the UI presents as a locked "managed by your organization" state.
//!
//! Because the UI calls it *managed by your organization*, a policy is only
//! believed when a non-administrator could not have written it:
//!
//! | Platform | Source, most authoritative first |
//! | -------- | -------------------------------- |
//! | Windows  | `HKLM\SOFTWARE\Policies\Maecly\Pluryprint` value `DefaultPrinter` (REG_SZ; Group Policy friendly, admin-only), then `C:\ProgramData\Pluryprint\policy.json` **only if the file is owned by Administrators or SYSTEM** |
//! | macOS    | `/Library/Application Support/Pluryprint/policy.json` (admin-writable) |
//! | Linux    | `/etc/pluryprint/policy.json` (root-writable) |
//!
//! **Legacy locations.** The app was called MaeclyPrint before. A policy an
//! admin deployed under the old name (`HKLM\SOFTWARE\Policies\Maecly\MaeclyPrint`,
//! `C:\ProgramData\MaeclyPrint\policy.json`,
//! `/Library/Application Support/MaeclyPrint/policy.json`,
//! `/etc/maeclyprint/policy.json`) is still honoured, with the same trust rules,
//! when nothing is found at any new location, and a warning asks the admin
//! to move it. Every new location, registry or file, beats every legacy one:
//! an admin who moved to the new file but left the old Group Policy in place
//! gets the new policy. The app runs as the user, so it cannot move a machine policy
//! itself. See ADR-0009.
//!
//! `C:\ProgramData` lets any standard user create folders (and own them), so a
//! `policy.json` there that a user created is ignored — otherwise any user
//! could impose a "managed" default printer on everyone else on the machine.
//! The folder holding it must be admin/SYSTEM-owned too: a user who created
//! `C:\ProgramData\Pluryprint` first could otherwise swap files inside it.
//!
//! The file is opened **once**, its owner is read from that open handle, and
//! the content is read from the same handle ([`read_trusted`]). Checking the
//! path and reading the path separately let a user who controls the folder
//! show user-written content during the read and put the admin's file back
//! before the owner check.
//!
//! **Debug builds only**, the per-user config dir (`<config_dir>/policy.json`)
//! is consulted last as a developer/testing fallback; a release build never
//! reads it, so a user cannot fake a managed state for themselves either.
//!
//! A missing source is not an error — policy is optional and absence yields
//! [`PrinterPolicy::default`]. Malformed JSON is logged and also degrades to
//! the default rather than failing app startup.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Organization-managed printer policy. Every field is optional so an empty or
/// partial `policy.json` still deserializes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct PrinterPolicy {
    /// Printer id the organization pins as the default. When set, the UI treats
    /// the default as read-only (managed by policy). `None` means unmanaged.
    #[serde(default)]
    pub default_printer: Option<String>,
}

impl PrinterPolicy {
    /// Load the effective policy from the first [`sources`] entry that has
    /// one. Untrusted, missing or unreadable files are skipped; a malformed
    /// trusted file is logged and treated as no-policy.
    pub fn load() -> Self {
        resolve(&sources(), read_source)
    }
}

/// One place a policy can come from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// `HKLM\SOFTWARE\Policies\Maecly\<app>` → `DefaultPrinter` (Windows).
    Registry { legacy: bool },
    /// A `policy.json`.
    File(PathBuf),
}

impl Source {
    fn is_legacy(&self) -> bool {
        match self {
            Source::Registry { legacy } => *legacy,
            Source::File(path) => is_legacy(path),
        }
    }
}

/// What one source holds.
#[derive(Debug)]
enum Found {
    /// No policy here (absent, unreadable, untrusted): try the next source.
    Nothing,
    Policy(PrinterPolicy),
    /// A trusted file that does not parse: no policy, and the search stops.
    Malformed,
}

/// Every place a policy can come from, most authoritative first. Every
/// current location — the registry, then the file — comes before every
/// legacy one, so an admin who moved to the new location but left the old
/// deployment in place gets the new policy.
fn sources() -> Vec<Source> {
    let mut out = Vec::new();
    if cfg!(windows) {
        out.push(Source::Registry { legacy: false });
    }
    if let Some(system) = system_policy_path(APP_DIR) {
        out.push(Source::File(system));
    }
    if cfg!(windows) {
        out.push(Source::Registry { legacy: true });
    }
    if let Some(legacy) = system_policy_path(LEGACY_APP_DIR) {
        out.push(Source::File(legacy));
    }
    // Developer/testing fallback: never in a release build.
    #[cfg(debug_assertions)]
    out.push(Source::File(dev_policy_path()));
    out
}

/// The per-user developer fallback (debug builds only).
#[cfg(debug_assertions)]
fn dev_policy_path() -> PathBuf {
    crate::util::paths::config_dir().join("policy.json")
}

/// Whether `source` is the debug-only per-user fallback.
#[cfg(test)]
fn is_dev_fallback(source: &Source) -> bool {
    *source == Source::File(crate::util::paths::config_dir().join("policy.json"))
}

/// The policy from the first source that has one. Pure over `read`, for tests.
fn resolve(sources: &[Source], mut read: impl FnMut(&Source) -> Found) -> PrinterPolicy {
    for source in sources {
        match read(source) {
            Found::Nothing => continue,
            Found::Policy(policy) => return policy,
            Found::Malformed => return PrinterPolicy::default(),
        }
    }
    PrinterPolicy::default()
}

/// Read one source, logging what an admin should hear about.
fn read_source(source: &Source) -> Found {
    let found = match source {
        Source::Registry { legacy } => match registry_policy(*legacy) {
            Some(policy) => Found::Policy(policy),
            None => return Found::Nothing,
        },
        Source::File(path) => {
            let raw = match read_trusted(path) {
                // Missing or unreadable — try the next candidate.
                PolicyFile::Missing => return Found::Nothing,
                PolicyFile::Untrusted => {
                    tracing::warn!(
                        event = "policy_untrusted",
                        path = %path.display(),
                        "ignoring policy.json that a non-administrator could have written"
                    );
                    return Found::Nothing;
                }
                PolicyFile::Trusted(raw) => raw,
            };
            match parse_policy(&raw) {
                Some(policy) => Found::Policy(policy),
                None => {
                    tracing::warn!(
                        event = "policy_parse_failed",
                        path = %path.display(),
                        "ignoring malformed policy.json"
                    );
                    return Found::Malformed;
                }
            }
        }
    };
    if source.is_legacy() {
        tracing::warn!(
            event = "policy_legacy_location",
            source = ?source,
            "using a policy deployed for MaeclyPrint; move it to the Pluryprint location"
        );
    }
    found
}

/// Folder name of the machine policy under the current product name.
const APP_DIR: &str = "Pluryprint";
/// Folder name a policy deployed for MaeclyPrint (the old name) lives under.
const LEGACY_APP_DIR: &str = "MaeclyPrint";

/// Whether `path` is one of the legacy (pre-rename) policy locations.
fn is_legacy(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(LEGACY_APP_DIR))
    })
}

/// The machine-wide policy path for this platform under the folder name
/// `app` (`Pluryprint`, or the legacy `MaeclyPrint`), if one is defined.
fn system_policy_path(app: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("ProgramData").map(|pd| PathBuf::from(pd).join(app).join("policy.json"))
    }
    #[cfg(target_os = "macos")]
    {
        Some(
            PathBuf::from("/Library/Application Support")
                .join(app)
                .join("policy.json"),
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Some(
            PathBuf::from("/etc")
                .join(app.to_ascii_lowercase())
                .join("policy.json"),
        )
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = app;
        None
    }
}

/// Parse policy JSON, returning `None` on malformed input (so the caller can
/// log and fall back). Pure, for unit tests.
fn parse_policy(raw: &str) -> Option<PrinterPolicy> {
    serde_json::from_str(raw).ok()
}

/// A policy from the registry value, if it is set and not blank. Pure, for
/// unit tests.
fn policy_from_registry_value(value: Option<String>) -> Option<PrinterPolicy> {
    let value = value?.trim().to_string();
    (!value.is_empty()).then_some(PrinterPolicy {
        default_printer: Some(value),
    })
}

/// `HKLM\SOFTWARE\Policies\Maecly\Pluryprint` → `DefaultPrinter`, or the
/// same value under the legacy `...\Maecly\MaeclyPrint` key. Only
/// administrators (and Group Policy) can write under `HKLM\SOFTWARE\Policies`.
fn registry_policy(legacy: bool) -> Option<PrinterPolicy> {
    #[cfg(windows)]
    {
        let key = if legacy {
            win::PolicyKey::Legacy
        } else {
            win::PolicyKey::Current
        };
        policy_from_registry_value(win::read_policy_string(key, "DefaultPrinter"))
    }
    #[cfg(not(windows))]
    {
        let _ = legacy;
        policy_from_registry_value(None)
    }
}

/// What reading one candidate policy file found.
#[derive(Debug, PartialEq, Eq)]
enum PolicyFile {
    /// Absent or unreadable: not a policy.
    Missing,
    /// Present, but a standard user could have written it (or its folder).
    Untrusted,
    /// The content of a file only an administrator could have written.
    Trusted(String),
}

/// Open `path` once, decide trust from the **open handle** (and the folder
/// that holds it), and read the content from that same handle, so the file
/// that was checked is the file that was read.
fn read_trusted(path: &std::path::Path) -> PolicyFile {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(path) else {
        return PolicyFile::Missing;
    };
    if !handle_is_admin_owned(&file) || !folder_is_admin_owned(path) {
        return PolicyFile::Untrusted;
    }
    let mut raw = String::new();
    match file.read_to_string(&mut raw) {
        Ok(_) => PolicyFile::Trusted(raw),
        Err(_) => PolicyFile::Missing,
    }
}

/// Whether the open `file` is one a standard user could not have written.
///
/// Windows: the owner, read from the handle, must be `BUILTIN\Administrators`
/// or `SYSTEM` (a user who creates a file owns it, and cannot hand ownership
/// to another principal without privileges a standard user lacks).
/// Elsewhere the system locations are already root/admin-only, so the
/// location is the main check; the handle must still be a regular file that
/// is not world-writable.
fn handle_is_admin_owned(file: &std::fs::File) -> bool {
    #[cfg(windows)]
    {
        win::handle_owned_by_admins_or_system(file)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o002 == 0)
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        false
    }
}

/// Whether the folder holding `path` is one a standard user could not have
/// created or rewritten (same rules as [`handle_is_admin_owned`]).
fn folder_is_admin_owned(path: &std::path::Path) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    #[cfg(windows)]
    {
        win::owned_by_admins_or_system(dir)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(dir)
            .map(|m| m.is_dir() && m.permissions().mode() & 0o002 == 0)
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = dir;
        false
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HANDLE, HLOCAL, WIN32_ERROR};
    use windows::Win32::Security::Authorization::{
        GetNamedSecurityInfoW, GetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID,
    };
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    /// Which policy key to read: the current product name's, or the one an
    /// admin deployed for MaeclyPrint (the old name).
    #[derive(Clone, Copy)]
    pub(super) enum PolicyKey {
        Current,
        Legacy,
    }

    /// A REG_SZ under the policy key, or `None` when the key or value is absent.
    pub(super) fn read_policy_string(which: PolicyKey, value: &str) -> Option<String> {
        let key = match which {
            PolicyKey::Current => w!("SOFTWARE\\Policies\\Maecly\\Pluryprint"),
            PolicyKey::Legacy => w!("SOFTWARE\\Policies\\Maecly\\MaeclyPrint"),
        };
        let name: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let mut size = 0u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
        };
        if rc != ERROR_SUCCESS || size < 2 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    pub(super) fn owned_by_admins_or_system(path: &Path) -> bool {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut owner = PSID::default();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let rc = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                None,
                None,
                &mut sd,
            )
        };
        owner_is_trusted(rc, owner, sd)
    }

    /// The owner of the object behind the open `file` handle, so the check
    /// and the read that follows see the same file.
    pub(super) fn handle_owned_by_admins_or_system(file: &std::fs::File) -> bool {
        use std::os::windows::io::AsRawHandle;
        let handle = HANDLE(file.as_raw_handle());
        let mut owner = PSID::default();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        let rc = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                None,
                None,
                Some(&mut sd),
            )
        };
        owner_is_trusted(rc, owner, sd)
    }

    /// Whether a security lookup succeeded and named Administrators or
    /// SYSTEM as the owner. Frees `sd` either way.
    fn owner_is_trusted(rc: WIN32_ERROR, owner: PSID, sd: PSECURITY_DESCRIPTOR) -> bool {
        let trusted = rc == ERROR_SUCCESS
            && !owner.0.is_null()
            && unsafe {
                IsWellKnownSid(owner, WinBuiltinAdministratorsSid).as_bool()
                    || IsWellKnownSid(owner, WinLocalSystemSid).as_bool()
            };
        // `owner` points into `sd`, so it is only freed after the check.
        if !sd.0.is_null() {
            unsafe {
                let _ = LocalFree(HLOCAL(sd.0));
            }
        }
        trusted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_policy_reads_default_printer() {
        let p = parse_policy(r#"{"default_printer": "HP_LaserJet"}"#).expect("valid json");
        assert_eq!(p.default_printer.as_deref(), Some("HP_LaserJet"));
    }

    #[test]
    fn parse_policy_defaults_missing_field() {
        // A present but field-less object is valid; default_printer is None.
        let p = parse_policy("{}").expect("empty object is valid");
        assert_eq!(p.default_printer, None);
    }

    /// ADR-0009: a policy deployed for MaeclyPrint is still read, but only
    /// after the Pluryprint location.
    #[test]
    fn legacy_policy_location_is_consulted_after_the_current_one() {
        let order = sources();
        let current = system_policy_path(APP_DIR);
        let legacy = system_policy_path(LEGACY_APP_DIR);
        if let (Some(current), Some(legacy)) = (current, legacy) {
            let at = |p: &PathBuf| {
                order
                    .iter()
                    .position(|s| *s == Source::File(p.clone()))
                    .unwrap()
            };
            assert!(at(&current) < at(&legacy), "{order:?}");
            assert!(!is_legacy(&current), "{current:?}");
            assert!(is_legacy(&legacy), "{legacy:?}");
            assert!(
                legacy
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("maeclyprint"),
                "{legacy:?}"
            );
        }
    }

    /// Every new location, registry or file, comes before every legacy one:
    /// an admin who moved to the new file but left the old GPO in place gets
    /// the new printer.
    #[test]
    fn new_locations_beat_legacy_ones_of_either_kind() {
        let order = sources();
        let first_legacy = order
            .iter()
            .position(Source::is_legacy)
            .unwrap_or(order.len());
        assert!(
            order[..first_legacy].iter().all(|s| !s.is_legacy()),
            "{order:?}"
        );
        assert!(
            order[first_legacy..]
                .iter()
                .all(|s| s.is_legacy() || is_dev_fallback(s)),
            "{order:?}"
        );
    }

    fn policy(printer: &str) -> Found {
        Found::Policy(PrinterPolicy {
            default_printer: Some(printer.into()),
        })
    }

    /// The pure resolution: first source with something wins.
    #[test]
    fn resolve_takes_the_first_source_that_has_a_policy() {
        let new_file = Source::File(PathBuf::from("/new/policy.json"));
        let old_file = Source::File(PathBuf::from("/MaeclyPrint/policy.json"));
        let order = vec![
            Source::Registry { legacy: false },
            new_file.clone(),
            Source::Registry { legacy: true },
            old_file.clone(),
        ];

        // New file vs. legacy GPO: the new file wins.
        let got = resolve(&order, |s| match s {
            Source::Registry { legacy: true } => policy("OldGpo"),
            s if *s == new_file => policy("NewFile"),
            _ => Found::Nothing,
        });
        assert_eq!(got.default_printer.as_deref(), Some("NewFile"));

        // New GPO vs. new file: the registry wins, as before.
        let got = resolve(&order, |s| match s {
            Source::Registry { legacy: false } => policy("NewGpo"),
            s if *s == new_file => policy("NewFile"),
            _ => Found::Nothing,
        });
        assert_eq!(got.default_printer.as_deref(), Some("NewGpo"));

        // Only legacy sources: still honoured, registry first.
        let got = resolve(&order, |s| match s {
            Source::Registry { legacy: true } => policy("OldGpo"),
            s if *s == old_file => policy("OldFile"),
            _ => Found::Nothing,
        });
        assert_eq!(got.default_printer.as_deref(), Some("OldGpo"));

        // A malformed trusted file is no policy, and stops the search.
        let got = resolve(&order, |s| match s {
            Source::Registry { legacy: false } => Found::Nothing,
            s if *s == new_file => Found::Malformed,
            _ => policy("Later"),
        });
        assert!(got.default_printer.is_none());

        assert!(resolve(&order, |_| Found::Nothing)
            .default_printer
            .is_none());
    }

    #[test]
    fn parse_policy_rejects_malformed() {
        assert!(parse_policy("{not json").is_none());
        assert!(parse_policy("").is_none());
    }

    #[test]
    fn registry_value_is_a_policy_only_when_it_names_a_printer() {
        assert_eq!(
            policy_from_registry_value(Some(" Front Desk ".into()))
                .and_then(|p| p.default_printer)
                .as_deref(),
            Some("Front Desk")
        );
        assert!(policy_from_registry_value(Some("  ".into())).is_none());
        assert!(policy_from_registry_value(None).is_none());
    }

    #[test]
    fn load_never_errors() {
        // Whatever is (or isn't) on disk, load() must yield a value.
        let _ = PrinterPolicy::load();
    }

    /// A file a standard user just created is theirs, not the
    /// administrators', so it must not count as an organization policy.
    #[cfg(windows)]
    #[test]
    fn a_user_created_policy_file_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        std::fs::write(&path, r#"{"default_printer":"Fake"}"#).unwrap();
        // CI runners run tests as an administrator, whose files may be owned
        // by the Administrators group; only assert when that is not the case.
        if !win::owned_by_admins_or_system(&path) {
            let file = std::fs::File::open(&path).unwrap();
            assert!(!handle_is_admin_owned(&file));
            assert_eq!(read_trusted(&path), PolicyFile::Untrusted);
        }
        assert!(!win::owned_by_admins_or_system(
            &dir.path().join("missing.json")
        ));
    }

    /// The trust decision and the read use one handle: what is returned is the
    /// content of the file that was checked, and a missing file is not a
    /// policy.
    #[cfg(unix)]
    #[test]
    fn read_trusted_reads_the_checked_handle() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.path().join("policy.json");
        assert_eq!(read_trusted(&path), PolicyFile::Missing);

        std::fs::write(&path, r#"{"default_printer":"Front"}"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            read_trusted(&path),
            PolicyFile::Trusted(r#"{"default_printer":"Front"}"#.into())
        );

        // Anyone could have rewritten a world-writable file or folder.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(read_trusted(&path), PolicyFile::Untrusted);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(read_trusted(&path), PolicyFile::Untrusted);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        // A directory where the file should be is not a policy file.
        let not_a_file = dir.path().join("dir.json");
        std::fs::create_dir(&not_a_file).unwrap();
        assert_ne!(
            read_trusted(&not_a_file),
            PolicyFile::Trusted(String::new())
        );
    }

    /// Release builds never consult the per-user fallback.
    #[test]
    fn per_user_fallback_is_debug_only() {
        let user = crate::util::paths::config_dir().join("policy.json");
        assert_eq!(
            sources().contains(&Source::File(user)),
            cfg!(debug_assertions)
        );
    }
}
