//! Encrypted-PDF inspection and decryption (FR-059).
//!
//! **Stub (round-3 contract, owned by lane L3).** The final [`inspect`] reads
//! the trailer for `/Encrypt` (no full parse for a plain PDF), then asks
//! `lopdf` whether the file needs a user password or only carries an owner
//! password, and whether its permissions allow printing. [`decrypt_to`] opens
//! the file with the user's password and writes a decrypted copy. Until L3
//! lands, every PDF inspects as [`PdfSecurity::Plain`] — today's behaviour.
//!
//! The no-print permission is always honoured: a PDF that forbids printing is
//! refused, never stripped.

use std::path::Path;

use crate::engine::{EngineError, PdfPassword};

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

/// Inspect the PDF at `path` (`size` bytes) for encryption.
pub fn inspect(path: &Path, size: u64) -> Result<PdfSecurity, EngineError> {
    let _ = (path, size);
    Ok(PdfSecurity::Plain)
}

/// Open the PDF at `input` with `password` and write a decrypted copy to
/// `output`. A wrong password is
/// [`EngineError::PdfProtected`]`(`[`crate::engine::PdfProtection::PasswordIncorrect`]`)`;
/// a file whose permissions forbid printing is `PrintingNotAllowed`.
pub fn decrypt_to(input: &Path, password: &PdfPassword, output: &Path) -> Result<(), EngineError> {
    let _ = (input, password, output);
    Err(EngineError::ConversionFailed(
        "PDF decryption is not implemented yet".into(),
    ))
}
