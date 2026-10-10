//! Windows print quality through a temporary per-user DEVMODE (decision D16,
//! `docs/03-requirements/print-settings.md`).
//!
//! SumatraPDF has no print-quality switch: it starts every job from the
//! printer's **per-user default DEVMODE** (`DocumentProperties` with no input)
//! and only layers its own `-print-settings` on top. So for a Draft or High
//! job (never for Normal, which is "whatever the driver does"):
//!
//! 1. a DEVMODE carrying the quality is built from the user's current default
//!    through a PrintTicket (`psk:PageOutputQuality`), which is how v4 and
//!    class drivers (the Microsoft IPP Class Driver `add_printer` installs)
//!    take it; a driver with no PrintTicket provider falls back to the public
//!    `dmPrintQuality` (`DMRES_DRAFT`/`DMRES_HIGH`), and a driver that offers
//!    neither is left alone — the job then says the quality was not applied;
//! 2. under a per-printer lock and a **journal** written before anything
//!    changes, that DEVMODE becomes the user's default (`SetPrinter` level 9,
//!    no administrator needed, other users untouched);
//! 3. as soon as SumatraPDF's job shows up in the queue (it has made its own
//!    copy of the DEVMODE by then) the original is put back — byte for byte,
//!    including "the user had no per-user default at all";
//! 4. the job's own DEVMODE (`JOB_INFO_2.pDevMode`) is read back to say what
//!    it really got ([`QualityOutcome`]).
//!
//! # Never clobber the user
//!
//! The restore only writes when the default is still exactly what this swap
//! left there ([`restore_action`]). If the user, or another program, changed
//! it in the meantime, that change wins and the swap only logs.
//!
//! # Crashes
//!
//! The journal (`<data>\devmode-swap\devmode-swap-<hash>.json`) holds the
//! original DEVMODE before the default is touched, and is removed only after
//! the restore. A process killed in between leaves it behind, and the next
//! swap on that printer — or the next start of the app or the CLI
//! ([`recover_stale_journals`]) — restores from it under the same rules.
//!
//! The lock is an OS file lock on `devmode-swap-<hash>.lock` rather than a
//! named mutex: it serializes the app, the CLI and the shell-extension
//! launches the same way, is released by the OS when a process dies (the
//! "abandoned mutex" case needs no special handling) and behaves the same on
//! every platform, so the swap logic below is tested on every CI lane against
//! an in-memory store ([`UserDefaults`]). Only the spooler calls are Windows
//! FFI.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::queue::job::Quality;

/// The env var that makes [`Swap::begin`] exit the process right after the
/// default was changed (debug builds only): the crash-recovery tests use it to
/// leave a real journal behind.
pub const ABORT_AFTER_SWAP_ENV: &str = "PLURYPRINT_TEST_ABORT_AFTER_SWAP";

/// The exit code of that simulated crash.
pub const ABORT_AFTER_SWAP_EXIT: i32 = 86;

/// How long a job waits for another Pluryprint process to finish its swap on
/// the same printer before printing without one.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(60);

/// A quality the swap can carry. [`Quality::Normal`] has none: it is the
/// driver's own default and is never forced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// `psk:Draft` / `DMRES_DRAFT`.
    Draft,
    /// `psk:High` / `DMRES_HIGH`.
    High,
}

impl Target {
    /// The swap target of a job's quality, or `None` for Normal.
    pub fn for_quality(quality: Quality) -> Option<Self> {
        match quality {
            Quality::Draft => Some(Self::Draft),
            Quality::High => Some(Self::High),
            Quality::Normal => None,
        }
    }

    /// The Print Schema keyword (local name in the `psk` namespace).
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::High => "High",
        }
    }

    /// The `dmPrintQuality` value (`DMRES_DRAFT`, `DMRES_HIGH`).
    pub fn dmres(self) -> i16 {
        match self {
            Self::Draft => -1,
            Self::High => -4,
        }
    }

    /// The word the profile uses, for messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::High => "high",
        }
    }
}

/// What a printer's driver says about a quality before printing — what the
/// honesty preview tells the user ([`super::sumatra::quality_note`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualitySupport {
    /// The driver offers it: the swap will be made.
    Offered,
    /// The driver offers neither the PrintTicket option nor `dmPrintQuality`:
    /// nothing is changed and the quality is not applied.
    NotOffered,
    /// The driver could not be asked (off Windows, or the query failed): the
    /// swap is tried and the job checked afterwards.
    Unknown,
}

/// What a spooled job really carried, read back from its own DEVMODE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualityOutcome {
    /// The job's settings carry the requested quality.
    Applied,
    /// They do not: the detail says what was found, or why nothing was sent.
    NotApplied(String),
    /// It could not be checked (the job already left the queue, or its
    /// settings could not be read); the detail says why.
    Unverified(String),
}

// ---------------------------------------------------------------------------
// DEVMODEW, read as bytes (the layout is the Win32 ABI), so the helpers are
// pure and tested everywhere.
// ---------------------------------------------------------------------------

const DM_SIZE_OFFSET: usize = 68;
const DM_DRIVER_EXTRA_OFFSET: usize = 70;
const DM_FIELDS_OFFSET: usize = 72;
const DM_PRINT_QUALITY_OFFSET: usize = 90;
/// `DM_PRINTQUALITY` in `dmFields`.
pub const DM_PRINTQUALITY: u32 = 0x0400;
/// `DM_PAPERSIZE` in `dmFields`.
const DM_PAPERSIZE: u32 = 0x0002;
/// `DM_DEFAULTSOURCE` in `dmFields`.
const DM_DEFAULTSOURCE: u32 = 0x0200;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// `dmSize + dmDriverExtra`: the whole DEVMODE, public and private parts.
pub fn devmode_len(dm: &[u8]) -> Option<usize> {
    let size = usize::from(u16_at(dm, DM_SIZE_OFFSET)?);
    let extra = usize::from(u16_at(dm, DM_DRIVER_EXTRA_OFFSET)?);
    (size > DM_PRINT_QUALITY_OFFSET && size + extra <= dm.len()).then_some(size + extra)
}

/// `dmPrintQuality` when `dmFields` says it is set.
pub fn print_quality(dm: &[u8]) -> Option<i16> {
    devmode_len(dm)?;
    if u32_at(dm, DM_FIELDS_OFFSET)? & DM_PRINTQUALITY == 0 {
        return None;
    }
    Some(u16_at(dm, DM_PRINT_QUALITY_OFFSET)? as i16)
}

/// `dmPaperSize` (a `DMPAPER_*` id) when `dmFields` says it is set.
pub fn paper_size_field(dm: &[u8]) -> Option<u16> {
    devmode_len(dm)?;
    (u32_at(dm, DM_FIELDS_OFFSET)? & DM_PAPERSIZE != 0).then(|| u16_at(dm, 78))?
}

/// `dmDefaultSource` (a `DMBIN_*` id) when `dmFields` says it is set.
pub fn default_source_field(dm: &[u8]) -> Option<u16> {
    devmode_len(dm)?;
    (u32_at(dm, DM_FIELDS_OFFSET)? & DM_DEFAULTSOURCE != 0).then(|| u16_at(dm, 88))?
}

