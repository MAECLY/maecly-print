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
    /// Load the effective policy: the Windows registry policy first, then the
    /// policy files in order. Untrusted, missing or unreadable files are
    /// skipped; a malformed trusted file is logged and treated as no-policy.
    pub fn load() -> Self {
        if let Some(policy) = registry_policy() {
            return policy;
        }
        for path in policy_paths() {
            let raw = match read_trusted(&path) {
                // Missing or unreadable — try the next candidate.
                PolicyFile::Missing => continue,
                PolicyFile::Untrusted => {
                    tracing::warn!(
                        event = "policy_untrusted",
                        path = %path.display(),
                        "ignoring policy.json that a non-administrator could have written"
                    );
                    continue;
                }
                PolicyFile::Trusted(raw) => raw,
            };
            return match parse_policy(&raw) {
                Some(policy) => policy,
                None => {
                    tracing::warn!(
                        event = "policy_parse_failed",
                        path = %path.display(),
                        "ignoring malformed policy.json"
                    );
                    Self::default()
                }
            };
        }
        Self::default()
    }
}

/// Candidate policy files, most authoritative first.
fn policy_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(system) = system_policy_path() {
        out.push(system);
    }
    // Developer/testing fallback: never in a release build.
    #[cfg(debug_assertions)]
    out.push(crate::util::paths::config_dir().join("policy.json"));
    out
}

/// The machine-wide policy path for this platform, if one is defined.
fn system_policy_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("ProgramData")
            .map(|pd| PathBuf::from(pd).join("Pluryprint").join("policy.json"))
    }
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(
            "/Library/Application Support/Pluryprint/policy.json",
        ))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Some(PathBuf::from("/etc/pluryprint/policy.json"))
    }
    #[cfg(not(any(windows, unix)))]
    {
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

/// `HKLM\SOFTWARE\Policies\Maecly\Pluryprint` → `DefaultPrinter`. Only
/// administrators (and Group Policy) can write under `HKLM\SOFTWARE\Policies`.
fn registry_policy() -> Option<PrinterPolicy> {
    #[cfg(windows)]
    {
        policy_from_registry_value(win::read_policy_string("DefaultPrinter"))
    }
    #[cfg(not(windows))]
    {
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

    /// A REG_SZ under the policy key, or `None` when the key or value is absent.
    pub(super) fn read_policy_string(value: &str) -> Option<String> {
        let name: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let key = w!("SOFTWARE\\Policies\\Maecly\\Pluryprint");
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
        assert_eq!(policy_paths().contains(&user), cfg!(debug_assertions));
    }
}
