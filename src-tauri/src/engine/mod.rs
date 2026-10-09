//! Print engine: format router, converters (everything → PDF) and per-OS
//! printer adapters (see `docs/04-architecture/print-engine.md` and ADR-0003).
//!
//! The engine has three pure-ish concerns, one per submodule:
//!
//! - [`pagerange`] — parse/validate `"1-5,10"` page-range expressions.
//! - folder expansion ([`expand_folder`]) — turn a dropped/selected directory
//!   into the list of printable files inside it (FR-004/FR-007), keeping the
//!   discarded ones with a reason (FR-005/FR-059).
//! - [`convert`] — turn a supported input file into a printable PDF (PDFs pass
//!   through; images are rasterised in-process; HEIC goes through the OS
//!   decoder; documents through LibreOffice, a headless browser or Pandoc).
//! - [`print`] — translate [`crate::queue::job::JobSettings`] into the
//!   SumatraPDF (Windows) / CUPS `lp` (Unix) invocation and spool it.
//! - [`deps`] — locate the external programs the pipeline shells out to
//!   (SumatraPDF and the documented converters) and say what a missing one
//!   costs the user (FR-058).
//!
//! Everything is small and testable: the flag builders and parsers are pure
//! functions with co-located unit tests; only the `spool`/`cancel` entry points
//! actually spawn a process, and those are `cfg`-gated per platform.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use walkdir::WalkDir;

pub mod convert;
pub mod deps;
pub mod pagerange;
pub mod print;

/// File extensions the engine can turn into a printable PDF, grouped by
/// converter (see [`convert::route`]): PDF passthrough; raster images (the
/// in-process `image` codecs); HEIC/HEIF (the OS decoder); Office, OpenDocument,
/// TXT and RTF (LibreOffice); HTML (a Chromium-class browser); Markdown (Pandoc,
/// then the browser). A format whose converter is not installed is accepted:
/// its job pauses with the FR-058 remediation instead of being refused at drop
/// time. The UI mirrors this list in `src/lib/formats.ts` (a test diffs them).
pub const SUPPORTED_EXTS: &[&str] = &[
    "pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp", "heic", "heif", "docx", "doc",
    "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp", "rtf", "txt", "html", "htm", "md",
    "markdown",
];

/// Whether `ext` (with or without a leading dot, any case) names a format the
/// engine can print.
pub fn is_supported(ext: &str) -> bool {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    SUPPORTED_EXTS.contains(&ext.as_str())
}

/// The lowercased extension of `path` without the leading dot, or an empty
/// string when the path has none. Shared by the router and callers that need to
/// classify a file the same way the engine does.
pub fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Folder expansion (FR-004 / FR-005 / FR-007 / FR-059)
// ---------------------------------------------------------------------------

/// How deep a folder selection is walked (FR-004: the user is asked, the
/// backend never guesses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderScan {
    /// Only the files directly inside the folder; subfolders are reported but
    /// not entered.
    TopLevel,
    /// The whole subtree, down to [`ExpansionLimits::max_depth`].
    Recursive,
}

/// Safety ceiling on one folder expansion. Dropping a disk root must bound the
/// walk rather than wedge the app, so both the number of entries considered and
/// the depth are capped; whichever bites is reported back to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct ExpansionLimits {
    /// Most directory entries a single expansion may look at.
    pub max_files: u32,
    /// Deepest level a recursive walk descends to, relative to the root (`1` =
    /// the files directly inside it).
    pub max_depth: u32,
}

/// Default entry ceiling: comfortably above any real print batch, far below a
/// disk root.
pub const MAX_FOLDER_FILES: u32 = 5_000;

/// Default depth ceiling for a recursive walk. Deeper trees are truncated with
/// [`FolderExpansion::depth_truncated`] set.
pub const MAX_FOLDER_DEPTH: u32 = 16;

impl Default for ExpansionLimits {
    fn default() -> Self {
        Self {
            max_files: MAX_FOLDER_FILES,
            max_depth: MAX_FOLDER_DEPTH,
        }
    }
}

/// Why a file found inside a folder did not make it into the print selection.
/// FR-005/FR-059: a discarded file is never dropped silently, it comes back
/// with the reason so the caller can show it and write it to the job log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The extension is not one [`is_supported`] accepts.
    UnsupportedFormat,
    /// Bigger than the configured `max_file_size_mb` ceiling.
    TooLarge,
    /// Could not be `stat`ed or walked (permissions, broken or looping
    /// symlink, disappeared mid-walk).
    Unreadable,
}