/// `dm` with `dmPrintQuality = value` and `DM_PRINTQUALITY` set, or `None`
/// when `dm` is not a DEVMODE.
pub fn with_print_quality(dm: &[u8], value: i16) -> Option<Vec<u8>> {
    let len = devmode_len(dm)?;
    let mut out = dm[..len].to_vec();
    let fields = u32_at(&out, DM_FIELDS_OFFSET)? | DM_PRINTQUALITY;
    out[DM_FIELDS_OFFSET..DM_FIELDS_OFFSET + 4].copy_from_slice(&fields.to_le_bytes());
    out[DM_PRINT_QUALITY_OFFSET..DM_PRINT_QUALITY_OFFSET + 2].copy_from_slice(&value.to_le_bytes());
    Some(out)
}

// ---------------------------------------------------------------------------
// Print Schema XML: just enough reading to find one feature's options. The
// documents come from the local PrintTicket provider; a document this cannot
// read simply has no options, which reads as "not offered".
// ---------------------------------------------------------------------------

/// The Print Schema keywords namespace (`psk:`).
const PSK_NS: &str = "http://schemas.microsoft.com/windows/2003/08/printing/printschemakeywords";
/// The Print Schema framework namespace (`psf:`).
const PSF_NS: &str = "http://schemas.microsoft.com/windows/2003/08/printing/printschemaframework";

/// One XML tag: `(qualified name, raw attributes, closing, self-closing)`.
fn tags(xml: &str) -> impl Iterator<Item = (&str, &str, bool, bool)> {
    xml.split('<').skip(1).filter_map(|chunk| {
        let body = &chunk[..chunk.find('>')?];
        if body.starts_with('?') || body.starts_with('!') {
            return None;
        }
        let closing = body.starts_with('/');
        let body = body.trim_start_matches('/');
        let self_closing = body.ends_with('/');
        let body = body.trim_end_matches('/');
        let (name, attrs) = body
            .split_once(|c: char| c.is_whitespace())
            .unwrap_or((body, ""));
        Some((name, attrs, closing, self_closing))
    })
}

/// The value of attribute `name` in a tag's raw attribute text.
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attrs;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let end = after[1..].find(quote)? + 1;
        if key == name {
            return Some(&after[1..end]);
        }
        rest = &after[end + 1..];
    }
    None
}

/// The prefix bound to namespace `ns` in the document (`psk`, usually).
fn prefix_for<'a>(xml: &'a str, ns: &str, fallback: &'a str) -> &'a str {
    for (_, attrs, closing, _) in tags(xml) {
        if closing {
            continue;
        }
        let mut rest = attrs;
        while let Some(pos) = rest.find("xmlns:") {
            let tail = &rest[pos + 6..];
            let Some(eq) = tail.find('=') else { break };
            let prefix = tail[..eq].trim();
            // `attr` reads `key="value"`: give it this one declaration.
            let view = format!("{prefix}{}", &tail[eq..]);
            if attr(&view, prefix) == Some(ns) {
                return prefix;
            }
            rest = &tail[eq..];
        }
    }
    fallback
}

/// The option names (qualified) directly inside the feature whose qualified
/// name is `feature`, in document order.
fn feature_options(xml: &str, feature: &str) -> Vec<String> {
    let psf = prefix_for(xml, PSF_NS, "psf");
    let feature_tag = format!("{psf}:Feature");
    let option_tag = format!("{psf}:Option");
    let mut out = Vec::new();
    // Depth of nested Feature elements inside the one we want (0 = outside).
    let mut depth = 0usize;
    for (name, attrs, closing, self_closing) in tags(xml) {
        if name == feature_tag {
            if closing {
                depth = depth.saturating_sub(1);
            } else if depth > 0 {
                if !self_closing {
                    depth += 1;
                }
            } else if attr(attrs, "name") == Some(feature) && !self_closing {
                depth = 1;
            }
        } else if depth == 1 && name == option_tag && !closing {
            if let Some(option) = attr(attrs, "name") {
                out.push(option.to_string());
            }
        }
    }
    out
}

/// The `PageOutputQuality` options a document lists, as keyword local names
/// (`Draft`, `Normal`, `High`…). Options in a private namespace keep their
/// prefix, so they never pass for a standard keyword.
pub fn quality_options(xml: &str) -> Vec<String> {
    let psk = prefix_for(xml, PSK_NS, "psk");
    let feature = format!("{psk}:PageOutputQuality");
    feature_options(xml, &feature)
        .into_iter()
        .map(|o| match o.split_once(':') {
            Some((p, local)) if p == psk => local.to_string(),
            _ => o,
        })
        .collect()
}

/// Whether a PrintCapabilities document offers `target`.
pub fn offers(capabilities_xml: &str, target: Target) -> bool {
    quality_options(capabilities_xml)
        .iter()
        .any(|o| o == target.keyword())
}

/// The quality a PrintTicket selects, as a keyword local name.
pub fn selected_quality(ticket_xml: &str) -> Option<String> {
    quality_options(ticket_xml).into_iter().next()
}

/// A delta PrintTicket that only selects `target`.
pub fn delta_ticket(target: Target) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<psf:PrintTicket xmlns:psf=\"{PSF_NS}\" xmlns:psk=\"{PSK_NS}\" version=\"1\">\
<psf:Feature name=\"psk:PageOutputQuality\"><psf:Option name=\"psk:{}\"/></psf:Feature>\
</psf:PrintTicket>",
        target.keyword()
    )
}

/// What the job's DEVMODE, its PrintTicket (when one could be made from it)
/// and the request say together. Pure: [`job_quality`] feeds it on Windows.
pub fn judge_job(
    target: Target,
    ticket_quality: Option<&str>,
    dm_quality: Option<i16>,
) -> QualityOutcome {
    match ticket_quality {
        Some(q) if q == target.keyword() => QualityOutcome::Applied,
        Some(other) => QualityOutcome::NotApplied(format!(
            "the job's settings carry \"{other}\" instead of \"{}\"",
            target.keyword()
        )),
        None => match dm_quality {
            Some(v) if v == target.dmres() => QualityOutcome::Applied,
            Some(v) => QualityOutcome::NotApplied(format!(
                "the job's settings carry print quality {v} instead of {}",
                target.dmres()
            )),
            None => QualityOutcome::NotApplied(
                "the job's settings carry no print quality at all".to_string(),
            ),
        },
    }
}

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

/// What a swap in progress has changed, written before the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    /// Format version.
    pub version: u32,
    /// The printer whose per-user default was changed.
    pub printer: String,
    /// The user's DEVMODE before the swap (hex); `None` when the user had no
    /// per-user default, which is what is restored then.
    pub original: Option<String>,
    /// The DEVMODE the swap asked for (hex).
    pub intended: String,
    /// What the spooler reported back after the write (hex). The driver may
    /// normalize what it is given, so this — not `intended` — is what "still
    /// ours" is checked against. `None` until it was read.
    pub written: Option<String>,
    /// The process that made the swap.
    pub pid: u32,
    /// When (RFC 3339).
    pub started: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

impl Journal {
    fn original_bytes(&self) -> Result<Option<Vec<u8>>, String> {
        match &self.original {
            None => Ok(None),
            Some(h) => unhex(h)
                .map(Some)
                .ok_or_else(|| "the journal's original DEVMODE is corrupt".to_string()),
        }
    }
}

