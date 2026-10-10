//! The one predicate that decides whether a reference in a document (an
//! `<image href>`, an `<img src>`, a CSS `url()`, a page's `<base href>`)
//! stays on the local disk, or would make Windows open a network share.
//!
//! **Why it matters.** Opening `\\host\share\x.png` makes Windows connect to
//! `host` over SMB (or WebDAV) and hand it the user's NTLM credentials, even
//! when the file is refused a moment later. A document someone sent the user
//! must never trigger that, so every converter that resolves references
//! (SVG, HTML, Markdown, the page renderer's own file server) asks this module
//! and nothing else. Ad-hoc `starts_with("\\\\")` checks have been bypassed
//! before (`/\host`, `\/host`, `file:///%5C%5Chost`), which is why there is one
//! implementation, tested against a table of bypass vectors.
//!
//! **How it decides** ([`classify`]), on the *fully* normalised value, the way
//! Windows and a WHATWG URL parser would read it:
//!
//! 1. ASCII tabs and newlines are removed anywhere (URL parsers do that), and
//!    leading and trailing spaces and control characters are trimmed;
//! 2. percent-escapes are decoded until nothing changes (so `%255C` cannot
//!    smuggle a backslash through two decoders);
//! 3. every `\` becomes `/` (Windows accepts both, and so do URL parsers for
//!    `file:`);
//! 4. then a value that starts with two separators (`//host`, `\\host`,
//!    `/\host`, `\\?\UNC\host`, the device namespace `\\.\`, `\\?\`) or with the
//!    NT prefix `\??\` is a [`RefClass::Share`]; a `file:` URL is a share when it
//!    names a host (`file://host/`) or its path is one (`file:////host`);
//!    `data:` is [`RefClass::Data`]; any other scheme (`http:`, `smb:`, ...) is
//!    [`RefClass::OtherScheme`]; everything else (a relative path, `/x`,
//!    `C:\x`, a host-less `file:///`) is [`RefClass::Local`].
//!
//! [`is_remote_path`] applies the same rule to a path that was already
//! resolved (joined onto a folder), right before it is opened.

use std::path::Path;

/// What a reference names, judged before anything touches the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefClass {
    /// A path on the local disk: relative, rooted, a drive path, or a
    /// host-less `file:///` URL.
    Local,
    /// A `data:` URI: inline bytes, nothing is opened.
    Data,
    /// A network share or device path: UNC (`\\host\share` in any slash
    /// mix), `\\?\UNC\`, `\\.\`, `\??\`, `file://host/`, `file:////host`.
    /// Opening it can leak the user's NTLM credentials.
    Share,
    /// Any other URL scheme (`http:`, `https:`, `ftp:`, `smb:`, `javascript:`).
    OtherScheme,
}

/// How many rounds of percent-decoding [`normalize`] applies at most. Three
/// is already more than any real reference needs; the cap only stops a
/// pathological value from looping.
const MAX_DECODE_ROUNDS: usize = 8;

/// `reference` as Windows and a URL parser would finally read it: tabs and
/// newlines removed, trimmed, percent-escapes decoded until stable, `\` turned
/// into `/`. Pure.
pub fn normalize(reference: &str) -> String {
    let mut value: String = reference
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect::<String>()
        .trim_matches(|c: char| c == ' ' || c.is_control())
        .to_string();
    for _ in 0..MAX_DECODE_ROUNDS {
        let decoded = percent_encoding::percent_decode_str(&value)
            .decode_utf8_lossy()
            .into_owned();
        if decoded == value {
            break;
        }
        value = decoded;
    }
    value
        .trim_matches(|c: char| c == ' ' || c.is_control())
        .replace('\\', "/")
}

/// The scheme of a normalised reference (`http` in `http://x`), if it has
/// one. A single letter followed by `:` is a Windows drive, not a scheme.
fn scheme_of(value: &str) -> Option<&str> {
    let (scheme, _) = value.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    let valid = first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (valid && scheme.len() > 1).then_some(scheme)
}

