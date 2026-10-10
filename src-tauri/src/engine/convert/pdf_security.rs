//! Encrypted-PDF inspection and decryption (FR-059).
//!
//! [`inspect`] decides what a PDF's security handler means for printing it:
//!
//! 1. **Fast path, no parse.** The trailer of the last cross-reference section
//!    (the classic `trailer` just before `startxref`, or the dictionary of the
//!    xref stream `startxref` points at) repeats `/Encrypt` in every encrypted
//!    file (ISO 32000-1 §7.5.8). When neither mentions it the file is
//!    [`PdfSecurity::Plain`] and costs two small reads.
//! 2. Otherwise `lopdf` loads it. lopdf authenticates the **empty user
//!    password** on its own: a file that opens with it carries only an owner
//!    password ([`PdfSecurity::OwnerOnly`], printable in place when its
//!    `PRINTABLE` permission is set); a file that does not
//!    ([`PdfSecurity::NeedsPassword`]) cannot be read without the user's
//!    password. A security handler lopdf cannot read (public-key, an unknown
//!    revision) is `PdfProtected(Unsupported)`.
//!
//! [`decrypt_to`] opens a protected PDF with the password the user typed and
//! writes a decrypted copy (the job's cache PDF; the caller deletes it with the
//! job). Approach chosen for the AGPL build: `lopdf` (MIT) in-process, no
//! external tool, so the password never reaches a command line (Sumatra's
//! `-pwd`, `qpdf --password=`), an environment block or a log.
//!
//! The no-print permission is always honoured (decision D12): a PDF whose
//! permissions forbid printing is refused, with or without a password, and
//! never stripped.
//!
//! What stays in memory: `lopdf` copies the password into its own `String`
//! while it derives the key; those copies live only for the duration of the
//! load. Our own copy is a [`PdfPassword`] (zeroed on drop).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use lopdf::encryption::DecryptionError;
use lopdf::{Document, Object, Permissions};

use crate::engine::{EngineError, Failure, PdfPassword, PdfProtection};

/// What a PDF's security handler means for printing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfSecurity {
    /// Not encrypted.
    Plain,
    /// Encrypted with an owner password only: it opens with the empty user
    /// password. `print` is whether its permissions allow printing.
    OwnerOnly {
        /// The `PRINTABLE` permission bit.
        print: bool,
    },
    /// Encrypted with a user password: nothing can be read without it.
    NeedsPassword,
}

/// How much of the file's end is searched for the last trailer. The trailer
/// sits right before `startxref`, a few hundred bytes from the end.
const TAIL_WINDOW: u64 = 16 * 1024;

/// How much is read at the `startxref` offset: enough for an xref stream's
/// dictionary (which is never compressed nor encrypted).
const XREF_WINDOW: u64 = 4 * 1024;

/// Inspect the PDF at `path` (`size` bytes) for encryption.
pub fn inspect(path: &Path, size: u64) -> Result<PdfSecurity, EngineError> {
    if !mentions_encrypt(path, size)? {
        return Ok(PdfSecurity::Plain);
    }
    let doc = Document::load(path).map_err(|e| unreadable_encryption(&e))?;
    classify(&doc)
}

/// What a loaded document's encryption means. Pure over `doc`.
fn classify(doc: &Document) -> Result<PdfSecurity, EngineError> {
    if let Some(state) = &doc.encryption_state {
        // lopdf opened it with the empty user password: owner password only.
        return Ok(PdfSecurity::OwnerOnly {
            print: state.permissions().contains(Permissions::PRINTABLE),
        });
    }
    if !doc.is_encrypted() {
        // `/Encrypt` appeared somewhere near the trailer without being one
        // (a string, a stale section): not encrypted after all.
        return Ok(PdfSecurity::Plain);
    }
    check_handler(doc)?;
    // Standard handler, and the empty password did not open it. Tell an
    // unreadable revision apart from a real user password.
    match doc.authenticate_user_password("") {
        Ok(()) => Ok(PdfSecurity::OwnerOnly { print: true }),
        Err(lopdf::Error::Decryption(DecryptionError::IncorrectPassword)) => {
            Ok(PdfSecurity::NeedsPassword)
        }
        Err(e) => Err(unreadable_encryption(&e)),
    }
}