/// FNV-1a of the printer name, case-folded: printer names are
/// case-insensitive on Windows, and file names must not carry `\\server\`.
fn printer_key(printer: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in printer.to_lowercase().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The journal file of `printer` in `dir`.
pub fn journal_path(dir: &Path, printer: &str) -> PathBuf {
    dir.join(format!("devmode-swap-{}.json", printer_key(printer)))
}

fn lock_path(dir: &Path, printer: &str) -> PathBuf {
    dir.join(format!("devmode-swap-{}.lock", printer_key(printer)))
}

/// Write `journal` so that a crash leaves either the old file or the new one,
/// never half of one: a temporary file, flushed to disk, then renamed.
pub fn write_journal(path: &Path, journal: &Journal) -> std::io::Result<()> {
    use std::io::Write as _;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(journal).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Read a journal; `Ok(None)` when there is none.
pub fn read_journal(path: &Path) -> Result<Option<Journal>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("unreadable journal {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read journal {}: {e}", path.display())),
    }
}

/// The per-user default store the swap writes: `SetPrinter`/`GetPrinter`
/// level 9 on Windows ([`SpoolerDefaults`]), a map in the tests.
pub trait UserDefaults {
    /// The user's DEVMODE for `printer`, or `None` when they have none.
    fn get(&self, printer: &str) -> Result<Option<Vec<u8>>, String>;
    /// Set (or, with `None`, clear) the user's DEVMODE for `printer`.
    fn set(&self, printer: &str, devmode: Option<&[u8]>) -> Result<(), String>;
}

impl<T: UserDefaults + ?Sized> UserDefaults for &T {
    fn get(&self, printer: &str) -> Result<Option<Vec<u8>>, String> {
        (**self).get(printer)
    }
    fn set(&self, printer: &str, devmode: Option<&[u8]>) -> Result<(), String> {
        (**self).set(printer, devmode)
    }
}

/// What a restore does, given the default as it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreAction {
    /// The default is still the swap's: put the original back.
    Restore,
    /// The default is already the original (nothing was written, or someone
    /// already restored it).
    AlreadyOriginal,
    /// Somebody changed the default after the swap: their change stays.
    LeaveUserChange,
}

/// Decide the restore from the default as it is now and the journal. Pure.
pub fn restore_action(current: Option<&[u8]>, journal: &Journal) -> RestoreAction {
    let original = journal.original.as_deref().and_then(unhex);
    if current == original.as_deref() {
        return RestoreAction::AlreadyOriginal;
    }
    let ours = match &journal.written {
        Some(w) => unhex(w),
        // Killed between the write and its read-back: only an exact match of
        // what was asked for is known to be ours.
        None => unhex(&journal.intended),
    };
    if current.is_some() && current == ours.as_deref() {
        RestoreAction::Restore
    } else {
        RestoreAction::LeaveUserChange
    }
}

/// Undo the swap a journal records, if it is still ours, and remove the
/// journal. The caller holds the printer's lock. `Ok(None)` when there is no
/// journal. A journal whose restore failed is kept for the next attempt.
pub fn recover_journal<S: UserDefaults>(
    store: &S,
    path: &Path,
) -> Result<Option<RestoreAction>, String> {
    let journal = match read_journal(path) {
        Ok(Some(j)) => j,
        Ok(None) => return Ok(None),
        Err(e) => {
            // Nothing can be restored from it; keep a copy for diagnosis and
            // stop it from blocking every later swap.
            let _ = std::fs::rename(path, path.with_extension("json.bad"));
            return Err(e);
        }
    };
    let action = apply_restore(store, &journal)?;
    let _ = std::fs::remove_file(path);
    Ok(Some(action))
}

fn apply_restore<S: UserDefaults>(store: &S, journal: &Journal) -> Result<RestoreAction, String> {
    let current = store.get(&journal.printer)?;
    let action = restore_action(current.as_deref(), journal);
    match action {
        RestoreAction::Restore => {
            let original = journal.original_bytes()?;
            store.set(&journal.printer, original.as_deref())?;
            tracing::info!(event = "devmode_swap_restored", printer = %journal.printer);
        }
        RestoreAction::AlreadyOriginal => {}
        RestoreAction::LeaveUserChange => tracing::warn!(
            event = "devmode_swap_skipped_restore",
            printer = %journal.printer,
            reason = "the printer's default was changed after the swap; that change is kept",
        ),
    }
    Ok(action)
}

/// Take the per-printer lock, waiting up to `timeout`. `Ok(None)` when
/// another process still holds it.
fn lock_printer(dir: &Path, printer: &str, timeout: Duration) -> std::io::Result<Option<File>> {
    std::fs::create_dir_all(dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(dir, printer))?;
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Why a swap was not made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SwapError {
    /// Another Pluryprint process kept the printer's lock past the timeout.
    #[error("another Pluryprint job kept this printer's default busy")]
    Busy,
    /// The driver offers no such quality: nothing was changed.
    #[error("the driver offers no \"{0}\" quality")]
    NotOffered(&'static str),
    /// The journal or the lock could not be written; nothing was changed.
    #[error("the swap journal could not be written: {0}")]
    Journal(String),
    /// The spooler refused (nothing was changed, or it was undone).
    #[error("{0}")]
    Spooler(String),
}

/// A per-user default changed for one job. Restored by [`Swap::restore`], or
/// on drop as a last resort; a crash leaves its journal for
/// [`recover_journal`].
pub struct Swap<S: UserDefaults> {
    store: S,
    journal_path: PathBuf,
    journal: Journal,
    restored: Option<Result<RestoreAction, String>>,
    _lock: File,
}

impl<S: UserDefaults> Swap<S> {
    /// Lock the printer, finish any swap a crashed process left behind, then
    /// make the DEVMODE `build` returns (from the now-restored default) the
    /// user's default for `printer`. `build` returning `None` means the driver
    /// offers no such quality: nothing is changed.
    pub fn begin(
        store: S,
        dir: &Path,
        printer: &str,
        target: Target,
        lock_timeout: Duration,
        build: impl FnOnce() -> Result<Option<Vec<u8>>, String>,
    ) -> Result<Self, SwapError> {
        let lock = lock_printer(dir, printer, lock_timeout)
            .map_err(|e| SwapError::Journal(e.to_string()))?
            .ok_or(SwapError::Busy)?;
        let journal_path = journal_path(dir, printer);
        if let Err(e) = recover_journal(&store, &journal_path) {
            tracing::warn!(event = "devmode_swap_recover_failed", printer = %printer, error = %e);
        }
        let devmode = build()
            .map_err(SwapError::Spooler)?
            .ok_or(SwapError::NotOffered(target.label()))?;
        let original = store.get(printer).map_err(SwapError::Spooler)?;
        let mut journal = Journal {
            version: 1,
            printer: printer.to_string(),
            original: original.as_deref().map(hex),
            intended: hex(&devmode),
            written: None,
            pid: std::process::id(),
            started: crate::queue::job::now_rfc3339(),
        };
        write_journal(&journal_path, &journal).map_err(|e| SwapError::Journal(e.to_string()))?;
        if let Err(e) = store.set(printer, Some(&devmode)) {
            let _ = std::fs::remove_file(&journal_path);
            return Err(SwapError::Spooler(e));
        }
        // Best effort: without the read-back the exact bytes asked for are
        // what "still ours" is checked against.
        if let Ok(Some(written)) = store.get(printer) {
            journal.written = Some(hex(&written));
            if let Err(e) = write_journal(&journal_path, &journal) {
                tracing::warn!(event = "devmode_swap_journal_update_failed", error = %e);
            }
        }
        tracing::info!(event = "devmode_swap_begun", printer = %printer, quality = target.label());
        if cfg!(debug_assertions) && std::env::var(ABORT_AFTER_SWAP_ENV).as_deref() == Ok("1") {
            std::process::exit(ABORT_AFTER_SWAP_EXIT);
        }
        Ok(Self {
            store,
            journal_path,
            journal,
            restored: None,
            _lock: lock,
        })
    }

    /// Put the original default back if it is still the swap's (see
    /// [`restore_action`]) and remove the journal. Idempotent: later calls
    /// return the first result. A failed restore keeps the journal, so the
    /// next swap or start retries it.
    pub fn restore(&mut self) -> Result<RestoreAction, String> {
        if let Some(done) = &self.restored {
            return done.clone();
        }
        let result = apply_restore(&self.store, &self.journal);
        if result.is_ok() {
            let _ = std::fs::remove_file(&self.journal_path);
        }
        self.restored = Some(result.clone());
        result
    }

    /// Whether [`Swap::restore`] already ran.
    pub fn is_restored(&self) -> bool {
        self.restored.is_some()
    }

    /// Test stand-in for the process dying: nothing is restored, the journal
    /// stays, and the lock is released as the OS would release it.
    #[cfg(test)]
    fn crash(mut self) {
        self.restored = Some(Err("crashed".into()));
    }
}

impl<S: UserDefaults> Drop for Swap<S> {
    fn drop(&mut self) {
        if self.restored.is_none() {
            if let Err(e) = self.restore() {
                tracing::error!(event = "devmode_swap_restore_failed", error = %e);
            }
        }
    }
}

/// Finish every swap a crashed process left in `dir` whose printer is not
/// locked by a live one. Returns `(printer, result)` per journal found.
pub fn recover_stale_journals_in<S: UserDefaults>(
    store: &S,
    dir: &Path,
) -> Vec<(String, Result<Option<RestoreAction>, String>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("devmode-swap-") && name.ends_with(".json")) {
            continue;
        }
        let printer = match read_journal(&path) {
            Ok(Some(j)) => j.printer,
            Ok(None) => continue,
            Err(e) => {
                let _ = std::fs::rename(&path, path.with_extension("json.bad"));
                out.push((name, Err(e)));
                continue;
            }
        };
        // A live swap holds the lock; its own restore will run.
        match lock_printer(dir, &printer, Duration::ZERO) {
            Ok(Some(_lock)) => {
                let result = recover_journal(store, &path);
                out.push((printer, result));
            }
            Ok(None) => {}
            Err(e) => out.push((printer, Err(e.to_string()))),
        }
    }
    out
}