/// Whether a normalised path (only `/` separators) names a share or a
/// device: two leading separators, or the NT `\??\` prefix. `//?/C:/x` (a
/// verbatim *drive* path, the form a canonical Windows path takes) is local.
fn path_is_share(path: &str) -> bool {
    if let Some(rest) = path.strip_prefix("//?/") {
        let b = rest.as_bytes();
        let verbatim_drive = b.len() >= 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b.len() == 2 || b[2] == b'/');
        return !verbatim_drive;
    }
    path.starts_with("//") || path.starts_with("/??/")
}

/// Classify a reference found in a document (module docs). Pure.
pub fn classify(reference: &str) -> RefClass {
    let value = normalize(reference);
    if path_is_share(&value) {
        return RefClass::Share;
    }
    let Some(scheme) = scheme_of(&value) else {
        return RefClass::Local;
    };
    match scheme.to_ascii_lowercase().as_str() {
        "data" => RefClass::Data,
        "file" => classify_file_url(&value[scheme.len() + 1..]),
        _ => RefClass::OtherScheme,
    }
}

/// Classify what follows `file:` in a normalised `file:` URL, the way a
/// WHATWG parser reads it: `//host/...` names a host unless the host is
/// empty, `localhost` or a drive letter (`file://C:/x`); the path that
/// remains is then checked like any other.
fn classify_file_url(rest: &str) -> RefClass {
    let path = match rest.strip_prefix("//") {
        Some(after) => {
            let (host, path) = match after.find('/') {
                Some(i) => (&after[..i], &after[i..]),
                None => (after, ""),
            };
            let b = host.as_bytes();
            let is_drive =
                b.len() == 2 && b[0].is_ascii_alphabetic() && matches!(b[1], b':' | b'|');
            if !(host.is_empty() || host.eq_ignore_ascii_case("localhost") || is_drive) {
                return RefClass::Share;
            }
            if is_drive {
                return RefClass::Local;
            }
            path
        }
        None => rest,
    };
    // `/C:/x` is a drive path; anything else that starts with two
    // separators (`file:////host/x`, `file:///\\host`) is a share.
    if path_is_share(path) {
        RefClass::Share
    } else {
        RefClass::Local
    }
}

/// Whether `reference` would open a share or device path (module docs).
pub fn is_share_ref(reference: &str) -> bool {
    classify(reference) == RefClass::Share
}

/// Whether `reference` stays on the local disk: a local path or a `data:`
/// URI. A share, and any other scheme, is not.
pub fn is_local_ref(reference: &str) -> bool {
    matches!(classify(reference), RefClass::Local | RefClass::Data)
}

/// Whether an already resolved `path` is a share or device path, the check
/// made right before the path is opened. Pure.
pub fn is_remote_path(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    path_is_share(&text)
}

