//! Plain text → PDF (FR-057), with no LibreOffice.
//!
//! The file is decoded ([`decode`]): a byte-order mark first (UTF-8, UTF-16
//! LE/BE), then strict UTF-8, else Windows-1252 — what a Western Windows
//! machine writes when it does not write UTF-8. The text is HTML-escaped into
//! a `<pre>` page in a monospaced font that wraps long lines
//! ([`to_html`]), and that page goes through the page renderer of
//! [`super::html`] (the system web view, no network, no script), laid out on
//! the job's paper.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::{PageSetup, WorkDir};
use crate::engine::{EngineError, Failure};

/// Extensions this converter handles.
pub const TEXT_EXTS: &[&str] = &["txt"];

/// The text in `bytes` (module docs). Never fails: an undecodable byte
/// becomes U+FFFD. A NUL character is dropped. Pure.
pub fn decode(bytes: &[u8]) -> String {
    let text = if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        encoding.decode_without_bom_handling(&bytes[bom..]).0
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        std::borrow::Cow::Borrowed(text)
    } else {
        encoding_rs::WINDOWS_1252
            .decode_without_bom_handling(bytes)
            .0
    };
    text.replace('\0', "")
}

/// Whether `bytes` look like a binary file rather than text: a NUL byte in
/// the first 8 KiB of a file without a UTF-16 byte-order mark. Pure.
pub fn looks_binary(bytes: &[u8]) -> bool {
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    !utf16 && bytes.iter().take(8192).any(|&b| b == 0)
}

/// The HTML page for `text`, titled `title`. Pure.
pub fn to_html(text: &str, title: &str) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    // Windows line endings render as one line break, as do old Mac ones.
    let body = text.replace("\r\n", "\n").replace('\r', "\n");
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>{}</title>\n\
         <style>\nhtml {{ color: #000; }} body {{ margin: 0; }}\n\
         pre {{ font-family: Consolas, 'SF Mono', Menlo, 'Courier New', monospace; \
         font-size: 10pt; line-height: 1.35; white-space: pre-wrap; \
         overflow-wrap: anywhere; margin: 0; tab-size: 8; }}\n</style>\n\
         </head><body><pre>{}</pre></body></html>\n",
        escape(title),
        escape(&body)
    )
}

/// Convert the text file at `input` into a PDF at `output`, laid out on the
/// job's paper (`page`).
pub fn convert(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
) -> Result<(), EngineError> {
    convert_with_renderer(
        input,
        output,
        work,
        cancel,
        timeout,
        page,
        super::html::configured_renderer(),
    )
}

/// [`convert`] with an explicit renderer.
pub fn convert_with_renderer(
    input: &Path,
    output: &Path,
    work: &WorkDir,
    cancel: &AtomicBool,
    timeout: Option<Duration>,
    page: &PageSetup,
    renderer: crate::settings::HtmlRenderer,
) -> Result<(), EngineError> {
    let bytes = std::fs::read(input).map_err(|e| {
        EngineError::ConversionFailed(
            Failure::new(
                "cannot_read_input",
                format!("cannot read {}: {e}", input.display()),
            )
            .param("file", input.display().to_string()),
        )
    })?;
    if looks_binary(&bytes) {
        return Err(EngineError::ConversionFailed(Failure::new(
            "text_unreadable",
            format!("{} is not a text file", input.display()),
        )));
    }
    let title = input
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "text".to_string());
    let html = to_html(&decode(&bytes), &title);
    let base_dir = input.parent().unwrap_or_else(|| Path::new("."));
    super::html::render_document(
        html.as_bytes(),
        base_dir,
        output,
        work,
        cancel,
        timeout,
        page,
        renderer,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_057_txt_utf16_bom_and_cp1252() {
        // UTF-16 LE and BE with a BOM.
        let mut le = vec![0xFF, 0xFE];
        let mut be = vec![0xFE, 0xFF];
        for unit in "Ñandú café".encode_utf16() {
            le.extend_from_slice(&unit.to_le_bytes());
            be.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(decode(&le), "Ñandú café");
        assert_eq!(decode(&be), "Ñandú café");
        // UTF-8 with and without a BOM.
        assert_eq!(decode("\u{FEFF}año".as_bytes()), "año");
        assert_eq!(decode("año €".as_bytes()), "año €");
        // Not UTF-8: Windows-1252 (0x80 is the euro sign there).
        assert_eq!(decode(b"a\xf1o \x80 caf\xe9"), "año € café");
    }

    #[test]
    fn fr_057_text_is_escaped_into_a_wrapping_pre() {
        let html = to_html(
            "<script>x</script> & \"q\"\r\nline2\rline3",
            "notes <1>.txt",
        );
        assert!(
            html.contains(
                "<pre>&lt;script&gt;x&lt;/script&gt; &amp; &quot;q&quot;\nline2\nline3</pre>"
            ),
            "{html}"
        );
        assert!(html.contains("<title>notes &lt;1&gt;.txt</title>"));
        assert!(html.contains("white-space: pre-wrap"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn fr_057_a_binary_file_is_not_printed_as_text() {
        assert!(looks_binary(b"PK\x03\x04\0\0binary"));
        assert!(!looks_binary(b"plain text"));
        let mut le = vec![0xFF, 0xFE];
        le.extend_from_slice(&[b'a', 0, b'b', 0]);
        assert!(!looks_binary(&le), "UTF-16 has NULs and is text");
        assert_eq!(decode(b"a\0b"), "ab");
    }

    #[test]
    fn fr_057_a_binary_txt_fails_with_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("x.txt");
        std::fs::write(&input, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        let work = WorkDir::new("txt-bin").unwrap();
        let err = convert_with_renderer(
            &input,
            &dir.path().join("o.pdf"),
            &work,
            &AtomicBool::new(false),
            None,
            &PageSetup::default(),
            crate::settings::HtmlRenderer::System,
        )
        .unwrap_err();
        assert!(
            matches!(&err, EngineError::ConversionFailed(f) if f.reason == Some("text_unreadable")),
            "{err:?}"
        );
    }
}