/// Where the journals and locks live: `<data>\devmode-swap`.
pub fn journal_dir() -> PathBuf {
    crate::util::paths::data_dir().join("devmode-swap")
}

// ---------------------------------------------------------------------------
// Windows: the spooler side.
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub use self::ffi::{
    build_job_devmode, job_devmode, job_quality, print_capabilities, quality_support,
    recover_stale_journals, recover_stale_journals_in_background, SpoolerDefaults,
};

#[cfg(windows)]
mod ffi {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, HANDLE, HGLOBAL, HWND,
    };
    use windows::Win32::Graphics::Gdi::{DEVMODEA, DEVMODEW, DM_IN_BUFFER, DM_OUT_BUFFER};
    use windows::Win32::Graphics::Printing::PrintTicket::{
        kPTJobScope, kUserDefaultDevmode, PTCloseProvider, PTConvertDevModeToPrintTicket,
        PTConvertPrintTicketToDevMode, PTGetPrintCapabilities, PTMergeAndValidatePrintTicket,
        PTOpenProvider, PTReleaseMemory, HPTPROVIDER,
    };
    use windows::Win32::Graphics::Printing::{
        ClosePrinter, DocumentPropertiesW, GetJobW, GetPrinterW, OpenPrinterW, SetPrinterW,
        JOB_INFO_2W, PRINTER_INFO_9W,
    };
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, IStream, COINIT_MULTITHREADED, STREAM_SEEK_SET,
    };

    use super::{
        devmode_len, judge_job, offers, print_quality, selected_quality, with_print_quality,
        QualityOutcome, QualitySupport, Target, UserDefaults,
    };

    /// 8-byte aligned scratch for spooler structs (they hold pointers).
    struct Aligned(Vec<u64>);

    impl Aligned {
        fn new(bytes: usize) -> Self {
            Self(vec![0u64; bytes.div_ceil(8).max(1)])
        }
        fn bytes_mut(&mut self) -> &mut [u8] {
            // SAFETY: a u64 buffer viewed as bytes, same length in bytes.
            unsafe {
                std::slice::from_raw_parts_mut(self.0.as_mut_ptr().cast::<u8>(), self.0.len() * 8)
            }
        }
        fn bytes(&self) -> &[u8] {
            // SAFETY: as above.
            unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast::<u8>(), self.0.len() * 8) }
        }
        fn from(bytes: &[u8]) -> Self {
            let mut a = Self::new(bytes.len());
            a.bytes_mut()[..bytes.len()].copy_from_slice(bytes);
            a
        }
    }

    /// An open printer handle, closed on drop.
    struct Printer {
        handle: HANDLE,
        name: HSTRING,
    }

    impl Printer {
        fn open(name: &str) -> Result<Self, String> {
            let name = HSTRING::from(name);
            let mut handle = HANDLE::default();
            // SAFETY: `name` outlives the call; `handle` is a valid out pointer.
            unsafe { OpenPrinterW(PCWSTR(name.as_ptr()), &mut handle, None) }
                .map_err(|e| format!("OpenPrinter failed: {e}"))?;
            Ok(Self { handle, name })
        }
        fn pcwstr(&self) -> PCWSTR {
            PCWSTR(self.name.as_ptr())
        }
    }

    impl Drop for Printer {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful OpenPrinterW.
            let _ = unsafe { ClosePrinter(self.handle) };
        }
    }

    /// Copy the DEVMODE at `p` (null → `None`).
    ///
    /// # Safety
    /// `p` is null or points at a complete DEVMODEW (public + private part).
    unsafe fn copy_devmode(p: *const DEVMODEW) -> Option<Vec<u8>> {
        if p.is_null() {
            return None;
        }
        let dm = &*p;
        let len = usize::from(dm.dmSize) + usize::from(dm.dmDriverExtra);
        Some(std::slice::from_raw_parts(p.cast::<u8>(), len).to_vec())
    }

    /// The per-user defaults, through `GetPrinter`/`SetPrinter` level 9
    /// (`PRINTER_INFO_9`): the current user's own DEVMODE for a printer,
    /// stored in their profile. No administrator rights; other users never
    /// see it.
    pub struct SpoolerDefaults;

    impl UserDefaults for SpoolerDefaults {
        fn get(&self, printer: &str) -> Result<Option<Vec<u8>>, String> {
            let p = Printer::open(printer)?;
            let mut needed = 0u32;
            // SAFETY: sizing call with no buffer.
            if let Err(e) = unsafe { GetPrinterW(p.handle, 9, None, &mut needed) } {
                if e.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult() {
                    return Err(format!("GetPrinter(9) failed: {e}"));
                }
            }
            if needed == 0 {
                return Ok(None);
            }
            let mut buf = Aligned::new(needed as usize);
            // SAFETY: the buffer holds `needed` bytes.
            unsafe { GetPrinterW(p.handle, 9, Some(buf.bytes_mut()), &mut needed) }
                .map_err(|e| format!("GetPrinter(9) failed: {e}"))?;
            // SAFETY: the spooler wrote a PRINTER_INFO_9W at the start, whose
            // DEVMODE pointer (if any) points into the same buffer.
            Ok(unsafe {
                let info = &*buf.0.as_ptr().cast::<PRINTER_INFO_9W>();
                copy_devmode(info.pDevMode)
            })
        }

        fn set(&self, printer: &str, devmode: Option<&[u8]>) -> Result<(), String> {
            let p = Printer::open(printer)?;
            let mut storage = devmode.map(Aligned::from);
            let info = PRINTER_INFO_9W {
                pDevMode: storage.as_mut().map_or(std::ptr::null_mut(), |s| {
                    s.0.as_mut_ptr().cast::<DEVMODEW>()
                }),
            };
            // SAFETY: `info` and the DEVMODE it points to live across the call.
            unsafe { SetPrinterW(p.handle, 9, Some(std::ptr::addr_of!(info).cast::<u8>()), 0) }
                .map_err(|e| format!("SetPrinter(9) failed: {e}"))
        }
    }

    /// `DocumentProperties`: with `input` `None`, the user's current default
    /// DEVMODE (what SumatraPDF starts from); with `Some`, that DEVMODE
    /// merged and validated by the driver.
    fn document_properties(p: &Printer, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        // SAFETY: sizing call.
        let size =
            unsafe { DocumentPropertiesW(HWND::default(), p.handle, p.pcwstr(), None, None, 0) };
        if size <= 0 {
            return Err("DocumentProperties could not size the DEVMODE".to_string());
        }
        let mut out = Aligned::new(size as usize);
        let input = input.map(Aligned::from);
        let mode = match &input {
            Some(_) => DM_IN_BUFFER.0 | DM_OUT_BUFFER.0,
            None => DM_OUT_BUFFER.0,
        };
        // SAFETY: `out` holds `size` bytes; `input` (if any) a whole DEVMODE.
        let r = unsafe {
            DocumentPropertiesW(
                HWND::default(),
                p.handle,
                p.pcwstr(),
                Some(out.0.as_mut_ptr().cast::<DEVMODEW>()),
                input.as_ref().map(|i| i.0.as_ptr().cast::<DEVMODEW>()),
                mode,
            )
        };
        if r < 0 {
            return Err("DocumentProperties failed".to_string());
        }
        let len = devmode_len(out.bytes()).ok_or("DocumentProperties returned no DEVMODE")?;
        Ok(out.bytes()[..len].to_vec())
    }

    /// COM for this thread, for as long as the guard lives (the PrintTicket
    /// provider and its streams need it). A thread that already has COM in
    /// another mode keeps it.
    struct Com(bool);

    impl Com {
        fn init() -> Self {
            // SAFETY: plain per-thread initialization.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Self(hr.is_ok())
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
        }
    }

    /// A PrintTicket provider, closed on drop. Bound to the thread that
    /// opened it: everything below runs on one thread.
    struct Provider(HPTPROVIDER);

    impl Provider {
        fn open(p: &Printer) -> Result<Self, String> {
            // SAFETY: the printer name outlives the call.
            unsafe { PTOpenProvider(p.pcwstr(), 1) }
                .map(Self)
                .map_err(|e| format!("PTOpenProvider failed: {e}"))
        }
    }

    impl Drop for Provider {
        fn drop(&mut self) {
            // SAFETY: the provider came from PTOpenProvider.
            let _ = unsafe { PTCloseProvider(self.0) };
        }
    }

    fn stream() -> Result<IStream, String> {
        // SAFETY: a fresh HGLOBAL-backed stream that frees itself.
        unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }
            .map_err(|e| format!("CreateStreamOnHGlobal failed: {e}"))
    }

    fn stream_with(bytes: &[u8]) -> Result<IStream, String> {
        let s = stream()?;
        let mut written = 0u32;
        // SAFETY: `bytes` is valid for its length.
        unsafe {
            s.Write(
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                Some(&mut written),
            )
        }
        .ok()
        .map_err(|e| format!("stream write failed: {e}"))?;
        rewind(&s)?;
        Ok(s)
    }

    fn rewind(s: &IStream) -> Result<(), String> {
        // SAFETY: plain seek on a live stream.
        unsafe { s.Seek(0, STREAM_SEEK_SET, None) }.map_err(|e| format!("stream seek failed: {e}"))
    }

    fn read_all(s: &IStream) -> Result<String, String> {
        rewind(s)?;
        let mut out = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            // SAFETY: `chunk` is valid for its length.
            unsafe {
                s.Read(
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    Some(&mut read),
                )
            }
            .ok()
            .map_err(|e| format!("stream read failed: {e}"))?;
            if read == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..read as usize]);
        }
        rewind(s)?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn ticket_of(prov: &Provider, dm: &[u8]) -> Result<IStream, String> {
        let ticket = stream()?;
        // SAFETY: `dm` is a whole DEVMODEW (the binding names it DEVMODEA,
        // the function takes the Unicode one).
        unsafe {
            PTConvertDevModeToPrintTicket(
                prov.0,
                dm.len() as u32,
                Aligned::from(dm).0.as_ptr().cast::<DEVMODEA>(),
                kPTJobScope,
                &ticket,
            )
        }
        .map_err(|e| format!("PTConvertDevModeToPrintTicket failed: {e}"))?;
        rewind(&ticket)?;
        Ok(ticket)
    }

    fn capabilities(prov: &Provider, ticket: &IStream) -> Result<String, String> {
        let caps = stream()?;
        rewind(ticket)?;
        // SAFETY: live streams.
        unsafe { PTGetPrintCapabilities(prov.0, ticket, &caps, None) }
            .map_err(|e| format!("PTGetPrintCapabilities failed: {e}"))?;
        read_all(&caps)
    }

    /// The user's default DEVMODE for `printer` with `target` selected, or
    /// `None` when the driver offers no such quality (see the module docs).
    pub fn build_job_devmode(printer: &str, target: Target) -> Result<Option<Vec<u8>>, String> {
        let _com = Com::init();
        let p = Printer::open(printer)?;
        let base = document_properties(&p, None)?;
        match via_print_ticket(&p, &base, target) {
            Ok(result) => return Ok(result),
            Err(e) => tracing::info!(
                event = "devmode_swap_print_ticket_unavailable",
                printer = %printer,
                error = %e,
            ),
        }
        // No PrintTicket provider: the public field, if the driver uses it.
        if print_quality(&base).is_none() {
            return Ok(None);
        }
        let asked = with_print_quality(&base, target.dmres()).ok_or("not a DEVMODE")?;
        let validated = document_properties(&p, Some(&asked))?;
        Ok((print_quality(&validated) == Some(target.dmres())).then_some(validated))
    }

    fn via_print_ticket(
        p: &Printer,
        base: &[u8],
        target: Target,
    ) -> Result<Option<Vec<u8>>, String> {
        let prov = Provider::open(p)?;
        let ticket = ticket_of(&prov, base)?;
        if !offers(&capabilities(&prov, &ticket)?, target) {
            return Ok(None);
        }
        let delta = stream_with(super::delta_ticket(target).as_bytes())?;
        let merged = stream()?;
        rewind(&ticket)?;
        // SAFETY: live streams.
        unsafe {
            PTMergeAndValidatePrintTicket(prov.0, &ticket, &delta, kPTJobScope, &merged, None)
        }
        .map_err(|e| format!("PTMergeAndValidatePrintTicket failed: {e}"))?;
        if selected_quality(&read_all(&merged)?).as_deref() != Some(target.keyword()) {
            // The driver's constraints resolved it away.
            return Ok(None);
        }
        let mut size = 0u32;
        let mut out: *mut DEVMODEA = std::ptr::null_mut();
        // SAFETY: out pointers are valid; the result is freed below.
        unsafe {
            PTConvertPrintTicketToDevMode(
                prov.0,
                &merged,
                kUserDefaultDevmode,
                kPTJobScope,
                &mut size,
                &mut out,
                None,
            )
        }
        .map_err(|e| format!("PTConvertPrintTicketToDevMode failed: {e}"))?;
        // SAFETY: the provider returned `size` bytes at `out`.
        let dm = unsafe { std::slice::from_raw_parts(out.cast::<u8>(), size as usize).to_vec() };
        // SAFETY: memory the provider allocated.
        let _ = unsafe { PTReleaseMemory(out.cast()) };
        // Let the driver validate it as it would SumatraPDF's copy.
        document_properties(p, Some(&dm)).map(Some)
    }

    /// The PrintCapabilities document of `printer` for the user's current
    /// default, for diagnostics.
    pub fn print_capabilities(printer: &str) -> Result<String, String> {
        let _com = Com::init();
        let p = Printer::open(printer)?;
        let base = document_properties(&p, None)?;
        let prov = Provider::open(&p)?;
        let ticket = ticket_of(&prov, &base)?;
        capabilities(&prov, &ticket)
    }

    /// Whether `printer`'s driver offers `target` (for the honesty preview).
    pub fn quality_support(printer: &str, target: Target) -> QualitySupport {
        let _com = Com::init();
        let probe = || -> Result<QualitySupport, String> {
            let p = Printer::open(printer)?;
            let base = document_properties(&p, None)?;
            match Provider::open(&p).and_then(|prov| {
                let ticket = ticket_of(&prov, &base)?;
                capabilities(&prov, &ticket)
            }) {
                Ok(caps) if offers(&caps, target) => Ok(QualitySupport::Offered),
                Ok(_) => Ok(QualitySupport::NotOffered),
                Err(_) if print_quality(&base).is_some() => Ok(QualitySupport::Unknown),
                Err(_) => Ok(QualitySupport::NotOffered),
            }
        };
        probe().unwrap_or(QualitySupport::Unknown)
    }

    /// The DEVMODE the spooler keeps for job `job_id` (`JOB_INFO_2`).
    pub fn job_devmode(printer: &str, job_id: u32) -> Result<Option<Vec<u8>>, String> {
        let p = Printer::open(printer)?;
        let mut needed = 0u32;
        // SAFETY: sizing call.
        let ok = unsafe { GetJobW(p.handle, job_id, 2, None, &mut needed) };
        if !ok.as_bool() {
            // SAFETY: thread-local read right after the call.
            let err = unsafe { GetLastError() };
            if err == ERROR_INVALID_PARAMETER {
                return Err("the job already left the queue".to_string());
            }
            if err != ERROR_INSUFFICIENT_BUFFER {
                return Err(format!("GetJob(2) failed: {err:?}"));
            }
        }
        let mut buf = Aligned::new(needed as usize);
        // SAFETY: the buffer holds `needed` bytes.
        let ok = unsafe { GetJobW(p.handle, job_id, 2, Some(buf.bytes_mut()), &mut needed) };
        if !ok.as_bool() {
            let err = unsafe { GetLastError() };
            return Err(format!("GetJob(2) failed: {err:?}"));
        }
        // SAFETY: a JOB_INFO_2W at the start; its DEVMODE inside the buffer.
        Ok(unsafe {
            let info = &*buf.0.as_ptr().cast::<JOB_INFO_2W>();
            copy_devmode(info.pDevMode)
        })
    }

    /// What job `job_id` on `printer` really carries for `target`.
    pub fn job_quality(printer: &str, job_id: u32, target: Target) -> QualityOutcome {
        let _com = Com::init();
        let dm = match job_devmode(printer, job_id) {
            Ok(Some(dm)) => dm,
            Ok(None) => {
                return QualityOutcome::Unverified(
                    "the spooler kept no settings for the job".into(),
                )
            }
            Err(e) => return QualityOutcome::Unverified(e),
        };
        let ticket_quality = Printer::open(printer)
            .and_then(|p| Provider::open(&p))
            .and_then(|prov| read_all(&ticket_of(&prov, &dm)?))
            .ok()
            .and_then(|xml| selected_quality(&xml));
        judge_job(target, ticket_quality.as_deref(), print_quality(&dm))
    }

    /// [`super::recover_stale_journals_in`] on the real spooler and the real
    /// journal folder, logged. Runs at every start of the app and the CLI.
    pub fn recover_stale_journals() {
        for (printer, result) in
            super::recover_stale_journals_in(&SpoolerDefaults, &super::journal_dir())
        {
            match result {
                Ok(Some(action)) => tracing::info!(
                    event = "devmode_swap_recovered",
                    printer = %printer,
                    action = ?action,
                ),
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    event = "devmode_swap_recover_failed",
                    printer = %printer,
                    error = %e,
                ),
            }
        }
    }

    /// [`recover_stale_journals`] off the calling thread: `SetPrinter` can
    /// block on a slow driver, and start-up must not wait for it.
    pub fn recover_stale_journals_in_background() {
        let _ = std::thread::Builder::new()
            .name("devmode-swap-recover".into())
            .spawn(recover_stale_journals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A driver normalizing the DEVMODE it is given.
    type Normalize = fn(&[u8]) -> Vec<u8>;

    /// An in-memory per-user default store, with a log of every write.
    #[derive(Default)]
    struct Fake {
        defaults: Mutex<HashMap<String, Vec<u8>>>,
        writes: Mutex<Vec<(String, Option<Vec<u8>>)>>,
        /// Applied to every write.
        normalize: Option<Normalize>,
    }

    impl UserDefaults for Fake {
        fn get(&self, printer: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.defaults.lock().unwrap().get(printer).cloned())
        }
        fn set(&self, printer: &str, devmode: Option<&[u8]>) -> Result<(), String> {
            let stored = devmode.map(|d| self.normalize.map_or_else(|| d.to_vec(), |n| n(d)));
            self.writes
                .lock()
                .unwrap()
                .push((printer.to_string(), stored.clone()));
            let mut map = self.defaults.lock().unwrap();
            match stored {
                Some(d) => map.insert(printer.to_string(), d),
                None => map.remove(printer),
            };
            Ok(())
        }
    }

    /// A minimal DEVMODEW: 220-byte public part plus `extra` private bytes.
    fn devmode(quality: Option<i16>, extra: &[u8]) -> Vec<u8> {
        let mut dm = vec![0u8; 220];
        dm[68..70].copy_from_slice(&220u16.to_le_bytes());
        dm[70..72].copy_from_slice(&(extra.len() as u16).to_le_bytes());
        if let Some(q) = quality {
            dm[72..76].copy_from_slice(&DM_PRINTQUALITY.to_le_bytes());
            dm[90..92].copy_from_slice(&q.to_le_bytes());
        }
        dm.extend_from_slice(extra);
        dm
    }

    const P: &str = "Office Laser";

    fn begin<'a>(store: &'a Fake, dir: &Path, dm: &[u8]) -> Result<Swap<&'a Fake>, SwapError> {
        let dm = dm.to_vec();
        Swap::begin(
            store,
            dir,
            P,
            Target::Draft,
            Duration::from_secs(5),
            move || Ok(Some(dm)),
        )
    }

    #[test]
    fn print_settings_quality_targets_only_draft_and_high() {
        assert_eq!(Target::for_quality(Quality::Draft), Some(Target::Draft));
        assert_eq!(Target::for_quality(Quality::High), Some(Target::High));
        assert_eq!(Target::for_quality(Quality::Normal), None);
        assert_eq!((Target::Draft.dmres(), Target::High.dmres()), (-1, -4));
    }

    #[test]
    fn print_settings_quality_devmode_bytes_round_trip() {
        let dm = devmode(None, b"private");
        assert_eq!(devmode_len(&dm), Some(227));
        assert_eq!(print_quality(&dm), None);
        let draft = with_print_quality(&dm, -1).unwrap();
        assert_eq!(print_quality(&draft), Some(-1));
        assert_eq!(
            &draft[220..],
            b"private",
            "the driver's private part is kept"
        );
        assert_eq!(devmode_len(&[0u8; 10]), None);

        let mut dm = devmode(None, b"");
        assert_eq!(
            (paper_size_field(&dm), default_source_field(&dm)),
            (None, None)
        );
        dm[72..76].copy_from_slice(&(DM_PAPERSIZE | DM_DEFAULTSOURCE).to_le_bytes());
        dm[78..80].copy_from_slice(&1u16.to_le_bytes());
        dm[88..90].copy_from_slice(&260u16.to_le_bytes());
        assert_eq!(paper_size_field(&dm), Some(1));
        assert_eq!(default_source_field(&dm), Some(260));
    }

    const CAPS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<psf:PrintCapabilities xmlns:psf="http://schemas.microsoft.com/windows/2003/08/printing/printschemaframework" xmlns:psk="http://schemas.microsoft.com/windows/2003/08/printing/printschemakeywords" xmlns:ns0000="http://schemas.example.com/private" version="1">
  <psf:Feature name="psk:PageMediaSize"><psf:Option name="psk:ISOA4"/><psf:Option name="psk:High"/></psf:Feature>
  <psf:Feature name="psk:PageOutputQuality">
    <psf:Property name="psf:SelectionType"><psf:Value xsi:type="xsd:QName">psk:PickOne</psf:Value></psf:Property>
    <psf:Option name="psk:Draft" constrained="psk:None"><psf:Property name="psk:DisplayName"><psf:Value>Draft</psf:Value></psf:Property></psf:Option>
    <psf:Option name='psk:Normal'/>
    <psf:Option name="ns0000:High"/>
  </psf:Feature>