/// Refuse a security handler other than the standard password one (for
/// example `/Adobe.PubSec`, certificate-based).
fn check_handler(doc: &Document) -> Result<(), EngineError> {
    let filter = doc
        .get_encrypted()
        .ok()
        .and_then(|d| d.get(b"Filter").ok())
        .and_then(|f| f.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned());
    match filter.as_deref() {
        Some("Standard") => Ok(()),
        Some(other) => Err(EngineError::PdfProtected(PdfProtection::Unsupported(
            format!("security handler {other}"),
        ))),
        None => Err(EngineError::PdfProtected(PdfProtection::Unsupported(
            "no security handler".into(),
        ))),
    }
}

/// An encrypted file lopdf cannot open at all.
fn unreadable_encryption(e: &lopdf::Error) -> EngineError {
    EngineError::PdfProtected(PdfProtection::Unsupported(e.to_string()))
}

/// Whether the last trailer of the PDF at `path` names an `/Encrypt`
/// dictionary. Reads the tail and the few KiB at `startxref`; never parses.
fn mentions_encrypt(path: &Path, size: u64) -> Result<bool, EngineError> {
    let mut file = std::fs::File::open(path).map_err(|e| cannot_read(path, e))?;
    let tail = read_at(&mut file, size.saturating_sub(TAIL_WINDOW), TAIL_WINDOW)
        .map_err(|e| cannot_read(path, e))?;
    if contains(&tail, b"/Encrypt") {
        return Ok(true);
    }
    // An xref stream keeps the trailer entries in its own dictionary, at the
    // offset `startxref` names (often far from the end).
    match startxref_offset(&tail) {
        Some(offset) if offset < size => {
            let at = read_at(&mut file, offset, XREF_WINDOW).map_err(|e| cannot_read(path, e))?;
            Ok(contains(&at, b"/Encrypt"))
        }
        _ => Ok(false),
    }
}

