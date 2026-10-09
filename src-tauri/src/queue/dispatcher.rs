//! The single-writer print-queue dispatcher.
//!
//! A [`Dispatcher`] owns the in-memory [`QueueState`] and is the *only* thing
//! that drives jobs through the `Queued → Converting → Converted → Printing →
//! Done` pipeline. All mutations flow through a single [`tokio`] task consuming a
//! [`Cmd`] channel, so there is no lock contention on the state machine itself:
//! the `RwLock` around [`QueueState`] exists only so read-only snapshots
//! ([`QueueHandle::snapshot`](crate::queue::QueueHandle::snapshot)) and the
//! in-flight progress callback can peek without a channel round-trip. The task
//! never holds the write guard across an `.await`.
//!
//! ## Testability
//!
//! The three side-effecting concerns are behind traits — [`JobRunner`] (the
//! engine), [`EventSink`] (Tauri event emission) and [`BlockingSpawner`] (how
//! blocking convert/print work is offloaded). Production wires the real
//! [`EngineRunner`] / [`TauriEmit`] / [`TokioBlockingSpawner`]; tests substitute
//! deterministic fakes and step the loop one [`Cmd`] at a time.
//!
//! ## Concurrency & retry decisions (documented behaviour)
//!
//! - Up to [`DispatcherLimits::max_concurrent`] conversions run at once
//!   (config-driven, defaulting to [`DEFAULT_MAX_CONCURRENT_CONVERSIONS`]);
//!   exactly one print runs at a time (spoolers serialise anyway, and it keeps
//!   job ordering legible). A conversion that outruns
//!   [`DispatcherLimits::conversion_timeout`] is failed with a
//!   `"conversion_timeout"` error (no retry). The print stage is bounded the
//!   same way by [`DispatcherLimits::print_timeout`]: the spooler child is
//!   killed and the job failed with `"print_timeout"` (also terminal), so a hung
//!   `lp`/SumatraPDF can never hold the single print slot forever.
//! - A printer that the OS no longer enumerates — or no configured printer at
//!   all — **pauses** the job with `"printer_unavailable"` instead of failing
//!   it, so the user can pick a replacement
//!   (`docs/03-requirements/print-settings.md` §printer).
//! - `queue_paused` gates *starting* new work only. Work already in flight runs
//!   to completion; a converting job that finishes while the queue is paused
//!   still reaches `Converted`, it just won't advance to `Printing`.
//! - Retry (FR-034): [`EngineError::UnsupportedSetting`] and
//!   [`EngineError::DependencyMissing`] are user-fixable, so the job is
//!   **Paused** with the error and never auto-retried — the user fixes the
//!   setting/driver and resumes. Any other error bumps `attempts`; the job gets
//!   the initial attempt plus three automatic retries (backoffs 1s/5s/25s); once
//!   `attempts >= MAX_ATTEMPTS` the job is **Failed**, otherwise it returns to
//!   the start of its stage (`Queued` for a conversion failure, `Converted` for
//!   a print failure — the cached PDF is kept) and a delayed `RetryDue` is
//!   scheduled after a [`RETRY_BACKOFFS`] backoff. Delayed jobs are held in a
//!   `delayed` set that the scheduler skips until the timer fires.
//! - Pause of a `Converting` job sets its cancel flag and moves it to `Paused`;
//!   the in-flight conversion aborts and its partial cache is discarded, so the
//!   job re-converts on resume. Pausing a `Printing` job is rejected
//!   (`InvalidState`) — the OS already owns it.
//! - Resume of a `Paused` job returns it to `Converted` when its cached PDF is
//!   still on disk, otherwise to `Queued`. The error is always cleared; a job
//!   paused by shutdown recovery (`interrupted_by_shutdown`) also has its
//!   `attempts` reset so it gets a clean run.
//! - Resume of a job paused with `printer_unavailable` **re-resolves the
//!   printer first** (`plan_printer_fix`). If the pinned printer is back, the
//!   job resumes onto it; if it is still gone but preferences now point
//!   somewhere usable — which is what the Printers screen's "Update references"
//!   reconciliation edits — the pin is dropped so dispatch follows them; and if
//!   nothing resolves, the resume is **refused** with what to fix instead of
//!   silently re-pausing. Without this a job pinned to a printer that
//!   disappeared was stuck forever: resuming re-ran the same failed lookup and
//!   nothing could re-point it.
//! - Cancel of a `Printing` job cannot un-spool synchronously (the OS job id is
//!   only known once `spool` returns), so the job is marked `Cancelled`
//!   immediately and the eventual `PrintDone` reconciles by cancelling the OS
//!   job and deleting the cache.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use crate::engine::print::{PrintError, SpoolOutcome};
use crate::engine::EngineError;
use crate::queue::job::{
    now_rfc3339, Job, JobError, JobProgress, JobState, QueueSnapshot, SCHEMA_VERSION,
};
use crate::queue::QueueState;
use crate::util::error::IpcError;

/// Default cap on conversions running concurrently, used when the config does
/// not override it (and as the value the `EngineCfg` default mirrors).
/// Conversions are CPU/IO heavy (rasterising images, parsing PDFs) so we cap the
/// pool rather than fan out. The effective cap comes from
/// [`DispatcherLimits::max_concurrent`] (config value clamped to `1..=8`).
pub(crate) const DEFAULT_MAX_CONCURRENT_CONVERSIONS: usize = 2;

/// Boot-time engine limits handed to the [`Dispatcher`]. Read once from
/// [`crate::settings::EngineCfg`] when the queue spawns; the running dispatcher
/// never re-reads them (engine settings apply on the next start).
#[derive(Debug, Clone, Copy)]
pub(crate) struct DispatcherLimits {
    /// Concurrent conversions allowed (already clamped to `1..=8`).
    pub max_concurrent: usize,
    /// Per-conversion wall-clock timeout; `None` disables it.
    pub conversion_timeout: Option<Duration>,
}

impl Default for DispatcherLimits {
    fn default() -> Self {
        Self {
            max_concurrent: DEFAULT_MAX_CONCURRENT_CONVERSIONS,
            conversion_timeout: None,
        }
    }
}

impl DispatcherLimits {
    /// Wall-clock ceiling for the print stage.
    ///
    /// Both blocking stages are bounded by the one engine timeout the user can
    /// actually set — [`crate::settings::EngineCfg::conversion_timeout_seconds`]
    /// (`0` disables both) — rather than the print stage inventing a magic
    /// constant of its own. Printing is a hand-off to the OS spooler and
    /// finishes in well under a conversion's budget, so the shared value is a
    /// ceiling for it, not a target.
    pub(crate) fn print_timeout(&self) -> Option<Duration> {
        self.conversion_timeout
    }
}

/// Failure count after which a transiently-failing job is marked `Failed`.
/// FR-034 grants the initial attempt plus three automatic retries, so a job
/// fails permanently on its fourth failure.
pub(crate) const MAX_ATTEMPTS: u32 = 4;

/// Backoff before each delayed retry, indexed by `attempts - 1`: all three
/// entries are used before the job fails on its [`MAX_ATTEMPTS`]th failure.
pub(crate) const RETRY_BACKOFFS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(25),
];

/// Minimum gap between progress emissions for a single job.
const PROGRESS_THROTTLE: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Commands & wire types
// ---------------------------------------------------------------------------

/// Successful print outcome flowing back from a blocking print task. Carries
/// the resolved printer name (so the job records which printer it actually went
/// to) alongside the spooler's own id.
pub(crate) struct PrintOk {
    pub printer: String,
    pub os_job_id: Option<String>,
}

/// Every message the dispatcher processes. User-facing commands carry a
/// [`oneshot`] reply so the async [`QueueHandle`](crate::queue::QueueHandle)
/// method can surface a per-job error; internal completions
/// (`ConversionDone`/`PrintDone`/`RetryDue`) are fire-and-forget.
pub(crate) enum Cmd {
    Submit {
        jobs: Vec<Job>,
    },
    Cancel {
        id: String,
        reply: oneshot::Sender<Result<(), IpcError>>,
    },
    CancelAll {
        reply: oneshot::Sender<u32>,
    },
    Pause {
        id: String,
        reply: oneshot::Sender<Result<(), IpcError>>,
    },
    Resume {
        id: String,
        reply: oneshot::Sender<Result<(), IpcError>>,
    },
    Retry {
        id: String,
        reply: oneshot::Sender<Result<(), IpcError>>,
    },
    PauseQueue,
    ResumeQueue,
    ClearDone {
        reply: oneshot::Sender<u32>,
    },
    ConversionDone {
        id: String,
        result: Result<Option<PathBuf>, EngineError>,
    },
    PrintDone {
        id: String,
        result: Result<PrintOk, PrintError>,
    },
    RetryDue {
        id: String,
    },
    /// A per-attempt conversion timer elapsed. `flag` is the exact cancel-flag
    /// [`Arc`] the timer captured, so a stale timer (from a prior attempt whose
    /// flag was since replaced) can be distinguished by pointer identity and
    /// ignored — see [`Dispatcher::on_conversion_timeout`].
    ConversionTimeout {
        id: String,
        flag: Arc<AtomicBool>,
    },
}

// ---------------------------------------------------------------------------
// Injected side-effect traits
// ---------------------------------------------------------------------------

/// The engine seam: everything the dispatcher needs the print engine to do.
/// Real impl is [`EngineRunner`]; tests fake it.
pub(crate) trait JobRunner: Send + Sync + 'static {
    /// Convert `job`'s input to a printable PDF (or `Ok(None)` for a PDF that
    /// prints in place). `cancel` is polled by the engine at stage boundaries.
    fn convert(
        &self,
        job: &Job,
        cancel: Arc<AtomicBool>,
        progress: Box<dyn Fn(f32) + Send + Sync>,
    ) -> Result<Option<PathBuf>, EngineError>;

    /// Spool the job's PDF to `printer`, waiting at most `timeout` for the
    /// spooler child (`None` waits forever).
    fn print(
        &self,
        job: &Job,
        printer: &str,
        timeout: Option<Duration>,
    ) -> Result<SpoolOutcome, PrintError>;

    /// Resolve the printer name to use and whether it supports duplex.
    ///
    /// `file_name` carries the input's name so a file-type routing rule can
    /// apply (see [`crate::ipc::printers::resolve_app_printer`]).
    fn resolve_printer(
        &self,
        requested: Option<&str>,
        file_name: &str,
    ) -> Result<(String, bool), PrintError>;

    /// Best-effort cancellation of an already-spooled OS job.
    fn cancel_os_job(&self, os_job_id: &str);
}

/// The event seam: how queue changes reach the webview. Real impl is
/// [`TauriEmit`]; tests collect into a vector.
pub(crate) trait EventSink: Send + Sync + 'static {
    fn queue_updated(&self, snapshot: &QueueSnapshot);
    fn job_progress(&self, id: &str, phase: &str, percent: f32);
}

/// The offload seam: how blocking convert/print closures are run. Real impl
/// hands them to [`tokio::task::spawn_blocking`]; tests run them inline for
/// determinism.
pub(crate) trait BlockingSpawner: Send + Sync + 'static {
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>);
}

// ---------------------------------------------------------------------------
// Real implementations
// ---------------------------------------------------------------------------

/// Production [`JobRunner`] backed by [`crate::engine`] and [`crate::platform`].
pub(crate) struct EngineRunner {
    /// Directory converted PDFs are written to (`<cache_dir>/conversions`).
    pub cache_dir: PathBuf,
}

impl JobRunner for EngineRunner {
    fn convert(
        &self,
        job: &Job,
        cancel: Arc<AtomicBool>,
        progress: Box<dyn Fn(f32) + Send + Sync>,
    ) -> Result<Option<PathBuf>, EngineError> {
        crate::engine::convert::convert_to_pdf(
            Path::new(&job.input_path),
            &self.cache_dir,
            &job.id,
            &cancel,
            &*progress,
        )
    }

    fn print(
        &self,
        job: &Job,
        printer: &str,
        timeout: Option<Duration>,
    ) -> Result<SpoolOutcome, PrintError> {
        // A converted job prints its cache PDF; a passthrough PDF prints in place.
        let pdf = match &job.cache_pdf {
            Some(p) => PathBuf::from(p),
            None => PathBuf::from(&job.input_path),
        };
        // Page ranges are PDF-only; an image is one page, so a range that
        // excludes page 1 would spool an empty job (see `effective_settings`).
        let settings = crate::engine::print::effective_settings(&job.settings, &job.input_ext);
        crate::engine::print::spool(&pdf, printer, &settings, &job.file_name, timeout)
    }