</psf:PrintCapabilities>"#;

    #[test]
    fn print_settings_quality_capabilities_are_read_from_the_print_schema() {
        assert_eq!(
            quality_options(CAPS),
            vec!["Draft", "Normal", "ns0000:High"]
        );
        assert!(offers(CAPS, Target::Draft));
        // A private `High`, or `High` under another feature, is not psk:High.
        assert!(!offers(CAPS, Target::High));
        // Another prefix bound to the keywords namespace still reads.
        let other = CAPS.replace("xmlns:psk=", "xmlns:k=").replace("psk:", "k:");
        assert!(offers(&other, Target::Draft));
        assert!(!offers("<garbage", Target::Draft));
    }

    #[test]
    fn print_settings_quality_delta_ticket_selects_the_target() {
        let t = delta_ticket(Target::High);
        assert_eq!(selected_quality(&t).as_deref(), Some("High"));
        assert!(t.contains(PSF_NS) && t.contains(PSK_NS));
    }

    #[test]
    fn print_settings_quality_job_is_judged_by_its_own_settings() {
        assert_eq!(
            judge_job(Target::Draft, Some("Draft"), None),
            QualityOutcome::Applied
        );
        assert!(matches!(
            judge_job(Target::Draft, Some("Normal"), Some(-1)),
            QualityOutcome::NotApplied(m) if m.contains("Normal")
        ));
        assert_eq!(
            judge_job(Target::High, None, Some(-4)),
            QualityOutcome::Applied
        );
        assert!(matches!(
            judge_job(Target::High, None, None),
            QualityOutcome::NotApplied(_)
        ));
    }

    #[test]
    fn print_settings_quality_swap_sets_then_restores_the_original_bytewise() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake::default();
        let original = devmode(Some(-3), b"user private bytes");
        store.set(P, Some(&original)).unwrap();
        let draft = devmode(Some(-1), b"draft");

        let mut swap = begin(&store, dir.path(), &draft).unwrap();
        assert_eq!(store.get(P).unwrap().as_deref(), Some(&draft[..]));
        assert!(
            journal_path(dir.path(), P).exists(),
            "journaled before restore"
        );
        assert_eq!(swap.restore(), Ok(RestoreAction::Restore));
        assert_eq!(store.get(P).unwrap().as_deref(), Some(&original[..]));
        assert!(!journal_path(dir.path(), P).exists());
        // Idempotent, and drop does not write again.
        assert_eq!(swap.restore(), Ok(RestoreAction::Restore));
        let writes = store.writes.lock().unwrap().len();
        drop(swap);
        assert_eq!(store.writes.lock().unwrap().len(), writes);
    }

    #[test]
    fn print_settings_quality_swap_restores_bytewise_after_abort() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake::default();
        let original = devmode(Some(-3), b"user private bytes");
        store.set(P, Some(&original)).unwrap();

        // The process dies right after the swap: no restore; the OS drops
        // its lock.
        let swap = begin(&store, dir.path(), &devmode(Some(-1), b"d")).unwrap();
        swap.crash();
        assert_ne!(store.get(P).unwrap().as_deref(), Some(&original[..]));

        // Next start.
        let results = recover_stale_journals_in(&store, dir.path());
        assert_eq!(
            results,
            vec![(P.to_string(), Ok(Some(RestoreAction::Restore)))]
        );
        assert_eq!(store.get(P).unwrap().as_deref(), Some(&original[..]));
        assert!(!journal_path(dir.path(), P).exists());
        assert!(recover_stale_journals_in(&store, dir.path()).is_empty());
    }

    #[test]
    fn print_settings_quality_swap_crash_recovery_runs_at_the_next_swap_too() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake::default();
        let original = devmode(Some(-3), b"u");
        store.set(P, Some(&original)).unwrap();
        // A crashed process left a journal and the swapped default behind.
        let left = devmode(Some(-4), b"h");
        store.set(P, Some(&left)).unwrap();
        write_journal(
            &journal_path(dir.path(), P),
            &Journal {
                version: 1,
                printer: P.into(),
                original: Some(hex(&original)),
                intended: hex(&left),
                written: Some(hex(&left)),
                pid: 1,
                started: String::new(),
            },
        )
        .unwrap();
        // The next swap first restores, then builds from the true default.
        let seen = Mutex::new(None);
        let swap = Swap::begin(
            &store,
            dir.path(),
            P,
            Target::Draft,
            Duration::from_secs(5),
            || {
                *seen.lock().unwrap() = store.get(P).unwrap();
                Ok(Some(devmode(Some(-1), b"d")))
            },
        )
        .unwrap();
        assert_eq!(seen.lock().unwrap().as_deref(), Some(&original[..]));
        drop(swap);
        assert_eq!(store.get(P).unwrap().as_deref(), Some(&original[..]));
    }

    #[test]
    fn print_settings_quality_swap_does_not_clobber_user_change() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake::default();
        store.set(P, Some(&devmode(Some(-3), b"u"))).unwrap();
        let mut swap = begin(&store, dir.path(), &devmode(Some(-1), b"d")).unwrap();
        // The user picks another default while the job is being handed over.
        let theirs = devmode(Some(-2), b"their choice");
        store.set(P, Some(&theirs)).unwrap();
        assert_eq!(swap.restore(), Ok(RestoreAction::LeaveUserChange));
        assert_eq!(store.get(P).unwrap().as_deref(), Some(&theirs[..]));
        assert!(!journal_path(dir.path(), P).exists());
    }

    #[test]
    fn print_settings_quality_swap_compares_with_what_the_driver_stored() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake {
            normalize: Some(|d| {
                let mut d = d.to_vec();
                d.push(0xAA);
                d[70] += 1; // one more private byte
                d
            }),
            ..Fake::default()
        };
        store
            .defaults
            .lock()
            .unwrap()
            .insert(P.into(), devmode(None, b""));
        let mut swap = begin(&store, dir.path(), &devmode(Some(-1), b"d")).unwrap();
        // What the store holds is not what was asked for, yet it is ours.
        assert_eq!(swap.restore(), Ok(RestoreAction::Restore));
    }

    #[test]
    fn print_settings_quality_swap_null_user_devmode_restored() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake::default();
        assert_eq!(store.get(P).unwrap(), None, "no per-user default");
        let mut swap = begin(&store, dir.path(), &devmode(Some(-1), b"d")).unwrap();
        assert!(store.get(P).unwrap().is_some());
        assert_eq!(swap.restore(), Ok(RestoreAction::Restore));
        assert_eq!(
            store.get(P).unwrap(),
            None,
            "cleared again, not left as ours"
        );
        assert_eq!(store.writes.lock().unwrap().last().unwrap().1, None);
    }

    #[test]
    fn print_settings_quality_swap_not_offered_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake::default();
        let r = Swap::begin(&store, dir.path(), P, Target::High, Duration::ZERO, || {
            Ok(None)
        });
        assert!(matches!(r, Err(SwapError::NotOffered("high"))));
        assert!(store.writes.lock().unwrap().is_empty());
        assert!(!journal_path(dir.path(), P).exists());
    }

    #[test]
    fn print_settings_quality_swap_killed_before_readback_restores_only_an_exact_match() {
        let original = devmode(Some(-3), b"u");
        let intended = devmode(Some(-1), b"d");
        let j = Journal {
            version: 1,
            printer: P.into(),
            original: Some(hex(&original)),
            intended: hex(&intended),
            written: None,
            pid: 1,
            started: String::new(),
        };
        assert_eq!(restore_action(Some(&intended), &j), RestoreAction::Restore);
        assert_eq!(
            restore_action(Some(&original), &j),
            RestoreAction::AlreadyOriginal
        );
        assert_eq!(
            restore_action(Some(&devmode(Some(-2), b"x")), &j),
            RestoreAction::LeaveUserChange
        );
        assert_eq!(restore_action(None, &j), RestoreAction::LeaveUserChange);
    }

    #[test]
    fn print_settings_quality_swap_concurrent_jobs_each_get_their_own_quality() {
        use std::sync::Arc;
        let dir = Arc::new(tempfile::tempdir().unwrap());
        let store = Arc::new(Fake::default());
        let original = devmode(Some(-3), b"u");
        store.set(P, Some(&original)).unwrap();
        // Each "job" records the default it saw while its swap was in place:
        // with the lock, nobody's swap overlaps anybody else's.
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let (dir, store, seen) = (dir.clone(), store.clone(), seen.clone());
                std::thread::spawn(move || {
                    let (target, q) = if i % 2 == 0 {
                        (Target::Draft, -1)
                    } else {
                        (Target::High, -4)
                    };
                    let mut swap = Swap::begin(
                        &*store,
                        dir.path(),
                        P,
                        target,
                        Duration::from_secs(30),
                        || Ok(Some(devmode(Some(q), b"j"))),
                    )
                    .unwrap();
                    std::thread::sleep(Duration::from_millis(20));
                    let during = print_quality(&store.get(P).unwrap().unwrap());
                    seen.lock().unwrap().push((q, during));
                    swap.restore().unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        for (asked, during) in seen.lock().unwrap().iter() {
            assert_eq!(Some(*asked), *during);
        }
        assert_eq!(store.get(P).unwrap().as_deref(), Some(&original[..]));
    }

    #[test]
    fn print_settings_quality_swap_busy_printer_is_not_swapped() {
        let dir = tempfile::tempdir().unwrap();
        let store = Fake::default();
        let _held = lock_printer(dir.path(), P, Duration::ZERO)
            .unwrap()
            .unwrap();
        let r = begin_with_timeout(&store, dir.path(), Duration::from_millis(100));
        assert!(matches!(r, Err(SwapError::Busy)));
        // A live swap's journal is not "stale".
        write_journal(
            &journal_path(dir.path(), P),
            &Journal {
                version: 1,
                printer: P.into(),
                original: None,
                intended: String::new(),
                written: None,
                pid: 1,
                started: String::new(),
            },
        )
        .unwrap();
        assert!(recover_stale_journals_in(&store, dir.path()).is_empty());
        assert!(store.writes.lock().unwrap().is_empty());
    }

    fn begin_with_timeout<'a>(
        store: &'a Fake,
        dir: &Path,
        timeout: Duration,
    ) -> Result<Swap<&'a Fake>, SwapError> {
        Swap::begin(store, dir, P, Target::Draft, timeout, || {
            Ok(Some(devmode(Some(-1), b"")))
        })
    }

    #[test]
    fn print_settings_quality_corrupt_journal_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = journal_path(dir.path(), P);
        std::fs::write(&path, b"{not json").unwrap();
        let store = Fake::default();
        let results = recover_stale_journals_in(&store, dir.path());
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_err());
        assert!(!path.exists());
        assert!(path.with_extension("json.bad").exists());
    }

    #[test]
    fn print_settings_quality_journal_names_are_per_printer_and_case_insensitive() {
        let d = Path::new("x");
        assert_eq!(journal_path(d, "HP"), journal_path(d, "hp"));
        assert_ne!(journal_path(d, "HP"), journal_path(d, "Canon"));
        let name = journal_path(d, r"\\server\queue");
        assert!(!name.file_name().unwrap().to_string_lossy().contains('\\'));
        assert_eq!(
            unhex(&hex(&[0, 1, 0xfe, 0xff])),
            Some(vec![0, 1, 0xfe, 0xff])
        );
        assert_eq!(unhex("abc"), None);
    }
}
