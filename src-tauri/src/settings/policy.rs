//! Organization printer policy (`policy.json`).
//!
//! A read-only, admin-managed file that lets an organization pin MaeclyPrint
//! behavior. Today it carries a single field — the enforced default printer —
//! which the UI presents as a locked "managed by your organization" state.
//!
//! Resolution order (first hit wins): the machine-wide system location for the
//! platform, then the per-user config dir (`<config_dir>/policy.json`) as a
//! developer/testing fallback. A missing file is not an error — policy is
//! optional and absence yields [`PrinterPolicy::default`]. Malformed JSON is
//! logged and also degrades to the default rather than failing app startup.
//!
//! | Platform | System path                                             |
//! | -------- | ------------------------------------------------------- |
//! | Windows  | `C:\ProgramData\MaeclyPrint\policy.json`                |
//! | macOS    | `/Library/Application Support/MaeclyPrint/policy.json`  |
//! | Linux    | `/etc/maeclyprint/policy.json`                          |

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::util::paths;

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
    /// Load the effective policy, checking the system path before the per-user
    /// config-dir fallback. Missing files are skipped; a malformed file is
    /// logged and treated as no-policy.
    pub fn load() -> Self {
        for path in policy_paths() {
            let Ok(raw) = std::fs::read_to_string(&path) else {
                // Missing or unreadable — try the next candidate.
                continue;
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

/// Candidate policy locations, most authoritative first.
fn policy_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(system) = system_policy_path() {
        out.push(system);
    }
    out.push(paths::config_dir().join("policy.json"));
    out
}

/// The machine-wide policy path for this platform, if one is defined.
fn system_policy_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        // ProgramData is machine-wide and admin-writable, per the spec.
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
    fn load_never_errors() {
        // Whatever is (or isn't) on disk, load() must yield a value.
        let _ = PrinterPolicy::load();
    }
}