    /// Resolve the target printer and its duplex capability.
    ///
    /// A named printer that the OS no longer enumerates is **not** an error we
    /// let the spooler discover: the spec makes such a job paused, not failed,
    /// so the user can pick a replacement
    /// (`docs/03-requirements/print-settings.md` §printer). The check only bites
    /// when enumeration actually worked and returned something — a failed or
    /// empty enumeration (the platform backends are best-effort) keeps the old
    /// behaviour of trusting the name and letting spooling speak, so a flaky
    /// `lpstat` can never pause the whole queue.
    fn resolve_printer(
        &self,
        requested: Option<&str>,
        file_name: &str,
    ) -> Result<(String, bool), PrintError> {
        let name = match resolve_app_printer_for(requested, file_name) {
            Some(p) => p,
            None => resolve_os_default_printer()?,
        };
        let enumerated = crate::platform::list_printers()
            .ok()
            .filter(|list| !list.is_empty());
        let Some(list) = enumerated else {
            return Ok((name, true));
        };
        match list.iter().find(|p| p.id == name || p.name == name) {
            Some(p) => Ok((name, p.duplex)),
            None => Err(PrintError::PrinterMissing { printer: name }),
        }
    }

    fn cancel_os_job(&self, os_job_id: &str) {
        #[cfg(unix)]
        {
            if let Err(e) = crate::engine::print::cups::cancel(os_job_id) {
                tracing::warn!(event = "os_job_cancel_failed", os_job_id = %os_job_id, error = %e);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = os_job_id;
        }
    }
}

/// The MaeclyPrint-level choice for this input: an explicit request, then the
/// file-type routing rule, then the app default. Delegates to the single
/// definition of that order so the queue, the CLI and the GUI agree.
fn resolve_app_printer_for(requested: Option<&str>, file_name: &str) -> Option<String> {
    let prefs = crate::settings::Config::load_or_default().printers;
    crate::ipc::printers::resolve_app_printer(&prefs, requested, file_name)
}

/// The OS default, used only once nothing at the app level applies. Having no
/// printer at all is user-fixable (pick one), so the caller pauses the job
/// rather than burning retries.
fn resolve_os_default_printer() -> Result<String, PrintError> {
    crate::platform::default_printer()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .ok_or(PrintError::NoPrinter)
}

/// Production [`EventSink`] emitting typed `tauri-specta` events to the webview.
pub(crate) struct TauriEmit {
    pub app: tauri::AppHandle,
}

impl EventSink for TauriEmit {
    fn queue_updated(&self, snapshot: &QueueSnapshot) {
        use tauri_specta::Event;
        if let Err(e) = crate::ipc::events::QueueUpdated(snapshot.clone()).emit(&self.app) {
            tracing::warn!(event = "queue_updated_emit_failed", error = %e);
        }
    }

    fn job_progress(&self, id: &str, phase: &str, percent: f32) {
        use tauri_specta::Event;
        let _ = crate::ipc::events::JobProgressEvent {
            id: id.to_string(),
            phase: phase.to_string(),
            percent,
        }
        .emit(&self.app);
    }
}

/// Production [`BlockingSpawner`].
pub(crate) struct TokioBlockingSpawner;

impl BlockingSpawner for TokioBlockingSpawner {
    fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
        tokio::task::spawn_blocking(task);
    }
}

// ---------------------------------------------------------------------------
// Post-lock side effects
// ---------------------------------------------------------------------------

/// Side effects a handler wants to run *after* releasing the state lock — file
/// deletes, OS-job cancels, delayed-retry scheduling. Collecting them keeps the
/// lock scopes tiny and avoids re-entrant locking when the inline test spawner
/// runs work synchronously.
#[derive(Default)]
struct Post {
    delete_cache: Vec<String>,
    cancel_os: Vec<String>,
    remove_delayed: Vec<String>,
    retry: Option<(String, Duration)>,
}

/// What resuming a job should do about the printer it is pinned to.
///
/// Only produced for a job paused with `printer_unavailable`; see
/// [`Dispatcher::plan_printer_fix`]. The fourth outcome — nothing usable at all
/// — is not a variant: it is an [`IpcError::InvalidState`] carrying what the
/// user has to fix, so resume *refuses* instead of quietly re-pausing.
enum PrinterFix {
    /// Not a printer problem; resume as usual.
    NotNeeded,
    /// The job's own printer resolves again.
    Unchanged,
    /// `missing` is still gone, but the app-level chain now points at
    /// `replacement`: drop the pin so this run follows the preferences.
    Unpin {
        missing: String,
        replacement: String,
    },
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// The single-writer state machine. Constructed by
/// [`crate::queue::spawn`] and driven by [`run`].
pub(crate) struct Dispatcher {
    state: Arc<RwLock<QueueState>>,
    self_tx: mpsc::UnboundedSender<Cmd>,
    runner: Arc<dyn JobRunner>,
    emit: Arc<dyn EventSink>,
    spawner: Arc<dyn BlockingSpawner>,
    queue_file: PathBuf,
    /// Boot-time engine limits (concurrency cap + conversion timeout).
    limits: DispatcherLimits,
    /// Cancellation flags for in-flight conversions, keyed by job id.
    converting: HashMap<String, Arc<AtomicBool>>,
    /// Jobs waiting out a retry backoff; the scheduler skips these.
    delayed: HashSet<String>,
    /// Last progress-emit instant per job, for throttling. Shared into the
    /// progress closures handed to the runner.
    progress_last: Arc<Mutex<HashMap<String, Instant>>>,
    /// Whether the last `queue.json` write failed, so persistence trouble is
    /// logged loudly once per streak instead of on every transition.
    persist_failing: bool,
}

impl Dispatcher {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        state: Arc<RwLock<QueueState>>,
        self_tx: mpsc::UnboundedSender<Cmd>,
        runner: Arc<dyn JobRunner>,
        emit: Arc<dyn EventSink>,
        spawner: Arc<dyn BlockingSpawner>,
        queue_file: PathBuf,
        limits: DispatcherLimits,
    ) -> Self {
        Self {
            state,
            self_tx,
            runner,
            emit,
            spawner,
            queue_file,
            limits,
            converting: HashMap::new(),
            delayed: HashSet::new(),
            progress_last: Arc::new(Mutex::new(HashMap::new())),
            persist_failing: false,
        }
    }

    /// Handle one command, then (re)schedule and persist/emit the result. This
    /// is the unit the run loop and the tests both drive.
    pub(crate) fn dispatch_one(&mut self, cmd: Cmd) {
        self.handle(cmd);
        self.schedule();
        self.commit();
    }