/// Whether `path` may be opened on behalf of a document whose folder is
/// `dir`: it is not a share or device path, unless it lies (textually) inside
/// `dir` itself — a document the user opened from a share may read its own
/// folder there. Pure.
pub fn may_open(path: &Path, dir: &Path) -> bool {
    if !is_remote_path(path) {
        return true;
    }
    let path = path.to_string_lossy().replace('\\', "/");
    let mut dir = dir.to_string_lossy().replace('\\', "/");
    if dir.is_empty() || !is_remote_path(Path::new(&dir)) {
        return false;
    }
    if !dir.ends_with('/') {
        dir.push('/');
    }
    path.len() > dir.len()
        && path[..dir.len()].eq_ignore_ascii_case(&dir)
        && !path[dir.len()..].split('/').any(|seg| seg == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NFR (security): every known way of spelling a share is a share,
    /// whatever the slashes, escapes or wrapping. Vectors from the round 3/4
    /// security reviews.
    #[test]
    fn nfr_security_share_bypass_vectors_table() {
        for v in [
            r"\\attacker\share\x.png",
            "//attacker/share/x.png",
            r"/\attacker\share\x.png",
            r"\/attacker/share/x.png",
            r"\\?\UNC\attacker\share\x.png",
            r"//?/UNC/attacker/share/x.png",
            r"\\.\pipe\x",
            r"\\.\C:\x.png",
            r"\\?\GLOBALROOT\Device\Mup\attacker\x",
            r"\??\UNC\attacker\share\x.png",
            "file://attacker/share/x.png",
            "FILE://attacker/x.png",
            "file:////attacker/share/x.png",
            "file://///attacker/share/x.png",
            r"file:\\attacker\share\x.png",
            r"file:/\attacker\share\x.png",
            "file:///%5C%5Cattacker%5Cshare%5Cx.png",
            "file:%2F%2Fattacker/x.png",
            "%5C%5Cattacker%5Cshare%5Cx.png",
            "%2F%2Fattacker/x.png",
            "%255C%255Cattacker%255Cx.png",
            "/%5Cattacker/x.png",
            "\t//attacker/x.png",
            "/\n/attacker/x.png",
            "  \\\\attacker\\x.png  ",
            "\u{0}//attacker/x.png",
        ] {
            assert_eq!(classify(v), RefClass::Share, "{v:?}");
            assert!(is_share_ref(v), "{v:?}");
            assert!(!is_local_ref(v), "{v:?}");
        }
    }

    #[test]
    fn nfr_security_local_and_other_references() {
        for v in [
            "img/a.png",
            "./a b.png",
            "../up.png",
            "/abs/x.png",
            r"C:\pics\a.png",
            "c:/pics/a.png",
            "C:x.png",
            "file:///C:/pics/a%20b.png",
            "file:///tmp/a.png",
            "file://localhost/tmp/a.png",
            "file://C:/x.png",
            "file:/tmp/a.png",
            "a%20b.png",
            r"\\?\C:\docs\a.png",
            "x#frag",
        ] {
            assert_eq!(classify(v), RefClass::Local, "{v:?}");
            assert!(is_local_ref(v), "{v:?}");
        }
        assert_eq!(classify("data:image/png;base64,AA=="), RefClass::Data);
        for v in [
            "http://example.com/x.png",
            "https://example.com/x.png",
            "smb://host/x",
            "javascript:alert(1)",
            "ftp://h/x",
        ] {
            assert_eq!(classify(v), RefClass::OtherScheme, "{v:?}");
            assert!(!is_local_ref(v) && !is_share_ref(v), "{v:?}");
        }
    }

    #[test]
    fn nfr_security_resolved_paths() {
        for p in [
            r"\\host\share\x",
            "//host/share/x",
            r"/\host\x",
            r"\\?\UNC\host\x",
            r"\\.\pipe\x",
            r"\??\UNC\host\x",
        ] {
            assert!(is_remote_path(Path::new(p)), "{p}");
        }
        for p in [
            r"C:\docs\x.png",
            "/tmp/x.png",
            r"\\?\C:\docs\x.png",
            "rel/x",
        ] {
            assert!(!is_remote_path(Path::new(p)), "{p}");
        }
        // A document on a share may open its own folder there, nothing else.
        let share = Path::new(r"\\server\docs\report");
        assert!(may_open(
            Path::new(r"\\server\docs\report\img\a.png"),
            share
        ));
        assert!(!may_open(
            Path::new(r"\\server\docs\report\..\x.png"),
            share
        ));
        assert!(!may_open(Path::new(r"\\attacker\x\a.png"), share));
        assert!(!may_open(
            Path::new(r"\\attacker\x\a.png"),
            Path::new(r"C:\docs")
        ));
        assert!(may_open(Path::new(r"C:\docs\a.png"), Path::new(r"C:\docs")));
    }
}
