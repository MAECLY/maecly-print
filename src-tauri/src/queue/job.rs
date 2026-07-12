//! Print-queue domain types and the job state machine.
//!
//! These are the wire types shared between the dispatcher, the on-disk
//! `queue.json` ([`crate::queue::persist`]) and the frontend (via
//! `specta`-generated bindings). The schema mirrors
//! `docs/04-architecture/data-model.md` §4 and the `QueueSnapshot` contract in
//! `docs/04-architecture/ipc-and-cli.md`.
//!
//! Everything here is pure data plus the [`can_transition`] guard: no I/O, no
//! process spawning. The dispatcher (a later stage) owns the actual driving of
//! jobs through the pipeline.

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// On-disk schema version for `queue.json`. Bumped only on a
/// forward-incompatible change; [`crate::queue::persist::load`] refuses to read
/// a file whose version is greater than this.
pub const SCHEMA_VERSION: u32 = 1;

/// Lifecycle state of a single print job.
///
/// The legal transitions between states are encoded in [`can_transition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    /// Created but not yet started.
    Queued,
    /// A converter process is running to produce the cache PDF.
    Converting,
    /// PDF is in the cache; waiting for a free printer.
    Converted,
    /// Handed off to the OS spooler.
    Printing,
    /// The printer reported completion (terminal).
    Done,
    /// Terminal error; details in [`Job::error`] (terminal).
    Failed,
    /// Transient stop — user paused it, or it was interrupted by shutdown.
    Paused,
    /// User cancelled the job (terminal).
    Cancelled,
}

impl JobState {
    /// `true` for states from which no further work happens: [`Done`],
    /// [`Failed`], [`Cancelled`].
    ///
    /// [`Done`]: JobState::Done
    /// [`Failed`]: JobState::Failed
    /// [`Cancelled`]: JobState::Cancelled
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Done | JobState::Failed | JobState::Cancelled
        )
    }

    /// `true` while a job is actively occupying an external resource — a
    /// converter process ([`Converting`]) or the spooler ([`Printing`]). These
    /// are exactly the states that shutdown recovery rewrites to [`Paused`].
    ///
    /// [`Converting`]: JobState::Converting
    /// [`Printing`]: JobState::Printing
    /// [`Paused`]: JobState::Paused
    pub fn is_active(self) -> bool {
        matches!(self, JobState::Converting | JobState::Printing)
    }
}

/// Duplex (double-sided) printing mode.
///
/// Note: the [`None`](Duplex::None) variant serializes to the string
/// `"none"`, matching the other lowercase enums here — it is unrelated to
/// [`Option::None`]. Refer to it by its full path (`Duplex::None`) in code to
/// avoid confusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Duplex {
    /// Single-sided.
    None,
    /// Double-sided, bound on the long edge.
    Long,
    /// Double-sided, bound on the short edge.
    Short,
}

/// Color vs. grayscale output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Full color.
    Color,
    /// Grayscale / black-and-white.
    Monochrome,
}

/// Target paper size. Serializes lowercase: `"a4"`, `"letter"`, `"legal"`,
/// `"a3"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PaperSize {
    A4,
    Letter,
    Legal,
    A3,
}

/// Page orientation. [`Auto`](Orientation::Auto) lets the engine pick per page
/// based on the source aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Auto,
    Portrait,
    Landscape,
}

/// How source pages are fitted to the target paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Scaling {
    /// Scale up or down to fill the printable area.
    Fit,
    /// Only shrink oversized pages; never enlarge.
    Shrink,
    /// Print at the source's actual size.
    None,
}

/// Per-job print settings. A resolved bundle: a profile plus any
/// per-invocation overrides collapse into one of these before a [`Job`] is
/// created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct JobSettings {
    /// Target printer id, or `None` to use the app/OS default at dispatch time.
    pub printer: Option<String>,
    pub duplex: Duplex,
    pub color_mode: ColorMode,
    /// Number of copies, `1..=999`.
    pub copies: u16,
    pub paper_size: PaperSize,
    pub orientation: Orientation,
    pub scaling: Scaling,
    /// Page-range expression (e.g. `"1-5,8"`), or `None` for all pages. The
    /// *syntax* is validated by the engine's parser, not here — see
    /// [`JobSettings::validate`].
    pub page_ranges: Option<String>,
}