    /// Kick the pipeline for jobs recovered from disk, then persist/emit the
    /// starting snapshot.
    pub(crate) fn bootstrap(&mut self) {
        self.schedule();
        self.commit();
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Submit { jobs } => self.on_submit(jobs),
            Cmd::Cancel { id, reply } => {
                let _ = reply.send(self.on_cancel(&id));
            }
            Cmd::CancelAll { reply } => {
                let _ = reply.send(self.on_cancel_all());
            }
            Cmd::Pause { id, reply } => {
                let _ = reply.send(self.on_pause(&id));
            }
            Cmd::Resume { id, reply } => {
                let _ = reply.send(self.on_resume(&id));
            }
            Cmd::Retry { id, reply } => {
                let _ = reply.send(self.on_retry(&id));
            }
            Cmd::PauseQueue => self.set_queue_paused(true),
            Cmd::ResumeQueue => self.set_queue_paused(false),
            Cmd::ClearDone { reply } => {
                let _ = reply.send(self.on_clear_done());
            }
            Cmd::ConversionDone { id, result } => self.on_conversion_done(id, result),
            Cmd::PrintDone { id, result } => self.on_print_done(id, result),
            Cmd::RetryDue { id } => {
                self.delayed.remove(&id);
            }
            Cmd::ConversionTimeout { id, flag } => self.on_conversion_timeout(id, flag),
        }
    }

    fn on_submit(&mut self, jobs: Vec<Job>) {
        let mut st = self.state.write().unwrap();
        for job in jobs {
            let f = JobLog::of(&job);
            // `input_path` appears here and nowhere else: FR-035 wants the file
            // path recorded once, and NFR-082 keeps absolute paths out of
            // WARN/ERROR records, which carry `file` (the basename) instead.
            tracing::info!(
                event = "job_submitted",
                job_id = %job.id,
                file = %f.file,
                input_path = %job.input_path,
                profile = %f.profile,
                printer = %f.printer,
                pages = f.pages,
                status = %f.status,
                submitted_at = %job.submitted_at
            );
            st.jobs.push(job);
        }
    }

    fn on_cancel(&mut self, id: &str) -> Result<(), IpcError> {
        let mut post = Post::default();
        let result = {
            let mut st = self.state.write().unwrap();
            match st.jobs.iter_mut().find(|j| j.id == id) {
                None => Err(IpcError::JobNotFound(id.to_string())),
                Some(job) if job.state.is_terminal() => Err(IpcError::InvalidState(format!(
                    "job {id} has already finished"
                ))),
                Some(job) => {
                    cancel_job_in_place(job, &self.converting, &mut post);
                    post.remove_delayed.push(id.to_string());
                    Ok(())
                }
            }
        };
        self.apply_post(post);
        if result.is_ok() {
            tracing::info!(event = "job_cancelled", job_id = %id);
        }
        result
    }

    fn on_cancel_all(&mut self) -> u32 {
        let mut post = Post::default();
        let count;
        {
            let mut st = self.state.write().unwrap();
            let ids: Vec<String> = st
                .jobs
                .iter()
                .filter(|j| !j.state.is_terminal())
                .map(|j| j.id.clone())
                .collect();
            count = ids.len() as u32;
            for job in st.jobs.iter_mut().filter(|j| !j.state.is_terminal()) {
                cancel_job_in_place(job, &self.converting, &mut post);
            }
            post.remove_delayed = ids;
        }
        self.apply_post(post);
        if count > 0 {
            tracing::info!(event = "queue_cancel_all", count);
        }
        count
    }

    fn on_pause(&mut self, id: &str) -> Result<(), IpcError> {
        let result = {
            let mut st = self.state.write().unwrap();
            match st.jobs.iter_mut().find(|j| j.id == id) {
                None => Err(IpcError::JobNotFound(id.to_string())),
                Some(job) => match job.state {
                    JobState::Queued | JobState::Converted => {
                        job.state = JobState::Paused;
                        job.progress = None;
                        Ok(())
                    }
                    JobState::Converting => {
                        if let Some(flag) = self.converting.get(&job.id) {
                            flag.store(true, Ordering::Relaxed);
                        }
                        job.state = JobState::Paused;
                        job.progress = None;
                        Ok(())
                    }
                    // Idempotent: pausing an already-paused job is a no-op.
                    JobState::Paused => Ok(()),
                    JobState::Printing => Err(IpcError::InvalidState(
                        "cannot pause a job that is already printing".to_string(),
                    )),
                    JobState::Done | JobState::Failed | JobState::Cancelled => Err(
                        IpcError::InvalidState(format!("job {id} has already finished")),
                    ),
                },
            }
        };
        if result.is_ok() {
            tracing::info!(event = "job_paused", job_id = %id);
        }
        result
    }

    /// Decide what resuming `id` should do about its printer, *before* the
    /// state is touched.
    ///
    /// Only jobs paused with `printer_unavailable` are re-resolved; everything
    /// else resumes unchanged. The check runs outside the state lock because
    /// resolution reads the OS printer list, and the write guard is shared with
    /// [`QueueHandle::snapshot`](crate::queue::QueueHandle::snapshot).
    ///
    /// The resolution asked for is the *same* one dispatch will do
    /// ([`JobRunner::resolve_printer`]), so a resume can never succeed here and
    /// then fail there for the same reason.
    fn plan_printer_fix(&self, id: &str) -> Result<PrinterFix, IpcError> {
        let (requested, file_name) = {
            let st = self.state.read().unwrap();
            let Some(job) = st.jobs.iter().find(|j| j.id == id) else {
                return Ok(PrinterFix::NotNeeded); // `on_resume` reports it
            };
            let stuck_on_printer = job.state == JobState::Paused
                && job
                    .error
                    .as_ref()
                    .is_some_and(|e| e.code == "printer_unavailable");
            if !stuck_on_printer {
                return Ok(PrinterFix::NotNeeded);
            }
            (job.settings.printer.clone(), job.file_name.clone())
        };

        // The printer came back (reinstalled, renamed back, spooler restarted).
        if self
            .runner
            .resolve_printer(requested.as_deref(), &file_name)
            .is_ok()
        {
            return Ok(PrinterFix::Unchanged);
        }

        let Some(missing) = requested else {
            // Nothing was pinned, so there is simply no printer to print on.
            return Err(IpcError::InvalidState(
                "no printer is configured — add one, or set a default on the Printers screen, then resume this job".to_string(),
            ));
        };

        // The job is pinned to a printer the OS no longer has. Ask what the
        // *preferences* now say for this file (routing rule, then app default,
        // then the OS default) — which is exactly what the Printers screen's
        // "Update references" reconciliation edits.
        match self.runner.resolve_printer(None, &file_name) {
            Ok((replacement, _)) => Ok(PrinterFix::Unpin {
                missing,
                replacement,
            }),
            Err(_) => Err(IpcError::InvalidState(format!(
                "printer '{missing}' is still not installed and nothing else is configured — reinstall it, or reassign it on the Printers screen, then resume this job"
            ))),
        }
    }

    fn on_resume(&mut self, id: &str) -> Result<(), IpcError> {
        // A job paused with `printer_unavailable` used to resume straight back
        // into the same failure with no way out: its printer was pinned in
        // `settings.printer`, that queue was gone, and nothing could re-point
        // it. Resuming now re-runs the real resolution first, so it either
        // succeeds, follows the reconciled preferences, or says why not.
        let fix = self.plan_printer_fix(id)?;

        let mut post = Post::default();
        let result = {
            let mut st = self.state.write().unwrap();
            match st.jobs.iter_mut().find(|j| j.id == id) {
                None => Err(IpcError::JobNotFound(id.to_string())),
                Some(job) if job.state != JobState::Paused => Err(IpcError::InvalidState(
                    "only a paused job can be resumed".to_string(),
                )),
                Some(job) => {
                    let cache_valid = job
                        .cache_pdf
                        .as_deref()
                        .map(|p| Path::new(p).exists())
                        .unwrap_or(false);
                    let interrupted = job
                        .error
                        .as_ref()
                        .map(|e| e.code == "interrupted_by_shutdown")
                        .unwrap_or(false);
                    job.error = None;
                    if interrupted {
                        job.attempts = 0;
                    }
                    if let PrinterFix::Unpin {
                        missing,
                        replacement,
                    } = &fix
                    {
                        // Drop the pin rather than writing `replacement` into
                        // the job: dispatch resolves the printer itself, and a
                        // name copied in here would go stale all over again.
                        job.settings.printer = None;
                        job.printer_resolved = None;
                        tracing::info!(
                            event = "job_printer_reassigned",
                            job_id = %job.id,
                            file = %job.file_name,
                            missing_printer = %missing,
                            printer = %replacement,
                        );
                    }
                    job.state = if cache_valid {
                        JobState::Converted
                    } else {
                        JobState::Queued
                    };
                    job.progress = None;
                    post.remove_delayed.push(id.to_string());
                    Ok(())
                }
            }
        };
        self.apply_post(post);
        if result.is_ok() {
            tracing::info!(event = "job_resumed", job_id = %id);
        }
        result
    }

    fn on_retry(&mut self, id: &str) -> Result<(), IpcError> {
        let result = {
            let mut st = self.state.write().unwrap();
            match st.jobs.iter_mut().find(|j| j.id == id) {
                None => Err(IpcError::JobNotFound(id.to_string())),
                Some(job) if job.state != JobState::Failed => Err(IpcError::InvalidState(
                    "only a failed job can be retried".to_string(),
                )),
                Some(job) => {
                    job.state = JobState::Queued;
                    job.attempts = 0;
                    job.error = None;
                    job.finished_at = None;
                    job.progress = None;
                    Ok(())
                }
            }
        };
        if result.is_ok() {
            tracing::info!(event = "job_retry", job_id = %id);
        }
        result
    }

    fn set_queue_paused(&mut self, paused: bool) {
        self.state.write().unwrap().queue_paused = paused;
        tracing::info!(
            event = if paused {
                "queue_paused"
            } else {
                "queue_resumed"
            }
        );
    }

    fn on_clear_done(&mut self) -> u32 {
        let mut post = Post::default();
        let count;
        {
            let mut st = self.state.write().unwrap();
            let before = st.jobs.len();
            for job in st.jobs.iter().filter(|j| j.state.is_terminal()) {
                if let Some(c) = &job.cache_pdf {
                    post.delete_cache.push(c.clone());
                }
            }
            st.jobs.retain(|j| !j.state.is_terminal());
            count = (before - st.jobs.len()) as u32;
        }
        self.apply_post(post);
        count
    }

    fn on_conversion_done(&mut self, id: String, result: Result<Option<PathBuf>, EngineError>) {
        // The conversion is no longer in flight regardless of outcome. Dropping
        // the throttle entry too keeps `progress_last` bounded by the number of
        // conversions actually in flight.
        self.converting.remove(&id);
        self.progress_last.lock().unwrap().remove(&id);
        let mut post = Post::default();
        {
            let mut st = self.state.write().unwrap();
            match st.jobs.iter_mut().find(|j| j.id == id) {
                None => {
                    // Job was cleared while converting; discard any file it produced.
                    if let Ok(Some(p)) = &result {
                        post.delete_cache.push(p.to_string_lossy().into_owned());
                    }
                }
                Some(job) => match job.state {
                    JobState::Converting => match result {
                        Ok(cache) => {
                            job.cache_pdf =
                                cache.as_ref().map(|p| p.to_string_lossy().into_owned());
                            job.state = JobState::Converted;
                            job.error = None;
                            job.progress = None;
                            tracing::info!(
                                event = "job_converted",
                                job_id = %id,
                                passthrough = cache.is_none()
                            );
                        }
                        // A cancel flag without a matching state change should
                        // not happen; requeue without penalty just in case.
                        Err(EngineError::Cancelled) => {
                            job.state = JobState::Queued;
                            job.progress = None;
                        }
                        Err(e) => record_failure(job, &e, true, &mut post),
                    },
                    // Cancelled/paused mid-conversion, or failed by a timeout
                    // that set this attempt's cancel flag: drop whatever partial
                    // cache the aborted conversion produced. The `converting`
                    // entry was already removed at the top of this handler.
                    JobState::Cancelled | JobState::Paused | JobState::Failed => {
                        if let Ok(Some(p)) = &result {
                            post.delete_cache.push(p.to_string_lossy().into_owned());
                        }
                        job.progress = None;
                    }
                    // Late/duplicate completion for a job that moved on.
                    _ => {
                        if let Ok(Some(p)) = &result {
                            post.delete_cache.push(p.to_string_lossy().into_owned());
                        }
                    }
                },
            }
        }
        self.apply_post(post);
    }

    /// A per-attempt conversion timer fired. Fail the job **only** if this timer
    /// still refers to the live conversion: the job must still be
    /// [`JobState::Converting`] and the cancel flag registered for it must be the
    /// exact [`Arc`] this timer captured. A pause/resume or a retry swaps in a
    /// fresh flag, so a stale timer from an earlier attempt is a no-op — this is
    /// what prevents a timer armed for attempt *N* from killing attempt *N+1*.
    ///
    /// On a real timeout: set the flag (so the running conversion aborts at its
    /// next stage boundary), mark the job [`JobState::Failed`] with the
    /// `"conversion_timeout"` code (terminal — no automatic retry), and drop any
    /// cached PDF. The eventual [`Cmd::ConversionDone`] for the aborted attempt
    /// finds a `Failed` job and discards its partial output (see
    /// [`Dispatcher::on_conversion_done`]).
    ///
    /// Waiting window: the job's id stays in `converting` until that
    /// stale `ConversionDone` arrives — the orphaned conversion keeps running
    /// (we can only *flag* a blocking task, not kill it). So if the user hits
    /// Retry before the orphan reports back, the job returns to `Queued` but the
    /// scheduler skips it (it's still in `converting`, see [`Dispatcher::schedule`])
    /// until the orphan finishes. The retry is delayed by that window, not lost.
    fn on_conversion_timeout(&mut self, id: String, flag: Arc<AtomicBool>) {
        // A stale flag identifies a timer whose attempt is already gone.
        let still_current = self
            .converting
            .get(&id)
            .map(|f| Arc::ptr_eq(f, &flag))
            .unwrap_or(false);
        if !still_current {
            return;
        }

        let timeout_secs = self
            .limits
            .conversion_timeout
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut post = Post::default();
        {
            let mut st = self.state.write().unwrap();
            if let Some(job) = st.jobs.iter_mut().find(|j| j.id == id) {
                if job.state == JobState::Converting {
                    flag.store(true, Ordering::Relaxed);
                    job.state = JobState::Failed;
                    job.error = Some(JobError::new(
                        "conversion_timeout",
                        format!("conversion timed out after {timeout_secs}s"),
                    ));
                    job.finished_at = Some(now_rfc3339());
                    job.progress = None;
                    if let Some(c) = job.cache_pdf.take() {
                        post.delete_cache.push(c);
                    }
                    let f = JobLog::of(job);
                    tracing::warn!(
                        event = "job_conversion_timeout",
                        job_id = %id,
                        file = %f.file,
                        profile = %f.profile,
                        printer = %f.printer,
                        pages = f.pages,
                        status = %f.status,
                        started_at = %f.started_at,
                        finished_at = %f.finished_at,
                        error = %f.error,
                        timeout_s = timeout_secs
                    );
                }
            }
        }
        self.apply_post(post);
    }

    fn on_print_done(&mut self, id: String, result: Result<PrintOk, PrintError>) {
        let mut post = Post::default();
        {
            let mut st = self.state.write().unwrap();
            match st.jobs.iter_mut().find(|j| j.id == id) {
                None => {
                    // Job cleared after spool; cancel it at the OS if it landed.
                    if let Ok(ok) = &result {
                        if let Some(oid) = &ok.os_job_id {
                            post.cancel_os.push(oid.clone());
                        }
                    }
                }
                Some(job) => match job.state {
                    JobState::Printing => match result {
                        Ok(ok) => {
                            job.state = JobState::Done;
                            job.finished_at = Some(now_rfc3339());
                            job.printer_resolved = Some(ok.printer);
                            job.progress = None;
                            job.error = None;
                            if let Some(c) = job.cache_pdf.take() {
                                post.delete_cache.push(c);
                            }
                            let f = JobLog::of(job);
                            tracing::info!(
                                event = "job_done",
                                job_id = %id,
                                file = %f.file,
                                profile = %f.profile,
                                printer = %f.printer,
                                pages = f.pages,
                                status = %f.status,
                                started_at = %f.started_at,
                                finished_at = %f.finished_at
                            );
                        }
                        Err(e) => record_print_failure(job, e, &mut post),
                    },
                    // Cancelled during the brief spool window: reconcile.
                    JobState::Cancelled => {
                        if let Ok(ok) = &result {
                            if let Some(oid) = &ok.os_job_id {
                                post.cancel_os.push(oid.clone());
                            }
                        }
                        if let Some(c) = job.cache_pdf.take() {
                            post.delete_cache.push(c);
                        }
                    }
                    _ => {}
                },
            }
        }
        self.apply_post(post);
    }

    /// Run post-lock side effects: file deletes, OS cancels, delayed-retry
    /// scheduling. Called with the state lock released.
    fn apply_post(&mut self, post: Post) {
        for path in post.delete_cache {
            delete_cache_file(&path);
        }
        for oid in post.cancel_os {
            self.runner.cancel_os_job(&oid);
        }
        for id in post.remove_delayed {
            self.delayed.remove(&id);
        }
        if let Some((id, delay)) = post.retry {
            self.delayed.insert(id.clone());
            let tx = self.self_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(Cmd::RetryDue { id });
            });
        }
    }

    /// Start as much new work as the concurrency limits and pause state allow.
    /// Mutates job states under a short lock, then spawns the blocking work with
    /// the lock released.
    fn schedule(&mut self) {
        let mut to_convert: Vec<(Job, Arc<AtomicBool>)> = Vec::new();
        let mut to_print: Option<Job> = None;
        {
            let mut st = self.state.write().unwrap();
            if !st.queue_paused {
                // Count the conversions actually in flight, not the jobs in the
                // Converting state: a paused, cancelled or timed-out conversion
                // is only *flagged* and keeps burning CPU until its next stage
                // boundary, so it holds its slot until `ConversionDone` removes
                // it from `self.converting` (FR-037).
                let mut converting = self.converting.len();
                while converting < self.limits.max_concurrent {
                    // A Queued job whose id is still in `self.converting` was
                    // paused/resumed — or retried after a conversion timeout —
                    // while its previous conversion is still aborting; starting a
                    // second conversion would race both onto the same cache path.
                    // It becomes eligible again once the stale ConversionDone
                    // arrives and removes the entry.
                    //
                    // This is the *waiting window* for a retried
                    // timed-out job. `on_conversion_timeout` marks the job Failed
                    // but leaves its orphaned conversion running (we can't kill a
                    // blocking task, only flag it). A user Retry moves the job
                    // back to Queued, yet it is skipped here until the orphan
                    // reports back and clears `converting`. It is delayed, never
                    // stuck forever — the orphan finishes (or the flag aborts it
                    // at the next stage boundary) and the retry then proceeds.
                    let Some(idx) = st.jobs.iter().position(|j| {
                        j.state == JobState::Queued
                            && !self.delayed.contains(&j.id)
                            && !self.converting.contains_key(&j.id)
                    }) else {
                        break;
                    };
                    let flag = Arc::new(AtomicBool::new(false));
                    {
                        let job = &mut st.jobs[idx];
                        job.state = JobState::Converting;
                        if job.started_at.is_none() {
                            job.started_at = Some(now_rfc3339());
                        }
                        job.progress = Some(JobProgress::measured("converting", 0.0));
                        job.error = None;
                        let f = JobLog::of(job);
                        tracing::info!(
                            event = "job_start_convert",
                            job_id = %job.id,
                            file = %f.file,
                            profile = %f.profile,
                            printer = %f.printer,
                            pages = f.pages,
                            status = %f.status,
                            started_at = %f.started_at
                        );
                    }
                    self.converting
                        .insert(st.jobs[idx].id.clone(), flag.clone());
                    to_convert.push((st.jobs[idx].clone(), flag));
                    converting += 1;
                }

                let printing = st.jobs.iter().any(|j| j.state == JobState::Printing);
                if !printing {
                    if let Some(idx) = st.jobs.iter().position(|j| {
                        j.state == JobState::Converted && !self.delayed.contains(&j.id)
                    }) {
                        let job = &mut st.jobs[idx];
                        job.state = JobState::Printing;
                        if job.started_at.is_none() {
                            job.started_at = Some(now_rfc3339());
                        }
                        // The spooler reports no completion fraction, so the
                        // printing phase is explicitly indeterminate rather than
                        // a bar frozen at 0 %.
                        job.progress = Some(JobProgress::indeterminate("printing"));
                        job.error = None;
                        let f = JobLog::of(job);
                        tracing::info!(
                            event = "job_start_print",
                            job_id = %job.id,
                            file = %f.file,
                            profile = %f.profile,
                            printer = %f.printer,
                            pages = f.pages,
                            status = %f.status,
                            started_at = %f.started_at
                        );
                        to_print = Some(st.jobs[idx].clone());
                    }
                }
            }
        }
        for (job, flag) in to_convert {
            self.spawn_convert(job, flag);
        }
        if let Some(job) = to_print {
            self.spawn_print(job);
        }
    }

    fn spawn_convert(&self, job: Job, flag: Arc<AtomicBool>) {
        let runner = self.runner.clone();
        let self_tx = self.self_tx.clone();
        let progress = self.make_progress(job.id.clone());
        let id = job.id.clone();
        // Arm a timeout for THIS attempt, capturing its exact cancel-flag Arc so
        // the handler can tell it apart from a later attempt's flag by identity.
        if let Some(timeout) = self.limits.conversion_timeout {
            let tx = self.self_tx.clone();
            let timer_id = id.clone();
            let timer_flag = flag.clone();
            tokio::spawn(async move {
                tokio::time::sleep(timeout).await;
                let _ = tx.send(Cmd::ConversionTimeout {
                    id: timer_id,
                    flag: timer_flag,
                });
            });
        }
        self.spawner.spawn_blocking(Box::new(move || {
            let result = runner.convert(&job, flag, progress);
            let _ = self_tx.send(Cmd::ConversionDone { id, result });
        }));
    }

    fn spawn_print(&self, job: Job) {
        let runner = self.runner.clone();
        let self_tx = self.self_tx.clone();
        let id = job.id.clone();
        // The whole print stage is bounded, so a wedged spooler cannot hold the
        // queue's single print slot forever (the conversion stage has had the
        // equivalent bound since the start — see `Cmd::ConversionTimeout`).
        let timeout = self.limits.print_timeout();
        self.spawner.spawn_blocking(Box::new(move || {
            let result = (|| -> Result<PrintOk, PrintError> {
                let (name, duplex_supported) =
                    runner.resolve_printer(job.settings.printer.as_deref(), &job.file_name)?;
                // Pure capability gate (honesty policy): a duplex request against
                // a non-duplex driver is an error rather than a silent downgrade.
                crate::engine::print::check_supported(
                    &job.settings,
                    &name,
                    duplex_supported,
                    true,
                )?;
                let outcome = runner.print(&job, &name, timeout)?;
                Ok(PrintOk {
                    printer: name,
                    os_job_id: outcome.os_job_id,
                })
            })();
            let _ = self_tx.send(Cmd::PrintDone { id, result });
        }));
    }

    /// Build the throttled progress callback for a converting job: it updates
    /// `job.progress` in memory (no persist) and emits a `JobProgressEvent`.
    fn make_progress(&self, job_id: String) -> Box<dyn Fn(f32) + Send + Sync> {
        let state = self.state.clone();
        let emit = self.emit.clone();
        let last = self.progress_last.clone();
        Box::new(move |percent: f32| {
            {
                let mut guard = last.lock().unwrap();
                let now = Instant::now();
                match guard.get(&job_id) {
                    Some(prev) if now.duration_since(*prev) < PROGRESS_THROTTLE => return,
                    _ => {
                        guard.insert(job_id.clone(), now);
                    }
                }
            }
            {
                let mut st = state.write().unwrap();
                if let Some(job) = st.jobs.iter_mut().find(|j| j.id == job_id) {
                    if job.state == JobState::Converting {
                        job.progress = Some(JobProgress::measured("converting", percent));
                    }
                }
            }
            emit.job_progress(&job_id, "converting", percent);
        })
    }

    fn commit(&mut self) {
        let snapshot = self.snapshot();
        match crate::queue::persist::save(&self.queue_file, &snapshot) {
            Ok(()) => {
                if self.persist_failing {
                    self.persist_failing = false;
                    tracing::info!(event = "queue_persist_recovered", file = %self.queue_file.display());
                }
            }
            Err(e) => {
                // Durability is silently void while this persists (full disk,
                // permissions); log loudly once per failure streak so the log
                // isn't spammed on every transition but the problem is visible.
                if self.persist_failing {
                    tracing::warn!(event = "queue_persist_failed", error = %e);
                } else {
                    self.persist_failing = true;
                    tracing::error!(
                        event = "queue_persist_failed",
                        file = %self.queue_file.display(),
                        error = %e,
                        "queue.json could not be written — job state will not survive a restart"
                    );
                }
            }
        }
        self.emit.queue_updated(&snapshot);
    }

    fn snapshot(&self) -> QueueSnapshot {
        let st = self.state.read().unwrap();
        QueueSnapshot {
            schema_version: SCHEMA_VERSION,
            queue_paused: st.queue_paused,
            jobs: st.jobs.clone(),
        }
    }
}

