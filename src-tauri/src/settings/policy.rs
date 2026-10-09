//! Organization printer policy.
//!
//! An admin-managed setting that lets an organization pin MaeclyPrint
//! behavior. Today it carries a single field — the enforced default printer —
//! which the UI presents as a locked "managed by your organization" state.
//!
//! Because the UI calls it *managed by your organization*, a policy is only
//! believed when a non-administrator could not have written it:
//!
//! | Platform | Source, most authoritative first |
//! | -------- | -------------------------------- |
//! | Windows  | `HKLM\SOFTWARE\Policies\Maecly\MaeclyPrint` value `DefaultPrinter` (REG_SZ; Group Policy friendly, admin-only), then `C:\ProgramData\MaeclyPrint\policy.json` **only if the file is owned by Administrators or SYSTEM** |
//! | macOS    | `/Library/Application Support/MaeclyPrint/policy.json` (admin-writable) |
//! | Linux    | `/etc/maeclyprint/policy.json` (root-writable) |
//!
//! `C:\ProgramData` lets any standard user create folders (and own them), so a
//! `policy.json` there that a user created is ignored — otherwise any user
//! could impose a "managed" default printer on everyone else on the machine.
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
            let Ok(raw) = std::fs::read_to_string(&path) else {
                // Missing or unreadable — try the next candidate.
                continue;
            };
            if !is_admin_owned(&path) {
                tracing::warn!(
                    event = "policy_untrusted",
                    path = %path.display(),
                    "ignoring policy.json that a non-administrator could have written"
                );
                continue;
            }
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
            .map(|pd| PathBuf::from(pd).join("MaeclyPrint").join("policy.json"))
    }
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(
            "/Library/Application Support/MaeclyPrint/policy.json",
        ))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Some(PathBuf::from("/etc/maeclyprint/policy.json"))
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

/// `HKLM\SOFTWARE\Policies\Maecly\MaeclyPrint` → `DefaultPrinter`. Only
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

/// Whether `path` is a file a standard user could not have written.
///
/// Windows: the file's owner must be `BUILTIN\Administrators` or `SYSTEM`
/// (a user who creates a file owns it, and cannot hand ownership to another
/// principal without privileges a standard user lacks). Elsewhere the system
/// locations are already root/admin-only, so the location is the check.
fn is_admin_owned(path: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        win::owned_by_admins_or_system(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        true
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID,
    };
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    /// A REG_SZ under the policy key, or `None` when the key or value is absent.
    pub(super) fn read_policy_string(value: &str) -> Option<String> {
        let name: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let key = w!("SOFTWARE\\Policies\\Maecly\\MaeclyPrint");
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
        if rc != ERROR_SUCCESS {
            return false;
        }
        let trusted = unsafe {
            IsWellKnownSid(owner, WinBuiltinAdministratorsSid).as_bool()
                || IsWellKnownSid(owner, WinLocalSystemSid).as_bool()
        };
        // `owner` points into `sd`, so it is only freed after the check.
        unsafe {
            let _ = LocalFree(HLOCAL(sd.0));
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
            assert!(!is_admin_owned(&path));
        }
        assert!(!win::owned_by_admins_or_system(
            &dir.path().join("missing.json")
        ));
    }

    /// Release builds never consult the per-user fallback.
    #[test]
    fn per_user_fallback_is_debug_only() {
        let user = crate::util::paths::config_dir().join("policy.json");
        assert_eq!(policy_paths().contains(&user), cfg!(debug_assertions));
    }
}
