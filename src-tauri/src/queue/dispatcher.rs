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
//!   `"conversion_timeout"` error (no retry).
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

use crate::engine::print::SpoolOutcome;
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
        result: Result<PrintOk, EngineError>,
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

    /// Spool the job's PDF to `printer`.
    fn print(&self, job: &Job, printer: &str) -> Result<SpoolOutcome, EngineError>;

    /// Resolve the printer name to use and whether it supports duplex.
    fn resolve_printer(&self, requested: Option<&str>) -> Result<(String, bool), EngineError>;

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

    fn print(&self, job: &Job, printer: &str) -> Result<SpoolOutcome, EngineError> {
        // A converted job prints its cache PDF; a passthrough PDF prints in place.
        let pdf = match &job.cache_pdf {
            Some(p) => PathBuf::from(p),
            None => PathBuf::from(&job.input_path),
        };
        crate::engine::print::spool(&pdf, printer, &job.settings, &job.file_name)
    }

    fn resolve_printer(&self, requested: Option<&str>) -> Result<(String, bool), EngineError> {
        let name = match requested {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => resolve_default_printer()?,
        };
        // Fetch the duplex capability from enumeration; if the printer is not
        // enumerable, don't block (assume supported) and let spooling surface
        // any real error.
        let duplex_supported = crate::platform::list_printers()
            .ok()
            .and_then(|list| {
                list.into_iter()
                    .find(|p| p.id == name || p.name == name)
                    .map(|p| p.duplex)
            })
            .unwrap_or(true);
        Ok((name, duplex_supported))
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

/// Resolve a default printer without an explicit request: MaeclyPrint's app
/// default, then the OS default.
fn resolve_default_printer() -> Result<String, EngineError> {
    if let Some(name) = crate::settings::Config::load_or_default()
        .printers
        .default_printer
        .filter(|s| !s.is_empty())
    {
        return Ok(name);
    }
    if let Some(name) = crate::platform::default_printer()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
    {
        return Ok(name);
    }
    Err(EngineError::PrintFailed(
        "no printer is configured; choose a printer first".to_string(),
    ))
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
            tracing::info!(event = "job_submitted", job_id = %job.id, file = %job.file_name);
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

    fn on_resume(&mut self, id: &str) -> Result<(), IpcError> {
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
                    tracing::warn!(
                        event = "job_conversion_timeout",
                        job_id = %id,
                        timeout_s = timeout_secs
                    );
                }
            }
        }
        self.apply_post(post);
    }

    fn on_print_done(&mut self, id: String, result: Result<PrintOk, EngineError>) {
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
                            tracing::info!(event = "job_done", job_id = %id);
                        }
                        Err(e) => record_failure(job, &e, false, &mut post),
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
                let mut converting = st
                    .jobs
                    .iter()
                    .filter(|j| j.state == JobState::Converting)
                    .count();
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
                        job.progress = Some(JobProgress {
                            phase: "converting".to_string(),
                            percent: 0.0,
                        });
                        job.error = None;
                        tracing::info!(event = "job_start_convert", job_id = %job.id);
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
                        job.progress = Some(JobProgress {
                            phase: "printing".to_string(),
                            percent: 0.0,
                        });
                        job.error = None;
                        tracing::info!(event = "job_start_print", job_id = %job.id);
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
        self.spawner.spawn_blocking(Box::new(move || {
            let result = (|| -> Result<PrintOk, EngineError> {
                let (name, duplex_supported) =
                    runner.resolve_printer(job.settings.printer.as_deref())?;
                // Pure capability gate (honesty policy): a duplex request against
                // a non-duplex driver is an error rather than a silent downgrade.
                crate::engine::print::check_supported(
                    &job.settings,
                    &name,
                    duplex_supported,
                    true,
                )?;
                let outcome = runner.print(&job, &name)?;
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
                        job.progress = Some(JobProgress {
                            phase: "converting".to_string(),
                            percent,
                        });
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

/// Apply the FR-034 failure policy to `job`, recording any follow-up in `post`.
fn record_failure(job: &mut Job, e: &EngineError, is_conversion: bool, post: &mut Post) {
    match e {
        // User-fixable: pause and wait for the user, no automatic retry.
        EngineError::UnsupportedSetting { .. } | EngineError::DependencyMissing(_) => {
            job.state = JobState::Paused;
            job.error = Some(to_job_error(e));
            job.progress = None;
            tracing::warn!(event = "job_paused_needs_fix", job_id = %job.id, error = %e);
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
                tracing::warn!(event = "job_failed", job_id = %job.id, attempts = job.attempts, error = %e);
            } else {
                job.state = if is_conversion {
                    JobState::Queued
                } else {
                    JobState::Converted
                };
                let idx = (job.attempts as usize - 1).min(RETRY_BACKOFFS.len() - 1);
                post.retry = Some((job.id.clone(), RETRY_BACKOFFS[idx]));
                tracing::warn!(
                    event = "job_retry_scheduled",
                    job_id = %job.id,
                    attempts = job.attempts,
                    backoff_ms = RETRY_BACKOFFS[idx].as_millis() as u64,
                    error = %e
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
    }

    struct FakeRunner {
        cache: PathBuf,
        convert_steps: Mutex<VecDeque<ConvertStep>>,
        print_steps: Mutex<VecDeque<PrintStep>>,
        printer: String,
        duplex_supported: AtomicBool,
        resolve_ok: AtomicBool,
        cancelled_os: Mutex<Vec<String>>,
        convert_calls: AtomicUsize,
        print_calls: AtomicUsize,
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
                cancelled_os: Mutex::new(Vec::new()),
                convert_calls: AtomicUsize::new(0),
                print_calls: AtomicUsize::new(0),
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

        fn print(&self, _job: &Job, _printer: &str) -> Result<SpoolOutcome, EngineError> {
            self.print_calls.fetch_add(1, Ordering::Relaxed);
            let step = self
                .print_steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(PrintStep::Ok(None));
            match step {
                PrintStep::Ok(os_job_id) => Ok(SpoolOutcome { os_job_id }),
                PrintStep::Fail => Err(EngineError::PrintFailed("boom".to_string())),
            }
        }

        fn resolve_printer(&self, requested: Option<&str>) -> Result<(String, bool), EngineError> {
            if !self.resolve_ok.load(Ordering::Relaxed) {
                return Err(EngineError::PrintFailed("no printer".to_string()));
            }
            let name = requested
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| self.printer.clone());
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
}