/// The run loop: bootstrap recovered jobs, then process commands until every
/// [`QueueHandle`](crate::queue::QueueHandle) sender is dropped.
pub(crate) async fn run(mut dispatcher: Dispatcher, mut rx: mpsc::UnboundedReceiver<Cmd>) {
    dispatcher.bootstrap();
    while let Some(cmd) = rx.recv().await {
        dispatcher.dispatch_one(cmd);
    }
    tracing::info!(event = "queue_dispatcher_stopped");
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Apply cancellation to a single job in place, recording any cache file to
/// delete. `Converting` jobs get their cancel flag set and clean up their
/// partial cache when the conversion reports back; `Printing` jobs reconcile at
/// `PrintDone`.
fn cancel_job_in_place(
    job: &mut Job,
    converting: &HashMap<String, Arc<AtomicBool>>,
    post: &mut Post,
) {
    match job.state {
        JobState::Converting => {
            if let Some(flag) = converting.get(&job.id) {
                flag.store(true, Ordering::Relaxed);
            }
        }
        JobState::Queued | JobState::Converted | JobState::Paused => {
            if let Some(c) = job.cache_pdf.take() {
                post.delete_cache.push(c);
            }
        }
        JobState::Printing => { /* os id unknown yet; reconciled at PrintDone */ }
        _ => {}
    }
    job.state = JobState::Cancelled;
    job.finished_at = Some(now_rfc3339());
    job.progress = None;
}

/// The FR-035 fields of a job, borrowed for one `tracing` record.
///
/// FR-035 requires every job to be logged with its start time, end time, file,
/// profile, printer, page count, status and error. Those all live on [`Job`]
/// already, so this is purely the rendering: it replaces the `Option`s with the
/// placeholders the log schema uses, so a query for `printer` never has to cope
/// with a missing key. The `tracing` macros still name every field at each call
/// site (they take literals, not a spliced struct); this only guarantees the
/// *values* are derived identically everywhere.
struct JobLog<'a> {
    file: &'a str,
    profile: &'a str,
    printer: &'a str,
    /// Page count, or `-1` when it is not known yet (a numeric field stays
    /// queryable; `"unknown"` in a number column would not).
    pages: i64,
    status: &'a str,
    started_at: &'a str,
    finished_at: &'a str,
    error: &'a str,
}

impl<'a> JobLog<'a> {
    fn of(job: &'a Job) -> Self {
        Self {
            // The basename, never the absolute path: WARN/ERROR records must
            // not carry full paths (NFR-082, data-model.md §5.1).
            file: &job.file_name,
            profile: job.profile_name.as_deref().unwrap_or("<none>"),
            printer: job
                .printer_resolved
                .as_deref()
                .or(job.settings.printer.as_deref())
                .unwrap_or("<default>"),
            pages: job.pages.map(i64::from).unwrap_or(-1),
            status: job.state.as_str(),
            started_at: job.started_at.as_deref().unwrap_or(""),
            finished_at: job.finished_at.as_deref().unwrap_or(""),
            error: job.error.as_ref().map(|e| e.code.as_str()).unwrap_or(""),
        }
    }
}

/// Apply the print-stage failure policy to `job`.
///
/// Two print outcomes are not ordinary transient failures:
///
/// - a **timeout** is terminal, exactly like `conversion_timeout`: the spooler
///   child was killed, and re-spooling would most likely wedge again, so the job
///   is `Failed` with `print_timeout` and no retry; and
/// - a **missing / unconfigured printer** is user-fixable, so the job is
///   `Paused` with `printer_unavailable` for the user to choose a replacement
///   (`docs/03-requirements/print-settings.md` §printer).
///
/// Everything else is an [`EngineError`] and follows the ordinary FR-034 retry
/// policy in [`record_failure`].
fn record_print_failure(job: &mut Job, e: PrintError, post: &mut Post) {
    let (code, terminal) = match &e {
        PrintError::TimedOut { .. } => ("print_timeout", true),
        PrintError::PrinterMissing { .. } | PrintError::NoPrinter => ("printer_unavailable", false),
        PrintError::Engine(engine) => return record_failure(job, engine, false, post),
    };

    job.error = Some(JobError::new(code, e.to_string()));
    job.progress = None;
    if terminal {
        job.state = JobState::Failed;
        job.finished_at = Some(now_rfc3339());
        if let Some(c) = job.cache_pdf.take() {
            post.delete_cache.push(c);
        }
    } else {
        // Keep the cached PDF: resuming onto another printer must not re-convert.
        job.state = JobState::Paused;
    }

    let f = JobLog::of(job);
    tracing::warn!(
        event = if terminal {
            "job_print_timeout"
        } else {
            "job_paused_needs_fix"
        },
        job_id = %job.id,
        file = %f.file,
        profile = %f.profile,
        printer = %f.printer,
        pages = f.pages,
        status = %f.status,
        started_at = %f.started_at,
        finished_at = %f.finished_at,
        attempts = job.attempts,
        error = %f.error,
        message = %e
    );
}