/// One discarded entry, with a human explanation ready for the job log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SkippedFile {
    pub path: String,
    /// File name, or the whole path when it has none.
    pub name: String,
    pub reason: SkipReason,
    /// User-facing detail (`"Unsupported file type (.docx)"`).
    pub detail: String,
}

/// The result of walking one folder: what will print, what will not and why,
/// plus whether a safety ceiling cut the walk short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FolderExpansion {
    /// The folder that was walked (already canonical when it came through IPC).
    pub root: String,
    /// How deep the walk went.
    pub scan: FolderScan,
    /// Printable files, sorted by full path so the print order is stable.
    pub files: Vec<String>,
    /// Everything discarded, same ordering rule.
    pub skipped: Vec<SkippedFile>,
    /// The entry ceiling was reached; the folder holds more than this.
    pub files_truncated: bool,
    /// Non-empty folders existed below the depth that was walked — expected in
    /// [`FolderScan::TopLevel`], a truncation warning in
    /// [`FolderScan::Recursive`].
    pub depth_truncated: bool,
    /// The ceilings that were applied, so the UI can phrase the warning without
    /// hardcoding them.
    pub limits: ExpansionLimits,
}

impl FolderExpansion {
    /// An empty result for `root`, used as the base of a walk and when the root
    /// itself cannot be read.
    fn empty(root: &Path, scan: FolderScan, limits: ExpansionLimits) -> Self {
        Self {
            root: root.to_string_lossy().into_owned(),
            scan,
            files: Vec::new(),
            skipped: Vec::new(),
            files_truncated: false,
            depth_truncated: false,
            limits,
        }
    }
}

/// Walk `root` and split its contents into printable files and discarded ones.
///
/// Never fails: an unreadable root or entry becomes a [`SkipReason::Unreadable`]
/// row, because a folder drop must degrade into "here is what I could use"
/// rather than an error dialog.
///
/// Symlinks are followed (a symlinked documents folder is a legitimate
/// selection), which walkdir does with loop detection — a link that points at
/// one of its own ancestors comes back as an error entry and is reported as
/// unreadable instead of spinning forever.
///
/// Ordering is deterministic twice over: the walk is sorted by file name (so
/// which entries the ceiling cuts is stable) and the two output lists are then
/// sorted by full path (so the print order is stable).
pub fn expand_folder(
    root: &Path,
    scan: FolderScan,
    limits: ExpansionLimits,
    max_file_size_mb: u32,
) -> FolderExpansion {
    let mut out = FolderExpansion::empty(root, scan, limits);
    let max_depth = match scan {
        FolderScan::TopLevel => 1,
        FolderScan::Recursive => limits.max_depth.max(1) as usize,
    };
    let max_entries = limits.max_files as usize;

    let walker = WalkDir::new(root)
        .min_depth(1)
        .max_depth(max_depth)
        .follow_links(true)
        .sort_by_file_name();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                out.skipped.push(skipped_from_walk_error(&err));
                continue;
            }
        };

        if entry.file_type().is_dir() {
            // A directory sitting *at* the ceiling is one we will not enter:
            // say so, but only when it actually holds something.
            if entry.depth() >= max_depth && has_entries(entry.path()) {
                out.depth_truncated = true;
            }
            continue;
        }

        if out.files.len() + out.skipped.len() >= max_entries {
            out.files_truncated = true;
            break;
        }

        match classify_entry(entry.path(), max_file_size_mb) {
            Ok(path) => out.files.push(path.to_string_lossy().into_owned()),
            Err(skipped) => out.skipped.push(skipped),
        }
    }

    out.files.sort();
    out.skipped.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Accept one file, or explain why not. Format is checked before size so a