impl Default for JobSettings {
    fn default() -> Self {
        Self {
            printer: None,
            duplex: Duplex::None,
            color_mode: ColorMode::Color,
            copies: 1,
            paper_size: PaperSize::A4,
            orientation: Orientation::Auto,
            scaling: Scaling::Fit,
            page_ranges: None,
        }
    }
}

impl JobSettings {
    /// Validate the fields this layer owns.
    ///
    /// Checks:
    /// - `copies` is in `1..=999`.
    /// - `page_ranges`, if present, is not the empty string.
    ///
    /// The page-range *grammar* is deliberately not validated here — the engine
    /// owns that parser. This only rejects an obviously empty range so the UI
    /// can surface an inline error early (FR-042).
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=999).contains(&self.copies) {
            return Err(format!(
                "copies must be between 1 and 999, got {}",
                self.copies
            ));
        }
        if let Some(ranges) = &self.page_ranges {
            if ranges.trim().is_empty() {
                return Err("page range must not be empty".to_string());
            }
        }
        Ok(())
    }
}

/// A request from the frontend/CLI to print one or more files with a shared
/// [`JobSettings`]. The dispatcher fans this out into one [`Job`] per file,
/// grouped under a single batch id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PrintRequest {
    /// Absolute paths of the files to print.
    pub files: Vec<String>,
    pub settings: JobSettings,
    /// Name of the profile these settings were derived from, for the log/UI.
    pub profile_name: Option<String>,
}

/// A structured job error. `code` is a stable machine-readable identifier;
/// `message` is human-readable (already localized/englished by the producer).
///
/// The special code `"interrupted_by_shutdown"` is set by shutdown recovery
/// ([`crate::queue::persist::recover`]) on jobs that were mid-flight when the
/// app was last closed; the UI treats it as resumable rather than a hard
/// failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct JobError {
    pub code: String,
    pub message: String,
}

impl JobError {
    /// Construct a [`JobError`] from any string-like code and message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Coarse progress for the currently-running phase of a job. `percent` is in
/// `0.0..=1.0`; `phase` is a short label such as `"converting"` or
/// `"printing"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct JobProgress {
    pub phase: String,
    pub percent: f32,
}

/// One print job: a single input file tracked through the convert → print
/// pipeline. Persisted verbatim in `queue.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Job {
    /// UUID v7 (time-sortable), assigned at creation.
    pub id: String,
    /// Groups all jobs submitted together in one [`PrintRequest`].
    pub batch_id: String,
    pub state: JobState,
    /// Absolute path of the source file.
    pub input_path: String,
    /// Display file name (last path component of `input_path`).
    pub file_name: String,
    /// Lowercased extension without the leading dot (e.g. `"pdf"`), empty when
    /// the file has none.
    pub input_ext: String,
    /// Source size in bytes (filled by the dispatcher after `stat`).
    pub input_bytes: u64,
    /// Page count once known (after conversion / PDF parse).
    pub pages: Option<u32>,
    /// Absolute path of the converted PDF in the cache, once produced.
    pub cache_pdf: Option<String>,
    pub settings: JobSettings,
    /// Profile name these settings came from, for the log/UI.
    pub profile_name: Option<String>,
    /// Printer actually chosen at dispatch time (after resolving `<default>`).
    pub printer_resolved: Option<String>,
    /// Number of times this job has been started (for retry/backoff, FR-034).
    pub attempts: u32,
    pub error: Option<JobError>,
    pub progress: Option<JobProgress>,
    /// RFC 3339 UTC timestamp of submission.
    pub submitted_at: String,
    /// RFC 3339 UTC timestamp of first start, if started.
    pub started_at: Option<String>,
    /// RFC 3339 UTC timestamp of reaching a terminal state, if finished.
    pub finished_at: Option<String>,
}

impl Job {
    /// Create a fresh [`JobState::Queued`] job for `input_path`.
    ///
    /// `file_name` and `input_ext` are derived from the path; the `id` is a new
    /// UUID v7. `input_bytes` starts at `0` — the caller fills it after
    /// `stat`ing the file. `submitted_at` is passed in (use [`now_rfc3339`]) so
    /// every job in a batch can share one timestamp.
    pub fn new(
        batch_id: impl Into<String>,
        input_path: impl Into<String>,
        settings: JobSettings,
        profile_name: Option<String>,
        submitted_at: String,
    ) -> Job {
        let input_path = input_path.into();
        let path = std::path::Path::new(&input_path);
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| input_path.clone());
        let input_ext = path
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        Job {
            id: uuid::Uuid::now_v7().to_string(),
            batch_id: batch_id.into(),
            state: JobState::Queued,
            input_path,
            file_name,
            input_ext,
            input_bytes: 0,
            pages: None,
            cache_pdf: None,
            settings,
            profile_name,
            printer_resolved: None,
            attempts: 0,
            error: None,
            progress: None,
            submitted_at,
            started_at: None,
            finished_at: None,
        }
    }
}