/// Apply the FR-034 failure policy to `job`, recording any follow-up in `post`.
fn record_failure(job: &mut Job, e: &EngineError, is_conversion: bool, post: &mut Post) {
    match e {
        // User-fixable: pause and wait for the user, no automatic retry.
        EngineError::UnsupportedSetting { .. } | EngineError::DependencyMissing(_) => {
            job.state = JobState::Paused;
            job.error = Some(to_job_error(e));
            job.progress = None;
            let f = JobLog::of(job);
            tracing::warn!(
                event = "job_paused_needs_fix",
                job_id = %job.id,
                file = %f.file,
                profile = %f.profile,
                printer = %f.printer,
                pages = f.pages,
                status = %f.status,
                started_at = %f.started_at,
                finished_at = %f.finished_at,
                attempts = job.attempts,
                error = %f.error,
                message = %e
            );
        }
        // Cancellation is handled by the cancel path; defensive here.
        EngineError::Cancelled => {
            job.state = JobState::Cancelled;
            job.finished_at = Some(now_rfc3339());
            job.progress = None;
            if let Some(c) = job.cache_pdf.take() {
                post.delete_cache.push(c);
            }
        }
        _ => {
            job.attempts += 1;
            job.error = Some(to_job_error(e));
            job.progress = None;
            if job.attempts >= MAX_ATTEMPTS {
                job.state = JobState::Failed;
                job.finished_at = Some(now_rfc3339());
                if let Some(c) = job.cache_pdf.take() {
                    post.delete_cache.push(c);
                }
                let f = JobLog::of(job);
                tracing::warn!(
                    event = "job_failed",
                    job_id = %job.id,
                    file = %f.file,
                    profile = %f.profile,
                    printer = %f.printer,
                    pages = f.pages,
                    status = %f.status,
                    started_at = %f.started_at,
                    finished_at = %f.finished_at,
                    attempts = job.attempts,
                    error = %f.error,
                    message = %e
                );
            } else {
                job.state = if is_conversion {
                    JobState::Queued
                } else {
                    JobState::Converted
                };
                let idx = (job.attempts as usize - 1).min(RETRY_BACKOFFS.len() - 1);
                post.retry = Some((job.id.clone(), RETRY_BACKOFFS[idx]));
                let f = JobLog::of(job);
                tracing::warn!(
                    event = "job_retry_scheduled",
                    job_id = %job.id,
                    file = %f.file,
                    profile = %f.profile,
                    printer = %f.printer,
                    pages = f.pages,
                    status = %f.status,
                    started_at = %f.started_at,
                    attempts = job.attempts,
                    backoff_ms = RETRY_BACKOFFS[idx].as_millis() as u64,
                    error = %f.error,
                    message = %e
                );
            }
        }
    }
}

/// Map an [`EngineError`] to a persisted [`JobError`] with a stable code.
fn to_job_error(e: &EngineError) -> JobError {
    let code = match e {
        EngineError::ConversionFailed(_) => "conversion_failed",
        EngineError::PrintFailed(_) => "print_failed",
        EngineError::UnsupportedSetting { .. } => "unsupported_setting",
        EngineError::DependencyMissing(_) => "dependency_missing",
        EngineError::UnsupportedFormat(_) => "unsupported_format",
        EngineError::Cancelled => "cancelled",
    };
    JobError::new(code, e.to_string())
}