/// folder full of huge videos reads as "unsupported file type" rather than the
/// misleading "too large"; the file-list surface
/// (`crate::ipc::jobs::inspect_files`) only ever sees files that passed both,
/// so the two orderings can never contradict each other in the UI.
fn classify_entry(path: &Path, max_file_size_mb: u32) -> Result<PathBuf, SkippedFile> {
    let ext = extension_of(path);
    if !is_supported(&ext) {
        let detail = if ext.is_empty() {
            "Unsupported file type (no extension)".to_string()
        } else {
            format!("Unsupported file type (.{ext})")
        };
        return Err(skipped(path, SkipReason::UnsupportedFormat, detail));
    }
    match std::fs::metadata(path) {
        Err(e) => Err(skipped(
            path,
            SkipReason::Unreadable,
            format!("Could not be read ({e})"),
        )),
        Ok(m) if crate::queue::exceeds_size_limit(m.len(), max_file_size_mb) => Err(skipped(
            path,
            SkipReason::TooLarge,
            format!("Larger than the {max_file_size_mb} MB limit"),
        )),
        Ok(_) => Ok(path.to_path_buf()),
    }
}

/// Build a [`SkippedFile`] for `path`, deriving the display name the same way
/// the rest of the app does.
fn skipped(path: &Path, reason: SkipReason, detail: String) -> SkippedFile {
    let full = path.to_string_lossy().into_owned();
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| full.clone());
    SkippedFile {
        path: full,
        name,
        reason,
        detail,
    }
}

/// Turn a walkdir error into a reported skip. A symlink loop is called out by
/// name: it is the one case where the *link* is fine and only the cycle is the
/// problem, and users deserve to know which one bit them.
fn skipped_from_walk_error(err: &walkdir::Error) -> SkippedFile {
    let path = err.path().unwrap_or_else(|| Path::new("")).to_path_buf();
    let detail = match err.loop_ancestor() {
        Some(ancestor) => format!(
            "Skipped: symbolic link loops back to {}",
            ancestor.display()
        ),
        None => format!("Could not be read ({err})"),
    };
    skipped(&path, SkipReason::Unreadable, detail)
}

/// Whether `dir` holds at least one entry. Used only to avoid reporting an
/// empty directory at the depth ceiling as truncated content; an unreadable
/// directory answers `false` rather than raising a second error.
fn has_entries(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

/// Every way the engine can fail. Messages are user-facing: they surface in the
/// queue UI and the job log, so they are written for a person, not a developer.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// A converter could not turn the input into a valid PDF (bad/corrupt input,
    /// unreadable file, unsupported image sub-format such as CMYK JPEG).
    #[error("conversion failed: {0}")]
    ConversionFailed(String),

    /// The OS spooler/printer backend rejected the job.
    #[error("printing failed: {0}")]
    PrintFailed(String),

    /// A requested setting the driver cannot honour and that would otherwise
    /// silently degrade the output (currently only duplex — see
    /// [`print::check_supported`]).
    #[error("the driver for '{printer}' does not support {setting}")]
    UnsupportedSetting { setting: String, printer: String },

    /// A required external program (CUPS `lp`, SumatraPDF, LibreOffice, a
    /// Chromium-class browser, Pandoc, the HEIF codec) is not installed or
    /// could not be located. The message is the FR-058 remediation text.
    #[error("missing dependency: {0}")]
    DependencyMissing(String),

    /// The file's format is not one the engine can print at all.
    #[error("unsupported file format: {0}")]
    UnsupportedFormat(String),

    /// The job's cancellation flag was set; work stopped at a stage boundary.
    #[error("job cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn is_supported_is_case_insensitive_and_dot_tolerant() {
        assert!(is_supported("pdf"));
        assert!(is_supported("PDF"));
        assert!(is_supported("JpEg"));
        assert!(is_supported(".png"));
        assert!(is_supported("TIFF"));
        assert!(is_supported("DOCX"));
        assert!(is_supported(".md"));
        assert!(!is_supported("gif")); // decodable by `image` but not enabled
        assert!(!is_supported("exe"));
        assert!(!is_supported(""));
    }

    #[test]
    fn fr_052_to_057_supported_exts_are_the_expected_set() {
        assert_eq!(
            SUPPORTED_EXTS,
            &[
                "pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "webp", "heic", "heif", "docx",
                "doc", "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp", "rtf", "txt", "html",
                "htm", "md", "markdown",
            ]
        );
    }

    #[test]
    fn extension_of_lowercases_and_handles_missing() {
        assert_eq!(extension_of(Path::new("/a/b/report.PDF")), "pdf");
        assert_eq!(extension_of(Path::new("/a/b/photo.JPeG")), "jpeg");
        assert_eq!(extension_of(Path::new("/a/b/Makefile")), "");
    }
}