/// The offset after the last `startxref` keyword in `tail`. Pure.
fn startxref_offset(tail: &[u8]) -> Option<u64> {
    const KEY: &[u8] = b"startxref";
    let at = tail.windows(KEY.len()).rposition(|w| w == KEY)? + KEY.len();
    let digits: String = tail[at..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    digits.parse().ok()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn read_at(file: &mut std::fs::File, offset: u64, len: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::with_capacity(len as usize);
    file.take(len).read_to_end(&mut buf)?;
    Ok(buf)
}

fn cannot_read(path: &Path, e: std::io::Error) -> EngineError {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    EngineError::ConversionFailed(
        Failure::new(
            "cannot_read_input",
            format!("cannot read {}: {e}", path.display()),
        )
        .param("file", name),
    )
}

/// Open the PDF at `input` with `password` and write a decrypted copy to
/// `output`. A wrong password is
/// [`EngineError::PdfProtected`]`(`[`PdfProtection::PasswordIncorrect`]`)`;
/// a file whose permissions forbid printing is `PrintingNotAllowed` (checked
/// whichever password opened it: the copy is only ever made to print it).
///
/// The copy is checked before it is handed on: it must load without a
/// password and have as many pages as the original; otherwise the job fails
/// with `pdf_decrypt_failed` rather than printing something incomplete.
pub fn decrypt_to(input: &Path, password: &PdfPassword, output: &Path) -> Result<(), EngineError> {
    let mut doc = Document::load_with_password(input, password.expose()).map_err(|e| match e {
        // [`inspect`] already vetted the security handler, so a password
        // lopdf rejects is simply the wrong one.
        lopdf::Error::InvalidPassword => {
            EngineError::PdfProtected(PdfProtection::PasswordIncorrect)
        }
        e => unreadable_encryption(&e),
    })?;
    let Some(state) = doc.encryption_state.take() else {
        return Err(if doc.is_encrypted() {
            // lopdf keeps a document it could not authenticate encrypted.
            check_handler(&doc)
                .err()
                .unwrap_or(EngineError::PdfProtected(PdfProtection::PasswordIncorrect))
        } else {
            decrypt_failed("the file is not encrypted")
        });
    };
    if !state.permissions().contains(Permissions::PRINTABLE) {
        return Err(EngineError::PdfProtected(PdfProtection::PrintingNotAllowed));
    }
    let pages = doc.get_pages().len();
    if pages == 0 {
        return Err(decrypt_failed("the decrypted document has no pages"));
    }

    // Every object is already decrypted in memory; drop the security handler
    // so the copy is written (and read back) as a plain PDF.
    if let Ok(Object::Reference(id)) = doc.trailer.get(b"Encrypt").cloned() {
        doc.objects.remove(&id);
    }
    doc.trailer.remove(b"Encrypt");

    let written = std::fs::File::create(output)
        .map_err(|e| e.to_string())
        .and_then(|mut f| doc.save_to(&mut f).map_err(|e| e.to_string()));
    if let Err(e) = written {
        return Err(EngineError::ConversionFailed(Failure::new(
            "cannot_write_output",
            format!("cannot write the decrypted PDF {}: {e}", output.display()),
        )));
    }

    match Document::load(output) {
        Ok(copy) if !copy.is_encrypted() && copy.get_pages().len() == pages => Ok(()),
        Ok(copy) => Err(decrypt_failed(&format!(
            "the decrypted copy has {} of {pages} pages",
            copy.get_pages().len()
        ))),
        Err(e) => Err(decrypt_failed(&format!(
            "the decrypted copy cannot be read: {e}"
        ))),
    }
}

fn decrypt_failed(why: &str) -> EngineError {
    EngineError::ConversionFailed(Failure::new(
        "pdf_decrypt_failed",
        format!("the PDF could not be decrypted: {why}"),
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
    use lopdf::encryption::{EncryptionState, EncryptionVersion};
    use lopdf::{dictionary, Stream};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// Real files written by macOS's Quartz PDF writer (AES-128); see
    /// `tests/fixtures/pdf_enc/README.md`.
    pub(crate) const QUARTZ_USER: &[u8] =
        include_bytes!("../../../tests/fixtures/pdf_enc/user.pdf");
    pub(crate) const QUARTZ_OWNER: &[u8] =
        include_bytes!("../../../tests/fixtures/pdf_enc/owner.pdf");
    pub(crate) const QUARTZ_NOPRINT: &[u8] =
        include_bytes!("../../../tests/fixtures/pdf_enc/noprint.pdf");
    /// The user password of [`QUARTZ_USER`].
    pub(crate) const QUARTZ_PASSWORD: &str = "s3cret";

    /// Which encryption [`encrypted_pdf`] applies.
    #[derive(Clone, Copy)]
    pub(crate) enum Scheme {
        Rc4,
        Aes256,
    }

    /// A plain document of `pages` pages, with a file `/ID` (needed by the
    /// RC4/AES-128 key derivation).
    pub(crate) fn plain_doc(pages: usize) -> Document {
        let mut d = Document::with_version("1.7");
        let pages_id = d.new_object_id();
        let mut kids = Vec::new();
        for i in 0..pages {
            let content = d.add_object(Stream::new(
                dictionary! {},
                format!("BT /F1 24 Tf 72 720 Td (page {i}) Tj ET").into_bytes(),
            ));
            let page = d.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Contents" => content,
            });
            kids.push(Object::Reference(page));
        }
        d.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => pages as i64,
            }),
        );
        let catalog = d.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        d.trailer.set("Root", catalog);
        let id = Object::string_literal("0123456789abcdef");
        d.trailer.set("ID", Object::Array(vec![id.clone(), id]));
        d
    }

    /// `doc` encrypted with `scheme`, owner password `owner`, user password
    /// `user` (empty: owner-only) and `perms`, as bytes.
    pub(crate) fn encrypted_pdf(scheme: Scheme, user: &str, perms: Permissions) -> Vec<u8> {
        let mut d = plain_doc(2);
        let state = match scheme {
            Scheme::Rc4 => EncryptionState::try_from(EncryptionVersion::V2 {
                document: &d,
                owner_password: "owner",
                user_password: user,
                key_length: 128,
                permissions: perms,
            }),
            Scheme::Aes256 => {
                let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
                EncryptionState::try_from(EncryptionVersion::V5 {
                    encrypt_metadata: true,
                    crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
                    file_encryption_key: &[7u8; 32],
                    stream_filter: b"StdCF".to_vec(),
                    string_filter: b"StdCF".to_vec(),
                    owner_password: "owner",
                    user_password: user,
                    permissions: perms,
                })
            }
        }
        .unwrap();
        d.encrypt(&state).unwrap();
        let mut bytes = Vec::new();
        d.save_to(&mut bytes).unwrap();
        bytes
    }

    fn no_print() -> Permissions {
        Permissions::all() - Permissions::PRINTABLE - Permissions::PRINTABLE_IN_HIGH_QUALITY
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn inspect_bytes(bytes: &[u8]) -> Result<PdfSecurity, EngineError> {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "x.pdf", bytes);
        inspect(&p, bytes.len() as u64)
    }

    fn password(p: &str) -> PdfPassword {
        PdfPassword::new(p.to_string()).unwrap()
    }

    /// FR-059: every scheme and both password kinds are told apart — a
    /// user password (RC4, AES-128 from Quartz, AES-256) needs the password;
    /// an owner-only file opens with the empty one and reports its print bit.
    #[test]
    fn fr_059_inspect_classifies_every_scheme() {
        for scheme in [Scheme::Rc4, Scheme::Aes256] {
            assert_eq!(
                inspect_bytes(&encrypted_pdf(scheme, "user", Permissions::all())).unwrap(),
                PdfSecurity::NeedsPassword
            );
            assert_eq!(
                inspect_bytes(&encrypted_pdf(scheme, "", Permissions::all())).unwrap(),
                PdfSecurity::OwnerOnly { print: true }
            );
            assert_eq!(
                inspect_bytes(&encrypted_pdf(scheme, "", no_print())).unwrap(),
                PdfSecurity::OwnerOnly { print: false }
            );
        }
        assert_eq!(
            inspect_bytes(QUARTZ_USER).unwrap(),
            PdfSecurity::NeedsPassword
        );
        assert_eq!(
            inspect_bytes(QUARTZ_OWNER).unwrap(),
            PdfSecurity::OwnerOnly { print: true }
        );
        assert_eq!(
            inspect_bytes(QUARTZ_NOPRINT).unwrap(),
            PdfSecurity::OwnerOnly { print: false }
        );
    }

    /// FR-059: a plain PDF is recognised from the trailer alone, without a
    /// parse — even when it is not a PDF lopdf could parse at all.
    #[test]
    fn fr_059_plain_pdf_fast_path_no_parse() {
        let mut plain = Vec::new();
        plain_doc(1).save_to(&mut plain).unwrap();
        assert_eq!(inspect_bytes(&plain).unwrap(), PdfSecurity::Plain);
        // Unparseable garbage with a trailer that does not mention /Encrypt
        // is "plain" for this check (the page check deals with it).
        let junk = b"%PDF-1.7\n garbage garbage\ntrailer<</Root 1 0 R>>\nstartxref\n9\n%%EOF\n";
        assert_eq!(inspect_bytes(junk).unwrap(), PdfSecurity::Plain);
        // ...while an /Encrypt in the trailer forces the real check.
        let fake =
            b"%PDF-1.7\n garbage\ntrailer<</Root 1 0 R/Encrypt 9 0 R>>\nstartxref\n9\n%%EOF\n";
        assert!(matches!(
            inspect_bytes(fake),
            Err(EngineError::PdfProtected(PdfProtection::Unsupported(_)))
        ));
    }

    #[test]
    fn startxref_offset_reads_the_last_keyword() {
        assert_eq!(
            startxref_offset(b"startxref\n12\n%%EOF\nstartxref\r\n345\r\n%%EOF"),
            Some(345)
        );
        assert_eq!(startxref_offset(b"no keyword"), None);
        assert_eq!(startxref_offset(b"startxref\n%%EOF"), None);
    }

    /// FR-059: a public-key (certificate) security handler is refused as
    /// unsupported encryption, not asked for a password nobody has.
    #[test]
    fn fr_059_unknown_security_handler_is_unsupported() {
        let mut d = plain_doc(1);
        let enc = d.add_object(dictionary! {
            "Filter" => "Adobe.PubSec", "V" => 4, "R" => 4,
        });
        d.trailer.set("Encrypt", enc);
        let mut bytes = Vec::new();
        d.save_to(&mut bytes).unwrap();
        let err = inspect_bytes(&bytes).unwrap_err();
        assert!(
            matches!(&err, EngineError::PdfProtected(PdfProtection::Unsupported(d)) if d.contains("Adobe.PubSec")),
            "{err:?}"
        );
    }

    /// FR-059: the right password gives a plain copy with every page; the
    /// wrong one is `password_incorrect`; nothing about it reaches the error.
    #[test]
    fn fr_059_decrypt_to_writes_a_plain_copy_with_every_page() {
        let dir = tempfile::tempdir().unwrap();
        let cases: [(&str, Vec<u8>, &str); 3] = [
            (
                "rc4",
                encrypted_pdf(Scheme::Rc4, "user", Permissions::all()),
                "user",
            ),
            (
                "aes256",
                encrypted_pdf(Scheme::Aes256, "user", Permissions::all()),
                "user",
            ),
            ("quartz", QUARTZ_USER.to_vec(), QUARTZ_PASSWORD),
        ];
        for (name, bytes, pw) in cases {
            let input = write(dir.path(), &format!("{name}.pdf"), &bytes);
            let out = dir.path().join(format!("{name}.dec.pdf"));
            decrypt_to(&input, &password(pw), &out).unwrap_or_else(|e| panic!("{name}: {e}"));
            let copy = Document::load(&out).unwrap();
            assert!(!copy.is_encrypted(), "{name}: the copy is plain");
            assert_eq!(copy.get_pages().len(), 2, "{name}: every page");
            assert_eq!(
                inspect(&out, std::fs::metadata(&out).unwrap().len()).unwrap(),
                PdfSecurity::Plain
            );

            let err = decrypt_to(&input, &password("wrong-guess"), &out).unwrap_err();
            assert!(
                matches!(
                    err,
                    EngineError::PdfProtected(PdfProtection::PasswordIncorrect)
                ),
                "{name}: {err:?}"
            );
            assert!(!format!("{err:?} {err}").contains("wrong-guess"));
        }
    }

    /// D12: a PDF that forbids printing stays refused even with its password.
    #[test]
    fn fr_059_no_print_bit_refused_even_with_the_password() {
        let dir = tempfile::tempdir().unwrap();
        let input = write(
            dir.path(),
            "np.pdf",
            &encrypted_pdf(Scheme::Aes256, "user", no_print()),
        );
        let out = dir.path().join("np.dec.pdf");
        let err = decrypt_to(&input, &password("user"), &out).unwrap_err();
        assert!(
            matches!(
                err,
                EngineError::PdfProtected(PdfProtection::PrintingNotAllowed)
            ),
            "{err:?}"
        );
        assert!(!out.exists(), "no copy is written");
    }

    /// Serialise `obj` the way a PDF writer would (hex strings). Enough for
    /// the hand-built object-stream fixture below.
    fn ser(obj: &Object, out: &mut Vec<u8>) {
        match obj {
            Object::Null => out.extend_from_slice(b"null"),
            Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
            Object::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
            Object::Real(r) => out.extend_from_slice(r.to_string().as_bytes()),
            Object::Name(n) => {
                out.push(b'/');
                out.extend_from_slice(n);
            }
            Object::String(s, _) => {
                out.push(b'<');
                for b in s {
                    out.extend_from_slice(format!("{b:02x}").as_bytes());
                }
                out.push(b'>');
            }
            Object::Array(items) => {
                out.push(b'[');
                for i in items {
                    ser(i, out);
                    out.push(b' ');
                }
                out.push(b']');
            }
            Object::Dictionary(d) => {
                out.extend_from_slice(b"<<");
                for (k, v) in d.iter() {
                    out.push(b'/');
                    out.extend_from_slice(k);
                    out.push(b' ');
                    ser(v, out);
                    out.push(b' ');
                }
                out.extend_from_slice(b">>");
            }
            Object::Reference((id, gen)) => {
                out.extend_from_slice(format!("{id} {gen} R").as_bytes())
            }
            Object::Stream(s) => {
                let mut dict = s.dict.clone();
                dict.set("Length", s.content.len() as i64);
                ser(&Object::Dictionary(dict), out);
                out.extend_from_slice(b"\nstream\n");
                out.extend_from_slice(&s.content);
                out.extend_from_slice(b"\nendstream");
            }
        }
    }

    /// An RC4-encrypted, user-password PDF whose catalog, page tree and page
    /// live in an (encrypted) object stream behind an xref stream — the
    /// layout of most modern PDFs, which lopdf 0.34 could not read at all
    /// (finding F2). Built by hand: lopdf cannot write it.
    pub(crate) fn object_stream_encrypted_pdf(user: &str) -> Vec<u8> {
        let mut id_doc = Document::with_version("1.7");
        let file_id = Object::string_literal("fedcba9876543210");
        id_doc
            .trailer
            .set("ID", Object::Array(vec![file_id.clone(), file_id.clone()]));
        let state = EncryptionState::try_from(EncryptionVersion::V2 {
            document: &id_doc,
            owner_password: "owner",
            user_password: user,
            key_length: 128,
            permissions: Permissions::all(),
        })
        .unwrap();

        // 1 Catalog, 2 Pages, 3 Page: compressed in object stream 5.
        let mut objstm = lopdf::ObjectStream::builder().build();
        objstm
            .add_object(
                (1, 0),
                Object::Dictionary(dictionary! { "Type" => "Catalog", "Pages" => (2, 0) }),
            )
            .unwrap();
        objstm
            .add_object(
                (2, 0),
                Object::Dictionary(dictionary! {
                    "Type" => "Pages", "Kids" => vec![Object::Reference((3, 0))], "Count" => 1,
                }),
            )
            .unwrap();
        objstm
            .add_object(
                (3, 0),
                Object::Dictionary(dictionary! {
                    "Type" => "Page", "Parent" => (2, 0),
                    "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                    "Contents" => (4, 0),
                }),
            )
            .unwrap();
        let mut objstm = Object::Stream(objstm.to_stream_object().unwrap());
        lopdf::encryption::encrypt_object(&state, (5, 0), &mut objstm).unwrap();
        let mut content = Object::Stream(Stream::new(
            dictionary! {},
            b"0 0 1 rg 72 600 200 100 re f".to_vec(),
        ));
        lopdf::encryption::encrypt_object(&state, (4, 0), &mut content).unwrap();
        let encrypt = Object::Dictionary(state.encode().unwrap());

        let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (num, obj) in [(4u32, &content), (5, &objstm), (6, &encrypt)] {
            offsets.push((num, out.len()));
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            ser(obj, &mut out);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        // Entries 0..=7, W [1 4 2].
        let mut rows = vec![[0u8, 0, 0, 0, 0, 0xff, 0xff]];
        for index in 0..3u16 {
            let [i0, i1] = index.to_be_bytes();
            rows.push([2, 0, 0, 0, 5, i0, i1]);
        }
        for (_, off) in &offsets {
            let [a, b, c, d] = (*off as u32).to_be_bytes();
            rows.push([1, a, b, c, d, 0, 0]);
        }
        let [a, b, c, d] = (xref_at as u32).to_be_bytes();
        rows.push([1, a, b, c, d, 0, 0]);
        let xref = Object::Stream(Stream::new(
            dictionary! {
                "Type" => "XRef", "Size" => 8,
                "W" => vec![1.into(), 4.into(), 2.into()],
                "Root" => (1, 0), "Encrypt" => (6, 0),
                "ID" => vec![file_id.clone(), file_id],
            },
            rows.concat(),
        ));
        out.extend_from_slice(b"7 0 obj\n");
        ser(&xref, &mut out);
        out.extend_from_slice(format!("\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
        out
    }

    /// FR-059 (F2): encryption behind an xref stream and object streams is
    /// found by the fast path, needs the password, and decrypts to a copy
    /// with its page.
    #[test]
    fn fr_059_object_stream_encrypted_fixture() {
        let bytes = object_stream_encrypted_pdf("user");
        assert_eq!(inspect_bytes(&bytes).unwrap(), PdfSecurity::NeedsPassword);
        let dir = tempfile::tempdir().unwrap();
        let input = write(dir.path(), "objstm.pdf", &bytes);
        let out = dir.path().join("objstm.dec.pdf");
        decrypt_to(&input, &password("user"), &out).unwrap();
        let copy = Document::load(&out).unwrap();
        assert_eq!(copy.get_pages().len(), 1);
        assert!(!copy.is_encrypted());

        // Owner-only, same layout: printable in place.
        assert_eq!(
            inspect_bytes(&object_stream_encrypted_pdf("")).unwrap(),
            PdfSecurity::OwnerOnly { print: true }
        );
    }

    type Converted = Result<Option<std::path::PathBuf>, EngineError>;

    /// Run the whole PDF route (`convert_to_pdf`) on `bytes`.
    fn convert(bytes: &[u8], password: Option<&PdfPassword>) -> (tempfile::TempDir, Converted) {
        let dir = tempfile::tempdir().unwrap();
        let input = write(dir.path(), "in.pdf", bytes);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let result = super::super::convert_to_pdf(
            &input,
            &dir.path().join("cache"),
            "job",
            &cancel,
            &|_| {},
            None,
            &super::super::PageSetup::default(),
            password,
        );
        (dir, result)
    }

    /// FR-059: a user-password PDF is refused as `password_required` — never
    /// as "the document has no pages", which is how it loads without one.
    #[test]
    fn fr_059_user_password_pdf_refused_password_required() {
        for bytes in [
            QUARTZ_USER.to_vec(),
            encrypted_pdf(Scheme::Aes256, "user", Permissions::all()),
            object_stream_encrypted_pdf("user"),
        ] {
            let (_dir, result) = convert(&bytes, None);
            let err = result.unwrap_err();
            assert!(
                matches!(
                    err,
                    EngineError::PdfProtected(PdfProtection::PasswordRequired)
                ),
                "{err:?}"
            );
        }
    }

    /// FR-059: an owner-only PDF that allows printing is printed in place,
    /// like any PDF (the spoolers open it with the empty password).
    #[test]
    fn fr_059_owner_only_printable_passes() {
        for bytes in [
            QUARTZ_OWNER.to_vec(),
            encrypted_pdf(Scheme::Rc4, "", Permissions::all()),
        ] {
            let (_dir, result) = convert(&bytes, None);
            assert_eq!(result.unwrap(), None, "printed in place");
        }
    }

    /// FR-059 / D12: a PDF whose permissions forbid printing is refused
    /// before it reaches the spooler, and the bit is never stripped.
    #[test]
    fn fr_059_no_print_bit_refused_printing_not_allowed() {
        for bytes in [
            QUARTZ_NOPRINT.to_vec(),
            encrypted_pdf(Scheme::Rc4, "", no_print()),
        ] {
            let (_dir, result) = convert(&bytes, None);
            let err = result.unwrap_err();
            assert!(
                matches!(
                    err,
                    EngineError::PdfProtected(PdfProtection::PrintingNotAllowed)
                ),
                "{err:?}"
            );
        }
    }

    /// FR-059: with the job's password the route prints a decrypted cache
    /// copy; with a wrong one it asks again and leaves nothing behind.
    #[test]
    fn fr_059_password_decrypts_to_the_cache_pdf() {
        let (dir, result) = convert(QUARTZ_USER, Some(&password(QUARTZ_PASSWORD)));
        let pdf = result.unwrap().expect("a decrypted cache copy");
        assert!(pdf.starts_with(dir.path().join("cache")));
        let copy = Document::load(&pdf).unwrap();
        assert!(!copy.is_encrypted());
        assert_eq!(copy.get_pages().len(), 2);

        let (dir, result) = convert(QUARTZ_USER, Some(&password("nope")));
        assert!(matches!(
            result.unwrap_err(),
            EngineError::PdfProtected(PdfProtection::PasswordIncorrect)
        ));
        assert!(
            !dir.path().join("cache").join("job.pdf").exists(),
            "no half-written copy"
        );

        // A password given for a plain PDF is ignored.
        let mut plain = Vec::new();
        plain_doc(1).save_to(&mut plain).unwrap();
        let (_dir, result) = convert(&plain, Some(&password("unused")));
        assert_eq!(result.unwrap(), None);
    }
}