/// Best-effort cache-file deletion; a missing file is not an error.
fn delete_cache_file(path: &str) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(event = "cache_delete_failed", path = %path, error = %e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::job::{Duplex, JobSettings};
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use tempfile::TempDir;

    // ---- Fakes -----------------------------------------------------------

    enum ConvertStep {
        Produce,
        Passthrough,
        Fail,
    }

    enum PrintStep {
        Ok(Option<String>),
        Fail,
        /// The spooler child outlived its budget and was killed — what the real
        /// engine reports from `wait_with_timeout`.
        Timeout,
    }

    struct FakeRunner {
        cache: PathBuf,
        convert_steps: Mutex<VecDeque<ConvertStep>>,
        print_steps: Mutex<VecDeque<PrintStep>>,
        printer: String,
        duplex_supported: AtomicBool,
        resolve_ok: AtomicBool,
        /// When false, `resolve_printer` reports the requested printer as no
        /// longer installed (the missing-printer path).
        printer_installed: AtomicBool,
        /// Printer names the OS no longer enumerates. Unlike
        /// `printer_installed`, this leaves the *fallback* printer working, so a
        /// test can model "the pinned queue is gone but the app default is
        /// fine" — the situation a paused job has to be able to escape.
        missing_printers: Mutex<HashSet<String>>,
        cancelled_os: Mutex<Vec<String>>,
        convert_calls: AtomicUsize,
        print_calls: AtomicUsize,
        /// Timeouts the fake was asked to enforce, one per `print` call.
        print_timeouts: Mutex<Vec<Option<Duration>>>,
        /// The settings the fake was handed for each `print` call, so a test can
        /// assert what would have reached the spooler.
        print_settings: Mutex<Vec<JobSettings>>,
        /// The file name handed to each `resolve_printer` call. A file-type
        /// routing rule can only apply if the dispatcher passes it, so this is
        /// how a test proves the plumbing exists.
        resolve_file_names: Mutex<Vec<String>>,
    }

    impl FakeRunner {
        fn new(cache: PathBuf) -> Self {
            Self {
                cache,
                convert_steps: Mutex::new(VecDeque::new()),
                print_steps: Mutex::new(VecDeque::new()),
                printer: "FakePrinter".to_string(),
                duplex_supported: AtomicBool::new(true),
                resolve_ok: AtomicBool::new(true),
                printer_installed: AtomicBool::new(true),
                missing_printers: Mutex::new(HashSet::new()),
                cancelled_os: Mutex::new(Vec::new()),
                convert_calls: AtomicUsize::new(0),
                print_calls: AtomicUsize::new(0),
                print_timeouts: Mutex::new(Vec::new()),
                print_settings: Mutex::new(Vec::new()),
                resolve_file_names: Mutex::new(Vec::new()),
            }
        }
    }

    impl JobRunner for FakeRunner {
        fn convert(
            &self,
            job: &Job,
            cancel: Arc<AtomicBool>,
            progress: Box<dyn Fn(f32) + Send + Sync>,
        ) -> Result<Option<PathBuf>, EngineError> {
            self.convert_calls.fetch_add(1, Ordering::Relaxed);
            if cancel.load(Ordering::Relaxed) {
                return Err(EngineError::Cancelled);
            }
            progress(0.5);
            let step = self
                .convert_steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ConvertStep::Produce);
            match step {
                ConvertStep::Produce => {
                    std::fs::create_dir_all(&self.cache).unwrap();
                    let p = self.cache.join(format!("{}.pdf", job.id));
                    std::fs::write(&p, b"%PDF-1.4 fake").unwrap();
                    Ok(Some(p))
                }
                ConvertStep::Passthrough => Ok(None),
                ConvertStep::Fail => Err(EngineError::ConversionFailed("boom".to_string())),
            }
        }

        fn print(
            &self,
            job: &Job,
            _printer: &str,
            timeout: Option<Duration>,
        ) -> Result<SpoolOutcome, PrintError> {
            self.print_calls.fetch_add(1, Ordering::Relaxed);
            self.print_timeouts.lock().unwrap().push(timeout);
            // Mirror the real runner: the spooler only ever sees the settings
            // that actually apply to this input.
            self.print_settings.lock().unwrap().push(
                crate::engine::print::effective_settings(&job.settings, &job.input_ext)
                    .into_owned(),
            );
            let step = self
                .print_steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(PrintStep::Ok(None));
            match step {
                PrintStep::Ok(os_job_id) => Ok(SpoolOutcome { os_job_id }),
                PrintStep::Fail => Err(EngineError::PrintFailed("boom".to_string()).into()),
                PrintStep::Timeout => Err(PrintError::TimedOut {
                    seconds: timeout.map(|t| t.as_secs()).unwrap_or(0),
                }),
            }
        }

        fn resolve_printer(
            &self,
            requested: Option<&str>,
            file_name: &str,
        ) -> Result<(String, bool), PrintError> {
            self.resolve_file_names
                .lock()
                .unwrap()
                .push(file_name.to_string());
            if !self.resolve_ok.load(Ordering::Relaxed) {
                return Err(PrintError::NoPrinter);
            }
            let name = requested
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| self.printer.clone());
            if !self.printer_installed.load(Ordering::Relaxed)
                || self.missing_printers.lock().unwrap().contains(&name)
            {
                return Err(PrintError::PrinterMissing { printer: name });
            }
            Ok((name, self.duplex_supported.load(Ordering::Relaxed)))
        }

        fn cancel_os_job(&self, os_job_id: &str) {
            self.cancelled_os
                .lock()
                .unwrap()
                .push(os_job_id.to_string());
        }
    }

    #[derive(Default)]
    struct FakeEmit {
        snapshots: Mutex<Vec<QueueSnapshot>>,
        progress: Mutex<Vec<(String, f32)>>,
    }

    impl EventSink for FakeEmit {
        fn queue_updated(&self, snapshot: &QueueSnapshot) {
            self.snapshots.lock().unwrap().push(snapshot.clone());
        }
        fn job_progress(&self, id: &str, _phase: &str, percent: f32) {
            self.progress
                .lock()
                .unwrap()
                .push((id.to_string(), percent));
        }
    }

    struct InlineSpawner;
    impl BlockingSpawner for InlineSpawner {
        fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
            task();
        }
    }

    /// A boxed blocking task, as handed to [`BlockingSpawner::spawn_blocking`].
    type BoxedTask = Box<dyn FnOnce() + Send + 'static>;

    /// A spawner that parks blocking tasks instead of running them, so a test
    /// can keep a conversion "in flight" (job stuck in `Converting`) while it
    /// advances the virtual clock to trigger the conversion-timeout timer.
    #[derive(Default, Clone)]
    struct DeferredSpawner {
        tasks: Arc<Mutex<VecDeque<BoxedTask>>>,
    }

    impl BlockingSpawner for DeferredSpawner {
        fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
            self.tasks.lock().unwrap().push_back(task);
        }
    }

    impl DeferredSpawner {
        /// Run the oldest parked task (e.g. the aborted conversion reporting
        /// back). Returns `false` when none are parked.
        fn run_next(&self) -> bool {
            let task = self.tasks.lock().unwrap().pop_front();
            match task {
                Some(t) => {
                    t();
                    true
                }
                None => false,
            }
        }
    }

    // ---- Harness ---------------------------------------------------------

    struct Harness {
        d: Dispatcher,
        rx: mpsc::UnboundedReceiver<Cmd>,
        tx: mpsc::UnboundedSender<Cmd>,
        runner: Arc<FakeRunner>,
        emit: Arc<FakeEmit>,
        state: Arc<RwLock<QueueState>>,
        queue_file: PathBuf,
        _tmp: TempDir,
    }

    /// Default harness: inline spawner, default limits (concurrency 2, no
    /// conversion timeout).
    fn harness() -> Harness {
        harness_with(Arc::new(InlineSpawner), DispatcherLimits::default())
    }

    /// Harness with an injected spawner and explicit limits, for the
    /// concurrency-cap and conversion-timeout tests.
    fn harness_with(spawner: Arc<dyn BlockingSpawner>, limits: DispatcherLimits) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("conversions");
        let queue_file = tmp.path().join("queue.json");
        let (tx, rx) = mpsc::unbounded_channel();
        let runner = Arc::new(FakeRunner::new(cache));
        let emit = Arc::new(FakeEmit::default());
        let state = Arc::new(RwLock::new(QueueState {
            queue_paused: false,
            jobs: Vec::new(),
        }));
        let d = Dispatcher::new(
            state.clone(),
            tx.clone(),
            runner.clone(),
            emit.clone(),
            spawner,
            queue_file.clone(),
            limits,
        );
        Harness {
            d,
            rx,
            tx,
            runner,
            emit,
            state,
            queue_file,
            _tmp: tmp,
        }
    }

    impl Harness {
        fn step(&mut self) -> bool {
            match self.rx.try_recv() {
                Ok(cmd) => {
                    self.d.dispatch_one(cmd);
                    true
                }
                Err(_) => false,
            }
        }

        fn run_to_idle(&mut self) {
            while self.step() {}
        }

        fn submit(&mut self, jobs: Vec<Job>) {
            self.tx.send(Cmd::Submit { jobs }).unwrap();
        }

        fn jobs(&self) -> Vec<Job> {
            self.state.read().unwrap().jobs.clone()
        }

        fn job(&self, id: &str) -> Job {
            self.jobs().into_iter().find(|j| j.id == id).unwrap()
        }

        fn count(&self, s: JobState) -> usize {
            self.jobs().iter().filter(|j| j.state == s).count()
        }

        /// Send a command carrying a oneshot reply, then pump the loop until the
        /// reply arrives.
        async fn request<T>(&mut self, make: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> T {
            let (rtx, mut rrx) = oneshot::channel();
            self.tx.send(make(rtx)).unwrap();
            loop {
                match rrx.try_recv() {
                    Ok(v) => return v,
                    Err(oneshot::error::TryRecvError::Empty) => {
                        if !self.step() {
                            tokio::task::yield_now().await;
                        }
                    }
                    Err(oneshot::error::TryRecvError::Closed) => panic!("reply dropped"),
                }
            }
        }

        /// Advance the (paused) virtual clock by `dur` and process anything that
        /// falls due. The leading yields let a freshly-spawned retry timer poll
        /// its `sleep` and register its deadline *before* the clock moves —
        /// otherwise advancing would miss a timer that hasn't been polled yet.
        async fn advance_and_settle(&mut self, dur: Duration) {
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            tokio::time::advance(dur).await;
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            self.run_to_idle();
        }
    }

    fn make_job(name: &str) -> Job {
        Job::new(
            "batch",
            format!("/tmp/{name}"),
            JobSettings::default(),
            None,
            now_rfc3339(),
        )
    }

    // ---- Tests -----------------------------------------------------------

    #[tokio::test]
    async fn happy_path_converts_prints_and_deletes_cache() {
        let mut h = harness();
        let j = make_job("a.pdf");
        let id = j.id.clone();
        let cache_path = h.runner.cache.join(format!("{id}.pdf"));
        h.submit(vec![j]);
        h.run_to_idle();

        assert_eq!(h.job(&id).state, JobState::Done);
        assert!(h.job(&id).cache_pdf.is_none(), "cache_pdf cleared on Done");
        assert!(!cache_path.exists(), "cache file deleted on Done");
        assert!(h.job(&id).finished_at.is_some());
        assert!(h.job(&id).started_at.is_some());
        assert_eq!(h.runner.print_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn two_conversions_start_and_third_waits() {
        let mut h = harness();
        let jobs: Vec<Job> = (0..3).map(|i| make_job(&format!("f{i}.pdf"))).collect();
        let ids: Vec<String> = jobs.iter().map(|j| j.id.clone()).collect();
        h.submit(jobs);
        h.step(); // process Submit only

        assert_eq!(h.count(JobState::Converting), 2);
        assert_eq!(h.count(JobState::Queued), 1);
        assert_eq!(h.job(&ids[2]).state, JobState::Queued);

        h.run_to_idle();
        assert!(ids.iter().all(|id| h.job(id).state == JobState::Done));
    }

    #[tokio::test]
    async fn never_more_than_one_print_at_a_time() {
        let mut h = harness();
        let jobs: Vec<Job> = (0..3).map(|i| make_job(&format!("f{i}.pdf"))).collect();
        h.submit(jobs);
        h.run_to_idle();

        let snapshots = h.emit.snapshots.lock().unwrap();
        for s in snapshots.iter() {
            let printing = s
                .jobs
                .iter()
                .filter(|j| j.state == JobState::Printing)
                .count();
            assert!(printing <= 1, "at most one printing at a time");
            let converting = s
                .jobs
                .iter()
                .filter(|j| j.state == JobState::Converting)
                .count();
            assert!(converting <= DEFAULT_MAX_CONCURRENT_CONVERSIONS);
        }
    }

    #[tokio::test]
    async fn passthrough_pdf_prints_input_in_place() {
        let mut h = harness();
        h.runner
            .convert_steps
            .lock()
            .unwrap()
            .push_back(ConvertStep::Passthrough);
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Done);
        assert_eq!(h.runner.print_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn transient_conversion_failure_retries_three_times_then_fails() {
        let mut h = harness();
        {
            let mut steps = h.runner.convert_steps.lock().unwrap();
            for _ in 0..4 {
                steps.push_back(ConvertStep::Fail);
            }
        }
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();

        // First failure: back to Queued, attempts=1, 1s backoff pending.
        assert_eq!(h.job(&id).state, JobState::Queued);
        assert_eq!(h.job(&id).attempts, 1);
        assert!(h.job(&id).error.is_some());

        // FR-034: three automatic retries at 1s/5s/25s, then permanent failure.
        h.advance_and_settle(Duration::from_secs(1)).await;
        assert_eq!(h.job(&id).attempts, 2);

        h.advance_and_settle(Duration::from_secs(5)).await;
        assert_eq!(h.job(&id).attempts, 3);
        assert_eq!(h.job(&id).state, JobState::Queued, "third retry pending");

        h.advance_and_settle(Duration::from_secs(25)).await;
        assert_eq!(h.job(&id).state, JobState::Failed);
        assert_eq!(h.job(&id).attempts, 4);
        assert_eq!(h.runner.convert_calls.load(Ordering::Relaxed), 4);
    }

    #[tokio::test]
    async fn resume_during_inflight_conversion_does_not_double_start() {
        // Pause a Converting job, resume it before the aborted conversion
        // reports back: the scheduler must NOT start a second conversion for
        // the same id (both would write the same cache path) until the stale
        // ConversionDone arrives.
        let mut h = harness();
        // Use a deferring spawner? The inline spawner completes conversions
        // synchronously, so simulate the in-flight window by hand: put the job
        // into Converting with a registered flag, as schedule() would.
        let mut j = make_job("a.pdf");
        let id = j.id.clone();
        j.state = JobState::Converting;
        h.state.write().unwrap().jobs.push(j);
        let flag = Arc::new(AtomicBool::new(false));
        h.d.converting.insert(id.clone(), flag.clone());

        // Pause sets the cancel flag and parks the job.
        let r = h
            .request(|reply| Cmd::Pause {
                id: id.clone(),
                reply,
            })
            .await;
        assert!(r.is_ok());
        assert!(flag.load(Ordering::Relaxed), "cancel flag set on pause");
        assert_eq!(h.job(&id).state, JobState::Paused);

        // Resume before the stale ConversionDone: job is Queued but must be
        // skipped by the scheduler while its old conversion is unaccounted for.
        let r = h
            .request(|reply| Cmd::Resume {
                id: id.clone(),
                reply,
            })
            .await;
        assert!(r.is_ok());
        assert_eq!(h.job(&id).state, JobState::Queued);
        assert_eq!(
            h.runner.convert_calls.load(Ordering::Relaxed),
            0,
            "no second conversion while the first is in flight"
        );

        // The stale completion arrives (aborted → Cancelled): the id is
        // released and the scheduler immediately starts a fresh conversion,
        // which runs to Done.
        h.tx.send(Cmd::ConversionDone {
            id: id.clone(),
            result: Err(EngineError::Cancelled),
        })
        .unwrap();
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Done);
        assert_eq!(h.runner.convert_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn transient_print_failure_retries_and_keeps_cache() {
        let mut h = harness();
        {
            let mut steps = h.runner.print_steps.lock().unwrap();
            steps.push_back(PrintStep::Fail); // first print fails
            steps.push_back(PrintStep::Ok(None)); // retry succeeds
        }
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();

        // Print failure returns the job to Converted (not Queued) and keeps the
        // cached PDF so the retry does not re-convert.
        assert_eq!(h.job(&id).state, JobState::Converted);
        assert_eq!(h.job(&id).attempts, 1);
        let cache = h.job(&id).cache_pdf.clone().unwrap();
        assert!(Path::new(&cache).exists(), "cache retained for print retry");

        h.advance_and_settle(Duration::from_secs(1)).await;
        assert_eq!(h.job(&id).state, JobState::Done);
        assert_eq!(h.runner.print_calls.load(Ordering::Relaxed), 2);
        assert_eq!(
            h.runner.convert_calls.load(Ordering::Relaxed),
            1,
            "not re-converted on print retry"
        );
    }

    #[tokio::test]
    async fn unsupported_setting_pauses_without_retry() {
        let mut h = harness();
        h.runner.duplex_supported.store(false, Ordering::Relaxed);
        let mut j = make_job("a.pdf");
        j.settings.duplex = Duplex::Long;
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();

        assert_eq!(h.job(&id).state, JobState::Paused);
        assert_eq!(h.job(&id).attempts, 0, "no retry for a user-fixable error");
        let err = h.job(&id).error.unwrap();
        assert_eq!(err.code, "unsupported_setting");
        assert_eq!(
            h.runner.print_calls.load(Ordering::Relaxed),
            0,
            "never spooled"
        );
    }

    #[tokio::test]
    async fn resume_of_interrupted_shutdown_reenters_pipeline() {
        let mut h = harness();
        let mut j = make_job("a.pdf");
        let id = j.id.clone();
        std::fs::create_dir_all(&h.runner.cache).unwrap();
        let cache_file = h.runner.cache.join(format!("{id}.pdf"));
        std::fs::write(&cache_file, b"%PDF fake").unwrap();
        j.state = JobState::Paused;
        j.cache_pdf = Some(cache_file.to_string_lossy().into_owned());
        j.error = Some(JobError::new(
            "interrupted_by_shutdown",
            "closed mid-flight",
        ));
        j.attempts = 2;
        h.state.write().unwrap().jobs.push(j);

        let r = h
            .request(|reply| Cmd::Resume {
                id: id.clone(),
                reply,
            })
            .await;
        assert!(r.is_ok());
        h.run_to_idle();

        assert_eq!(h.job(&id).state, JobState::Done);
        assert_eq!(h.job(&id).attempts, 0, "shutdown resume resets attempts");
    }

    #[tokio::test]
    async fn resume_without_cache_requeues_for_reconversion() {
        let mut h = harness();
        let mut j = make_job("a.pdf");
        let id = j.id.clone();
        j.state = JobState::Paused; // no cache_pdf
        h.state.write().unwrap().jobs.push(j);

        let r = h
            .request(|reply| Cmd::Resume {
                id: id.clone(),
                reply,
            })
            .await;
        assert!(r.is_ok());
        // Should re-enter at Queued and run the full pipeline to Done.
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Done);
        assert_eq!(h.runner.convert_calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn paused_queue_starts_no_new_work_then_resumes() {
        let mut h = harness();
        {
            let mut st = h.state.write().unwrap();
            st.queue_paused = true;
            st.jobs.push(make_job("a.pdf"));
            st.jobs.push(make_job("b.pdf"));
        }
        h.d.schedule();
        assert_eq!(h.count(JobState::Converting), 0);
        assert_eq!(h.count(JobState::Queued), 2);

        h.state.write().unwrap().queue_paused = false;
        h.d.schedule();
        h.run_to_idle();
        assert!(h.jobs().iter().all(|j| j.state == JobState::Done));
    }

    #[tokio::test]
    async fn pausing_queue_lets_inflight_conversion_finish() {
        let mut h = harness();
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.step(); // start convert; ConversionDone is queued but unprocessed
        assert_eq!(h.job(&id).state, JobState::Converting);

        // Pause before the pending completion is processed.
        h.state.write().unwrap().queue_paused = true;
        h.run_to_idle();

        // In-flight conversion still completes, but does not advance to printing.
        assert_eq!(h.job(&id).state, JobState::Converted);
    }

    #[tokio::test]
    async fn cancel_queued_job_marks_cancelled() {
        let mut h = harness();
        let jobs: Vec<Job> = (0..3).map(|i| make_job(&format!("f{i}.pdf"))).collect();
        let queued_id = jobs[2].id.clone();
        h.submit(jobs);
        h.step(); // 2 converting, 1 queued

        let r = h
            .request(|reply| Cmd::Cancel {
                id: queued_id.clone(),
                reply,
            })
            .await;
        assert!(r.is_ok());
        assert_eq!(h.job(&queued_id).state, JobState::Cancelled);
        h.run_to_idle();
    }

    #[tokio::test]
    async fn cancel_of_converting_job_cleans_partial_cache() {
        let mut h = harness();
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.step(); // convert ran inline and produced a file; ConversionDone pending
        let cache_path = h.runner.cache.join(format!("{id}.pdf"));
        assert!(cache_path.exists());
        assert_eq!(h.job(&id).state, JobState::Converting);

        let r = h
            .request(|reply| Cmd::Cancel {
                id: id.clone(),
                reply,
            })
            .await;
        assert!(r.is_ok());
        assert_eq!(h.job(&id).state, JobState::Cancelled);
        h.run_to_idle(); // process the pending ConversionDone -> cleanup
        assert!(!cache_path.exists(), "partial cache removed");
    }

    #[tokio::test]
    async fn cancel_all_counts_non_terminal_jobs() {
        let mut h = harness();
        let jobs: Vec<Job> = (0..3).map(|i| make_job(&format!("f{i}.pdf"))).collect();
        h.submit(jobs);
        h.step(); // 2 converting, 1 queued -> all 3 non-terminal

        let n = h.request(|reply| Cmd::CancelAll { reply }).await;
        assert_eq!(n, 3);
        h.run_to_idle();
        assert_eq!(h.count(JobState::Cancelled), 3);
    }

    #[tokio::test]
    async fn clear_done_removes_only_terminal_jobs() {
        let mut h = harness();
        let a = make_job("a.pdf");
        let b = make_job("b.pdf");
        let (ida, idb) = (a.id.clone(), b.id.clone());
        h.submit(vec![a, b]);
        h.run_to_idle(); // both Done

        // Pause the queue so a fresh job stays Queued (non-terminal).
        h.tx.send(Cmd::PauseQueue).unwrap();
        h.run_to_idle();
        let c = make_job("c.pdf");
        let idc = c.id.clone();
        h.submit(vec![c]);
        h.run_to_idle();
        assert_eq!(h.job(&idc).state, JobState::Queued);

        let n = h.request(|reply| Cmd::ClearDone { reply }).await;
        assert_eq!(n, 2);
        assert_eq!(h.jobs().len(), 1);
        assert!(h.jobs().iter().any(|j| j.id == idc));
        assert!(!h.jobs().iter().any(|j| j.id == ida || j.id == idb));
    }

    #[tokio::test]
    async fn print_done_for_cancelled_job_cancels_os_and_cleans_cache() {
        let mut h = harness();
        let mut j = make_job("a.pdf");
        let id = j.id.clone();
        std::fs::create_dir_all(&h.runner.cache).unwrap();
        let cache = h.runner.cache.join("leftover.pdf");
        std::fs::write(&cache, b"x").unwrap();
        j.state = JobState::Cancelled;
        j.cache_pdf = Some(cache.to_string_lossy().into_owned());
        h.state.write().unwrap().jobs.push(j);

        h.tx.send(Cmd::PrintDone {
            id: id.clone(),
            result: Ok(PrintOk {
                printer: "P".to_string(),
                os_job_id: Some("P-1".to_string()),
            }),
        })
        .unwrap();
        h.run_to_idle();

        assert_eq!(
            h.runner.cancelled_os.lock().unwrap().as_slice(),
            &["P-1".to_string()]
        );
        assert!(!cache.exists());
    }

    #[tokio::test]
    async fn events_and_persistence_track_transitions() {
        let mut h = harness();
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();

        // The emitted snapshots pass through Converting … Done, in that order.
        let states: Vec<JobState> = h
            .emit
            .snapshots
            .lock()
            .unwrap()
            .iter()
            .filter_map(|s| s.jobs.iter().find(|j| j.id == id).map(|j| j.state))
            .collect();
        assert!(states.contains(&JobState::Converting));
        assert!(states.contains(&JobState::Printing));
        assert_eq!(*states.last().unwrap(), JobState::Done);

        // Progress was emitted for the conversion.
        assert!(!h.emit.progress.lock().unwrap().is_empty());

        // The queue file on disk reflects the terminal state.
        let raw = std::fs::read_to_string(&h.queue_file).unwrap();
        let snap: QueueSnapshot = serde_json::from_str(&raw).unwrap();
        assert!(snap
            .jobs
            .iter()
            .any(|j| j.id == id && j.state == JobState::Done));
    }

    #[tokio::test]
    async fn retry_of_failed_job_resets_and_reruns() {
        let mut h = harness();
        let mut j = make_job("a.pdf");
        let id = j.id.clone();
        j.state = JobState::Failed;
        j.attempts = 3;
        j.error = Some(JobError::new("conversion_failed", "boom"));
        j.finished_at = Some(now_rfc3339());
        h.state.write().unwrap().jobs.push(j);

        let r = h
            .request(|reply| Cmd::Retry {
                id: id.clone(),
                reply,
            })
            .await;
        assert!(r.is_ok());
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Done);
        assert_eq!(h.job(&id).attempts, 0);
    }

    #[tokio::test]
    async fn cancel_missing_job_is_job_not_found() {
        let mut h = harness();
        let r = h
            .request(|reply| Cmd::Cancel {
                id: "nope".to_string(),
                reply,
            })
            .await;
        assert!(matches!(r, Err(IpcError::JobNotFound(_))));
    }

    #[tokio::test]
    async fn pause_printing_job_is_invalid_state() {
        let mut h = harness();
        let mut j = make_job("a.pdf");
        let id = j.id.clone();
        j.state = JobState::Printing;
        h.state.write().unwrap().jobs.push(j);
        let r = h
            .request(|reply| Cmd::Pause {
                id: id.clone(),
                reply,
            })
            .await;
        assert!(matches!(r, Err(IpcError::InvalidState(_))));
    }

    // ---- Concurrency cap (DispatcherLimits::max_concurrent) ---------------

    #[tokio::test]
    async fn schedule_honors_max_concurrent_of_one() {
        let mut h = harness_with(
            Arc::new(DeferredSpawner::default()),
            DispatcherLimits {
                max_concurrent: 1,
                conversion_timeout: None,
            },
        );
        let jobs: Vec<Job> = (0..3).map(|i| make_job(&format!("f{i}.pdf"))).collect();
        h.submit(jobs);
        h.step(); // process Submit → schedule
                  // Deferred spawner parks the conversions, so states reflect the cap.
        assert_eq!(h.count(JobState::Converting), 1);
        assert_eq!(h.count(JobState::Queued), 2);
    }

    #[tokio::test]
    async fn schedule_honors_max_concurrent_of_three() {
        let mut h = harness_with(
            Arc::new(DeferredSpawner::default()),
            DispatcherLimits {
                max_concurrent: 3,
                conversion_timeout: None,
            },
        );
        let jobs: Vec<Job> = (0..4).map(|i| make_job(&format!("f{i}.pdf"))).collect();
        h.submit(jobs);
        h.step();
        assert_eq!(h.count(JobState::Converting), 3);
        assert_eq!(h.count(JobState::Queued), 1);
    }

    /// FR-037: pausing or cancelling a converting job only *flags* its blocking
    /// task, which keeps running until its next stage boundary. Those orphans
    /// still occupy CPU, so they count against `max_concurrent` until their
    /// `ConversionDone` arrives.
    #[tokio::test]
    async fn fr_037_orphaned_conversions_still_count_against_the_cap() {
        let spawner = Arc::new(DeferredSpawner::default());
        let mut h = harness_with(spawner.clone(), DispatcherLimits::default());
        let jobs: Vec<Job> = (0..4).map(|i| make_job(&format!("f{i}.pdf"))).collect();
        let ids: Vec<String> = jobs.iter().map(|j| j.id.clone()).collect();
        h.submit(jobs);
        h.step();
        assert_eq!(
            spawner.tasks.lock().unwrap().len(),
            2,
            "two conversions run"
        );

        // Pause one, cancel the other: both tasks are still running.
        h.request(|reply| Cmd::Pause {
            id: ids[0].clone(),
            reply,
        })
        .await
        .unwrap();
        h.request(|reply| Cmd::Cancel {
            id: ids[1].clone(),
            reply,
        })
        .await
        .unwrap();
        assert_eq!(
            spawner.tasks.lock().unwrap().len(),
            2,
            "no new conversion may start while the orphans still run"
        );
        assert_eq!(h.count(JobState::Queued), 2);

        // Once an orphan reports back, its slot frees up.
        spawner.run_next();
        h.run_to_idle();
        assert_eq!(h.count(JobState::Converting), 1);
    }

    // ---- Conversion timeout ----------------------------------------------

    #[tokio::test(start_paused = true)]
    async fn conversion_timeout_fails_job_and_sets_flag() {
        let spawner = Arc::new(DeferredSpawner::default());
        let mut h = harness_with(
            spawner.clone(),
            DispatcherLimits {
                max_concurrent: 2,
                conversion_timeout: Some(Duration::from_secs(60)),
            },
        );
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle(); // Submit → Converting; convert parked, timer armed.
        assert_eq!(h.job(&id).state, JobState::Converting);
        let flag =
            h.d.converting
                .get(&id)
                .cloned()
                .expect("attempt flag registered");
        assert!(!flag.load(Ordering::Relaxed));

        // Advance past the timeout: the timer sends ConversionTimeout.
        h.advance_and_settle(Duration::from_secs(60)).await;
        assert_eq!(h.job(&id).state, JobState::Failed);
        assert!(
            flag.load(Ordering::Relaxed),
            "cancel flag set so the running conversion aborts"
        );
        let err = h.job(&id).error.unwrap();
        assert_eq!(err.code, "conversion_timeout");
        assert!(h.job(&id).finished_at.is_some());
        assert_eq!(h.job(&id).attempts, 0, "timeout is terminal, not a retry");

        // The parked (now-aborted) conversion reports back; it must not
        // resurrect the failed job.
        spawner.run_next();
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn conversion_timeout_after_completion_is_noop() {
        // Inline spawner completes the conversion (and the whole job) before the
        // timer ever fires; advancing past the deadline must not touch the
        // terminal job — its `converting` entry is already gone.
        let mut h = harness_with(
            Arc::new(InlineSpawner),
            DispatcherLimits {
                max_concurrent: 2,
                conversion_timeout: Some(Duration::from_secs(60)),
            },
        );
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Done);

        h.advance_and_settle(Duration::from_secs(60)).await;
        assert_eq!(h.job(&id).state, JobState::Done, "stale timer is a no-op");
    }

    #[tokio::test]
    async fn stale_conversion_timeout_does_not_kill_current_attempt() {
        // A timer armed for attempt N fires after attempt N+1 has started with a
        // fresh flag. The Arc::ptr_eq guard must ignore it.
        let mut h = harness();
        let mut j = make_job("a.pdf");
        let id = j.id.clone();
        j.state = JobState::Converting;
        h.state.write().unwrap().jobs.push(j);

        // The live attempt (N+1) registers flag B.
        let flag_b = Arc::new(AtomicBool::new(false));
        h.d.converting.insert(id.clone(), flag_b.clone());

        // A stale timer from attempt N fires with a different flag (A).
        let flag_a = Arc::new(AtomicBool::new(false));
        h.tx.send(Cmd::ConversionTimeout {
            id: id.clone(),
            flag: flag_a.clone(),
        })
        .unwrap();
        h.step();

        assert_eq!(
            h.job(&id).state,
            JobState::Converting,
            "current attempt survives a stale timer"
        );
        assert!(!flag_b.load(Ordering::Relaxed), "live flag untouched");
        assert!(!flag_a.load(Ordering::Relaxed), "stale flag not set");
    }

    // ---- Print timeout (FR-034 / NFR-012) --------------------------------

    /// Harness whose engine limits carry a print budget (the same knob as the
    /// conversion budget — see [`DispatcherLimits::print_timeout`]).
    fn harness_with_print_timeout(secs: u64) -> Harness {
        harness_with(
            Arc::new(InlineSpawner),
            DispatcherLimits {
                max_concurrent: 2,
                conversion_timeout: Some(Duration::from_secs(secs)),
            },
        )
    }

    #[tokio::test]
    async fn fr_034_print_timeout_fails_job_terminally_and_queue_keeps_going() {
        // The regression this pins: a hung spooler used to leave the job in
        // `printing` forever, and since exactly one job prints at a time, the
        // whole queue died with it. The assertion that matters is the *second*
        // job reaching Done.
        let mut h = harness_with_print_timeout(60);
        h.runner
            .print_steps
            .lock()
            .unwrap()
            .push_back(PrintStep::Timeout);

        let a = make_job("a.pdf");
        let b = make_job("b.pdf");
        let (ida, idb) = (a.id.clone(), b.id.clone());
        h.submit(vec![a, b]);
        h.run_to_idle();

        // The wedged job is terminal with a distinguishable reason and is not
        // retried — re-spooling would most likely wedge again.
        assert_eq!(h.job(&ida).state, JobState::Failed);
        let err = h.job(&ida).error.unwrap();
        assert_eq!(err.code, "print_timeout");
        assert!(
            err.message.contains("60s"),
            "reason names the budget: {err:?}"
        );
        assert_eq!(
            h.job(&ida).attempts,
            0,
            "a timeout is terminal, not a retry"
        );
        assert!(h.job(&ida).finished_at.is_some());
        assert!(h.job(&ida).cache_pdf.is_none(), "cache released");

        // The queue moved on.
        assert_eq!(h.job(&idb).state, JobState::Done);
        assert_eq!(h.runner.print_calls.load(Ordering::Relaxed), 2);

        // The budget really reached the engine seam.
        assert_eq!(
            h.runner.print_timeouts.lock().unwrap()[0],
            Some(Duration::from_secs(60))
        );
    }

    #[tokio::test]
    async fn print_timeout_disabled_passes_no_budget_to_the_engine() {
        let mut h = harness(); // default limits: timeout disabled
        h.submit(vec![make_job("a.pdf")]);
        h.run_to_idle();
        assert_eq!(h.runner.print_timeouts.lock().unwrap().as_slice(), &[None]);
    }

    // ---- Missing printer (print-settings.md §printer) ---------------------

    #[tokio::test]
    async fn fr_034_missing_printer_pauses_the_job_instead_of_failing_it() {
        // The spec (and the Printers screen banner) promise "paused, not
        // failed" so the user can pick a replacement.
        let mut h = harness();
        h.runner.printer_installed.store(false, Ordering::Relaxed);
        let mut j = make_job("a.pdf");
        j.settings.printer = Some("Old Office".to_string());
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();

        assert_eq!(h.job(&id).state, JobState::Paused);
        assert_eq!(
            h.job(&id).attempts,
            0,
            "no retry burn on a user-fixable error"
        );
        let err = h.job(&id).error.unwrap();
        assert_eq!(err.code, "printer_unavailable");
        assert!(err.message.contains("Old Office"), "names it: {err:?}");
        assert_eq!(h.runner.print_calls.load(Ordering::Relaxed), 0);
        assert!(
            h.job(&id).cache_pdf.is_some(),
            "cache kept so resuming onto another printer does not re-convert"
        );
    }

    /// A job pinned to a printer that disappears used to be a dead end:
    /// resuming re-ran the same lookup, failed the same way, and there was no
    /// way to re-point it. Resume now re-resolves, and once preferences point
    /// somewhere usable the job goes there instead of being stuck.
    #[tokio::test]
    async fn fr_paused_job_escapes_a_printer_that_is_gone_for_good() {
        let mut h = harness();
        h.runner
            .missing_printers
            .lock()
            .unwrap()
            .insert("Old Office".to_string());

        let mut j = make_job("a.pdf");
        j.settings.printer = Some("Old Office".to_string());
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();

        assert_eq!(h.job(&id).state, JobState::Paused);
        assert_eq!(h.job(&id).error.unwrap().code, "printer_unavailable");

        // Resuming re-resolves: "Old Office" is still gone, but the app-level
        // chain answers with the printer that is there, so the pin is dropped
        // and the job actually prints.
        h.request(|reply| Cmd::Resume {
            id: id.clone(),
            reply,
        })
        .await
        .expect("resume succeeds now that another printer resolves");

        assert_eq!(
            h.job(&id).settings.printer,
            None,
            "the dead pin is dropped so dispatch can resolve normally"
        );
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Done);
        assert_eq!(h.job(&id).printer_resolved.as_deref(), Some("FakePrinter"));
        assert_eq!(
            h.runner.convert_calls.load(Ordering::Relaxed),
            1,
            "the cached PDF is reused — resuming must not re-convert"
        );
    }

    /// The other half of the same rule: when there is still nothing to print
    /// on, resume must say so instead of silently putting the job back in the
    /// state it was already stuck in.
    #[tokio::test]
    async fn fr_resume_refuses_while_no_printer_resolves_at_all() {
        let mut h = harness();
        h.runner.printer_installed.store(false, Ordering::Relaxed);

        let mut j = make_job("a.pdf");
        j.settings.printer = Some("Old Office".to_string());
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Paused);

        let err = h
            .request(|reply| Cmd::Resume {
                id: id.clone(),
                reply,
            })
            .await
            .expect_err("nothing resolves, so the resume is refused");
        let msg = err.to_string();
        assert!(msg.contains("Old Office"), "names the printer: {msg}");
        assert!(msg.contains("Printers screen"), "says what to do: {msg}");

        // The job is untouched: still paused, still explaining why.
        assert_eq!(h.job(&id).state, JobState::Paused);
        assert_eq!(
            h.job(&id).error.unwrap().code,
            "printer_unavailable",
            "the reason survives a refused resume"
        );
        assert_eq!(h.job(&id).settings.printer.as_deref(), Some("Old Office"));

        // Once the printer is back, the very same resume works and keeps it.
        h.runner.printer_installed.store(true, Ordering::Relaxed);
        h.request(|reply| Cmd::Resume {
            id: id.clone(),
            reply,
        })
        .await
        .expect("resume works once the printer is installed again");
        assert_eq!(h.job(&id).settings.printer.as_deref(), Some("Old Office"));
        h.run_to_idle();
        assert_eq!(h.job(&id).state, JobState::Done);
    }

    #[tokio::test]
    async fn no_configured_printer_pauses_the_job() {
        let mut h = harness();
        h.runner.resolve_ok.store(false, Ordering::Relaxed);
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle();

        assert_eq!(h.job(&id).state, JobState::Paused);
        assert_eq!(h.job(&id).error.unwrap().code, "printer_unavailable");
    }

    // ---- Page ranges are PDF-only ----------------------------------------

    #[tokio::test]
    async fn fr_040_image_job_is_spooled_without_page_ranges() {
        let mut h = harness();
        let mut img = Job::new(
            "batch",
            "/tmp/photo.png",
            JobSettings {
                page_ranges: Some("2-3".to_string()),
                ..JobSettings::default()
            },
            None,
            now_rfc3339(),
        );
        let mut pdf = img.clone();
        img.state = JobState::Queued;
        pdf.id = uuid::Uuid::now_v7().to_string();
        pdf.input_path = "/tmp/report.pdf".to_string();
        pdf.file_name = "report.pdf".to_string();
        pdf.input_ext = "pdf".to_string();
        h.submit(vec![img, pdf]);
        h.run_to_idle();

        let spooled = h.runner.print_settings.lock().unwrap();
        assert_eq!(spooled.len(), 2);
        assert!(
            spooled[0].page_ranges.is_none(),
            "an image is one page; a 2-3 range would spool nothing"
        );
        assert_eq!(spooled[1].page_ranges.as_deref(), Some("2-3"));
    }

    // ---- File-type routing ------------------------------------------------

    /// The "Route file types here" rules on the Printers screen were persisted
    /// but never consumed by any print path. Routing is decided per file, so
    /// the dispatcher has to hand the file name to `resolve_printer`; without
    /// it the rule can never match, which is exactly how the feature was dead.
    #[tokio::test]
    async fn routing_needs_the_file_name_at_printer_resolution() {
        let mut h = harness();
        let mut pdf = Job::new(
            "batch",
            "/tmp/report.pdf",
            JobSettings::default(),
            None,
            now_rfc3339(),
        );
        pdf.state = JobState::Queued;
        pdf.file_name = "report.pdf".to_string();
        pdf.input_ext = "pdf".to_string();
        h.submit(vec![pdf]);
        h.run_to_idle();

        let seen = h.runner.resolve_file_names.lock().unwrap();
        assert_eq!(
            seen.as_slice(),
            ["report.pdf"],
            "the job's file name must reach printer resolution for a .pdf rule to apply"
        );
    }

    // ---- Progress honesty -------------------------------------------------

    #[tokio::test]
    async fn printing_progress_is_indeterminate_not_zero() {
        // The spooler reports no fraction, so `percent` is null while printing
        // (the UI must draw an indeterminate bar) — never a bar stuck at 0 %.
        let spawner = Arc::new(DeferredSpawner::default());
        let mut h = harness_with(spawner.clone(), DispatcherLimits::default());
        let j = make_job("a.pdf");
        let id = j.id.clone();
        h.submit(vec![j]);
        h.run_to_idle(); // conversion parked
        spawner.run_next(); // conversion reports back
        h.run_to_idle(); // → Printing, spool parked

        assert_eq!(h.job(&id).state, JobState::Printing);
        let p = h
            .job(&id)
            .progress
            .expect("printing carries a progress phase");
        assert_eq!(p.phase, "printing");
        assert!(p.percent.is_none(), "no invented percentage");
    }

    // ---- FR-035 structured job log ---------------------------------------

    /// Collects the fields of every `tracing` event emitted while it is the
    /// thread's default subscriber, so the log record itself can be asserted on
    /// rather than the code that builds it.
    #[derive(Clone, Default)]
    struct LogCapture {
        events: Arc<Mutex<Vec<HashMap<String, String>>>>,
    }

    impl LogCapture {
        /// A capture to install with `with_default`.
        ///
        /// `tracing` caches each callsite's interest process-wide. A callsite
        /// first hit by a parallel test thread with no subscriber installed can
        /// be cached as `never`, and then this thread's scoped subscriber never
        /// sees the record. Installing an always-interested global registry once
        /// keeps every callsite live. A drain loop does not fix that: the
        /// harness spawner is inline, so nothing is ever left parked.
        fn install() -> Self {
            static GLOBAL: std::sync::Once = std::sync::Once::new();
            GLOBAL.call_once(|| {
                let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
            });
            Self::default()
        }

        /// The fields of the first record with this `event` name.
        fn record(&self, event: &str) -> HashMap<String, String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.get("event").map(String::as_str) == Some(event))
                .cloned()
                .unwrap_or_else(|| panic!("no {event} record was emitted"))
        }
    }

    #[derive(Default)]
    struct FieldGrab(HashMap<String, String>);

    impl tracing::field::Visit for FieldGrab {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            // Debug-quoting is uniform here, so strip it and compare values.
            self.0.insert(
                field.name().to_string(),
                format!("{value:?}").trim_matches('"').to_string(),
            );
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut grab = FieldGrab::default();
            event.record(&mut grab);
            self.events.lock().unwrap().push(grab.0);
        }
    }

    #[tokio::test]
    async fn fr_035_job_records_carry_start_end_file_profile_printer_pages_status() {
        use tracing_subscriber::layer::SubscriberExt;

        let capture = LogCapture::install();
        let subscriber = tracing_subscriber::registry().with(capture.clone());

        let mut h = harness();
        let mut j = make_job("report.pdf");
        j.profile_name = Some("Photo Color".to_string());
        j.pages = Some(3);
        j.settings.printer = Some("Office".to_string());
        let id = j.id.clone();

        tracing::subscriber::with_default(subscriber, || {
            h.submit(vec![j]);
            h.run_to_idle();
        });
        assert_eq!(h.job(&id).state, JobState::Done);

        // Submission records the file path once, at INFO.
        let submitted = capture.record("job_submitted");
        assert_eq!(submitted["input_path"], "/tmp/report.pdf");
        assert_eq!(submitted["file"], "report.pdf");
        assert_eq!(submitted["profile"], "Photo Color");
        assert_eq!(submitted["pages"], "3");

        // Start time (FR-035) is on the stage-start records.
        for event in ["job_start_convert", "job_start_print"] {
            let rec = capture.record(event);
            assert!(!rec["started_at"].is_empty(), "{event} lacks a start time");
            assert_eq!(rec["profile"], "Photo Color");
            assert_eq!(rec["printer"], "Office");
            assert_eq!(rec["pages"], "3");
        }

        // Completion carries every FR-035 field, and no absolute path.
        let done = capture.record("job_done");
        for key in [
            "job_id",
            "file",
            "profile",
            "printer",
            "pages",
            "status",
            "started_at",
            "finished_at",
        ] {
            assert!(
                done.contains_key(key),
                "job_done is missing {key}: {done:?}"
            );
        }
        assert_eq!(done["status"], "done");
        assert_eq!(done["printer"], "Office");
        assert!(!done["finished_at"].is_empty());
        assert!(
            !done.contains_key("input_path"),
            "terminal records must not carry absolute paths"
        );
    }

    #[tokio::test]
    async fn fr_035_failure_record_carries_the_error_and_the_job_context() {
        use tracing_subscriber::layer::SubscriberExt;

        let capture = LogCapture::install();
        let subscriber = tracing_subscriber::registry().with(capture.clone());

        let mut h = harness_with_print_timeout(60);
        h.runner
            .print_steps
            .lock()
            .unwrap()
            .push_back(PrintStep::Timeout);
        let mut j = make_job("report.pdf");
        j.profile_name = Some("Quick BW".to_string());
        j.pages = Some(7);

        tracing::subscriber::with_default(subscriber, || {
            h.submit(vec![j]);
            h.run_to_idle();
        });

        let rec = capture.record("job_print_timeout");
        assert_eq!(rec["status"], "failed");
        assert_eq!(rec["error"], "print_timeout");
        assert_eq!(rec["profile"], "Quick BW");
        assert_eq!(rec["pages"], "7");
        assert_eq!(rec["file"], "report.pdf");
        assert!(!rec["started_at"].is_empty());
        assert!(!rec["finished_at"].is_empty());
    }
}