/// The result of submitting a [`PrintRequest`]: the batch id and the ids of the
/// jobs it produced (one per input file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct JobBatch {
    pub batch_id: String,
    pub job_ids: Vec<String>,
}

/// Serializable view of the whole queue, returned by the `queue_status` IPC
/// command and pushed via the `queue-updated` event. Also the on-disk shape of
/// `queue.json` (reused by [`crate::queue::persist`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct QueueSnapshot {
    pub schema_version: u32,
    /// Whether the whole queue is paused (FR-033).
    pub queue_paused: bool,
    pub jobs: Vec<Job>,
}

impl Default for QueueSnapshot {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            queue_paused: false,
            jobs: Vec::new(),
        }
    }
}

/// Current UTC time as an RFC 3339 string, the format used for every timestamp
/// in `queue.json`.
///
/// Falls back to the Unix epoch string (rather than panicking) in the
/// impossible case that formatting fails.
pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Pure guard for the job state machine: `true` iff a job may move directly
/// from `from` to `to`.
///
/// The legal transitions (from `docs/04-architecture/data-model.md` §4):
///
/// - `Queued`     → `Converting` (start) | `Cancelled` | `Paused`
/// - `Converting` → `Converted` (ok) | `Failed` | `Cancelled` | `Paused` |
///   `Queued` (retry resets to the start of the pipeline)
/// - `Converted`  → `Printing` (start) | `Cancelled` | `Paused` | `Converted`
///   (a retry that keeps the cached PDF stays in place)
/// - `Printing`   → `Done` | `Failed` | `Cancelled` | `Paused` | `Converted`
///   (a transient print failure returns to `Converted`, keeping the cached PDF
///   for the delayed retry)
/// - `Paused`     → `Queued` | `Converted` | `Cancelled` — resume returns the
///   job to whichever pre-pause stage the dispatcher decides is correct
///   (re-convert vs. re-print); this guard permits both targets.
/// - `Failed`     → `Queued` (retry). This is the only transition out of a
///   terminal state.
/// - `Done` / `Cancelled` are terminal with no successors.
///
/// The guard is intentionally permissive where the docs are silent; every
/// allowance above is deliberate.
pub fn can_transition(from: JobState, to: JobState) -> bool {
    use JobState::*;
    matches!(
        (from, to),
        (Queued, Converting)
            | (Queued, Cancelled)
            | (Queued, Paused)
            | (Converting, Converted)
            | (Converting, Failed)
            | (Converting, Cancelled)
            | (Converting, Paused)
            | (Converting, Queued)
            | (Converted, Printing)
            | (Converted, Cancelled)
            | (Converted, Paused)
            | (Converted, Converted)
            | (Printing, Done)
            | (Printing, Failed)
            | (Printing, Cancelled)
            | (Printing, Paused)
            | (Printing, Converted)
            | (Paused, Queued)
            | (Paused, Converted)
            | (Paused, Cancelled)
            | (Failed, Queued)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_and_active_predicates() {
        assert!(JobState::Done.is_terminal());
        assert!(JobState::Failed.is_terminal());
        assert!(JobState::Cancelled.is_terminal());
        assert!(!JobState::Queued.is_terminal());
        assert!(!JobState::Paused.is_terminal());

        assert!(JobState::Converting.is_active());
        assert!(JobState::Printing.is_active());
        assert!(!JobState::Queued.is_active());
        assert!(!JobState::Converted.is_active());
        assert!(!JobState::Done.is_active());
    }

    #[test]
    fn default_settings_are_sane() {
        let s = JobSettings::default();
        assert_eq!(s.duplex, Duplex::None);
        assert_eq!(s.color_mode, ColorMode::Color);
        assert_eq!(s.copies, 1);
        assert_eq!(s.paper_size, PaperSize::A4);
        assert_eq!(s.orientation, Orientation::Auto);
        assert_eq!(s.scaling, Scaling::Fit);
        assert!(s.page_ranges.is_none());
        assert!(s.printer.is_none());
        s.validate().expect("defaults must validate");
    }

    #[test]
    fn validate_copies_bounds() {
        let with_copies = |copies| JobSettings {
            copies,
            ..Default::default()
        };
        assert!(with_copies(0).validate().is_err());
        assert!(with_copies(1000).validate().is_err());
        assert!(with_copies(1).validate().is_ok());
        assert!(with_copies(999).validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_page_ranges() {
        let with_ranges = |r: Option<&str>| JobSettings {
            page_ranges: r.map(String::from),
            ..Default::default()
        };
        assert!(with_ranges(Some("")).validate().is_err());
        assert!(with_ranges(Some("   ")).validate().is_err());
        // Syntax is NOT checked here: any non-empty string passes this layer.
        assert!(with_ranges(Some("not-a-real-range")).validate().is_ok());
        assert!(with_ranges(Some("1-5,8")).validate().is_ok());
    }

    #[test]
    fn new_job_derives_name_and_ext() {
        let job = Job::new(
            "batch-1",
            "/home/x/report.PDF",
            JobSettings::default(),
            Some("Quick BW".to_string()),
            now_rfc3339(),
        );
        assert_eq!(job.file_name, "report.PDF");
        assert_eq!(job.input_ext, "pdf");
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.batch_id, "batch-1");
        assert_eq!(job.attempts, 0);
        assert_eq!(job.input_bytes, 0);
        assert!(job.error.is_none());
        assert!(!job.id.is_empty());
        assert_eq!(job.profile_name.as_deref(), Some("Quick BW"));
    }

    #[test]
    fn new_job_handles_extensionless_path() {
        let job = Job::new(
            "b",
            "/home/x/Makefile",
            JobSettings::default(),
            None,
            now_rfc3339(),
        );
        assert_eq!(job.file_name, "Makefile");
        assert_eq!(job.input_ext, "");
    }

    #[test]
    fn can_transition_legal_moves() {
        use JobState::*;
        assert!(can_transition(Queued, Converting));
        assert!(can_transition(Converting, Converted));
        assert!(can_transition(Converting, Queued)); // retry
        assert!(can_transition(Converted, Printing));
        assert!(can_transition(Printing, Done));
        assert!(can_transition(Printing, Paused));
        assert!(can_transition(Paused, Converted)); // resume
        assert!(can_transition(Paused, Queued)); // resume
        assert!(can_transition(Failed, Queued)); // retry
    }

    #[test]
    fn can_transition_illegal_moves() {
        use JobState::*;
        assert!(!can_transition(Queued, Done));
        assert!(!can_transition(Queued, Printing));
        assert!(!can_transition(Done, Queued)); // done is terminal
        assert!(!can_transition(Cancelled, Queued)); // cancelled is terminal
        assert!(!can_transition(Done, Printing));
        assert!(!can_transition(Converting, Printing)); // must pass through converted
        assert!(!can_transition(Failed, Printing)); // retry only goes to queued
    }

    #[test]
    fn serialized_job_uses_lowercase_wire_format() {
        let job = Job::new(
            "batch-1",
            "/tmp/a.pdf",
            JobSettings::default(),
            None,
            "2026-07-11T10:00:00Z".to_string(),
        );
        let v = serde_json::to_value(&job).expect("job serializes");
        assert_eq!(v["state"], "queued");
        assert_eq!(v["settings"]["duplex"], "none");
        assert_eq!(v["settings"]["color_mode"], "color");
        assert_eq!(v["settings"]["paper_size"], "a4");
        assert_eq!(v["settings"]["orientation"], "auto");
        assert_eq!(v["settings"]["scaling"], "fit");
        assert_eq!(v["input_ext"], "pdf");
        assert_eq!(v["submitted_at"], "2026-07-11T10:00:00Z");
        // Optionals present as JSON null, not omitted.
        assert!(v["error"].is_null());
        assert!(v["cache_pdf"].is_null());
    }

    #[test]
    fn snapshot_default_is_empty_and_current() {
        let snap = QueueSnapshot::default();
        assert_eq!(snap.schema_version, SCHEMA_VERSION);
        assert!(!snap.queue_paused);
        assert!(snap.jobs.is_empty());
    }
}
