//! Persistent print-job queue: domain types, on-disk state and the
//! single-writer dispatcher task that drives jobs through the
//! convert → print pipeline (see `docs/04-architecture/data-model.md` §4).
//!
//! The public surface here is small: [`QueueHandle`] is the cloneable handle the
//! IPC layer holds (via Tauri managed state) to submit work and issue control
//! commands, and [`spawn`] boots the dispatcher task and returns that handle.
//! Everything that actually mutates the queue lives behind the
//! [`dispatcher`] channel — see that module for the state-machine semantics.

pub mod job;
pub mod persist;

mod dispatcher;

// Re-exported so `crate::settings::EngineCfg`'s default can mirror the
// dispatcher's own concurrency default without exposing the whole module.
pub(crate) use dispatcher::DEFAULT_MAX_CONCURRENT_CONVERSIONS;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::Context;
use tokio::sync::{mpsc, oneshot};

use crate::queue::job::{now_rfc3339, Job, JobBatch, PrintRequest, QueueSnapshot, SCHEMA_VERSION};
use crate::util::error::IpcError;

/// PDFs larger than this are not parsed for a page count at submit/inspect time
/// (the parse would stall the UI); their page count stays `None` until after
/// conversion.
pub(crate) const MAX_PDF_PROBE_BYTES: u64 = 100 * 1024 * 1024;

/// The dispatcher's live, in-memory view of the queue. Written only by the
/// dispatcher task; read (cloned) by [`QueueHandle::snapshot`] and the progress
/// callback.
#[derive(Debug, Clone)]
pub struct QueueState {
    pub queue_paused: bool,
    pub jobs: Vec<Job>,
}

/// Cloneable handle to the running queue. Held by the IPC layer as Tauri managed
/// state; every method is cheap (a channel send, or a read-lock clone).
#[derive(Clone)]
pub struct QueueHandle {
    tx: mpsc::UnboundedSender<dispatcher::Cmd>,
    state: Arc<RwLock<QueueState>>,
}

impl QueueHandle {
    /// A consistent, serializable view of the queue for `queue_status` and the
    /// initial UI paint. Reads the live state directly — no dispatcher
    /// round-trip — so it is safe to call from any thread.
    pub fn snapshot(&self) -> QueueSnapshot {
        let st = self.state.read().unwrap();
        QueueSnapshot {
            schema_version: SCHEMA_VERSION,
            queue_paused: st.queue_paused,
            jobs: st.jobs.clone(),
        }
    }

    /// Validate a [`PrintRequest`], fan it out into one [`Job`] per file, and
    /// hand the batch to the dispatcher.
    ///
    /// Validation is synchronous and returns before the jobs are enqueued, so a
    /// bad request never reaches the queue. Each file is `stat`ed (missing →
    /// [`IpcError::FileNotFound`]), its extension checked against the engine
    /// ([`IpcError::UnsupportedFormat`]), and PDFs are probed for a page count
    /// (best effort). Settings and page-range syntax are validated up front.
    /// Performs blocking filesystem I/O, so callers run it off the UI thread.
    pub fn submit(&self, req: PrintRequest) -> Result<JobBatch, IpcError> {
        if req.files.is_empty() {
            return Err(IpcError::InvalidRequest(
                "no files were provided to print".to_string(),
            ));
        }
        req.settings.validate().map_err(IpcError::InvalidRequest)?;
        if let Some(ranges) = &req.settings.page_ranges {
            crate::engine::pagerange::parse(ranges).map_err(IpcError::InvalidRequest)?;
        }

        // Read the size ceiling once for the whole batch (config is loaded per
        // submit, not per file). `sanitized()` so a hand-edited `0` can't reject
        // every file.
        let max_mb = crate::settings::Config::load_or_default()
            .engine
            .sanitized()
            .max_file_size_mb;

        let submitted_at = now_rfc3339();
        let batch_id = uuid::Uuid::now_v7().to_string();
        let mut jobs = Vec::with_capacity(req.files.len());
        let mut job_ids = Vec::with_capacity(req.files.len());

        for file in &req.files {
            let path = Path::new(file);
            let meta = std::fs::metadata(path).map_err(|_| IpcError::FileNotFound(file.clone()))?;
            if !meta.is_file() {
                return Err(IpcError::InvalidRequest(format!("{file} is not a file")));
            }
            if exceeds_size_limit(meta.len(), max_mb) {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| file.clone());
                return Err(IpcError::InvalidRequest(format!(
                    "{name} is larger than the {max_mb} MB limit"
                )));
            }
            let ext = crate::engine::extension_of(path);
            if !crate::engine::is_supported(&ext) {
                return Err(IpcError::UnsupportedFormat(format!(
                    "{file} is not a printable file type"
                )));
            }

            let mut job = Job::new(
                batch_id.clone(),
                file.clone(),
                req.settings.clone(),
                req.profile_name.clone(),
                submitted_at.clone(),
            );
            job.input_bytes = meta.len();
            job.pages = probe_pages(path, &ext, meta.len());
            job_ids.push(job.id.clone());
            jobs.push(job);
        }

        self.tx
            .send(dispatcher::Cmd::Submit { jobs })
            .map_err(|_| IpcError::Internal("print queue is not running".to_string()))?;
        Ok(JobBatch { batch_id, job_ids })
    }

    /// Cancel a single job. Surfaces [`IpcError::JobNotFound`] /
    /// [`IpcError::InvalidState`] from the dispatcher.
    pub async fn cancel(&self, id: String) -> Result<(), IpcError> {
        self.request(|reply| dispatcher::Cmd::Cancel { id, reply })
            .await
    }

    /// Pause a single job. See the dispatcher docs for per-state semantics.
    pub async fn pause(&self, id: String) -> Result<(), IpcError> {
        self.request(|reply| dispatcher::Cmd::Pause { id, reply })
            .await
    }

    /// Resume a paused job back into the pipeline.
    pub async fn resume(&self, id: String) -> Result<(), IpcError> {
        self.request(|reply| dispatcher::Cmd::Resume { id, reply })
            .await
    }

    /// Retry a failed job from the start (attempts reset).
    pub async fn retry(&self, id: String) -> Result<(), IpcError> {
        self.request(|reply| dispatcher::Cmd::Retry { id, reply })
            .await
    }

    /// Cancel every non-terminal job; returns how many were cancelled.
    pub async fn cancel_all(&self) -> Result<u32, IpcError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(dispatcher::Cmd::CancelAll { reply: tx })
            .map_err(|_| IpcError::Internal("print queue is not running".to_string()))?;
        rx.await
            .map_err(|_| IpcError::Internal("print queue dropped the reply".to_string()))
    }

    /// Remove all terminal (done/failed/cancelled) jobs; returns how many were
    /// removed.
    pub async fn clear_done(&self) -> Result<u32, IpcError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(dispatcher::Cmd::ClearDone { reply: tx })
            .map_err(|_| IpcError::Internal("print queue is not running".to_string()))?;
        rx.await
            .map_err(|_| IpcError::Internal("print queue dropped the reply".to_string()))
    }

    /// Push changed engine settings to the running dispatcher, so the
    /// concurrency cap and both timeouts apply without a restart. Takes the
    /// config as saved; it is sanitized here, like every other consumer does.
    pub fn apply_engine_settings(&self, engine: &crate::settings::EngineCfg) {
        let limits = dispatcher::DispatcherLimits::from_engine(&engine.sanitized());
        let _ = self.tx.send(dispatcher::Cmd::SetLimits { limits });
    }

    /// Pause the whole queue (stops starting new work; in-flight continues).
    pub fn pause_queue(&self) {
        let _ = self.tx.send(dispatcher::Cmd::PauseQueue);
    }

    /// Resume the whole queue.
    pub fn resume_queue(&self) {
        let _ = self.tx.send(dispatcher::Cmd::ResumeQueue);
    }

    /// Send a reply-carrying command and await its `Result<(), IpcError>`.
    async fn request(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<(), IpcError>>) -> dispatcher::Cmd,
    ) -> Result<(), IpcError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(make(tx))
            .map_err(|_| IpcError::Internal("print queue is not running".to_string()))?;
        rx.await
            .map_err(|_| IpcError::Internal("print queue dropped the reply".to_string()))?
    }
}

/// Boot the queue: recover any persisted state, sweep orphaned cache files,
/// spawn the dispatcher task, and return the [`QueueHandle`] for the IPC layer
/// to manage.
///
/// Recovery order (durable before serving): `load` → `recover` (in-flight jobs →
/// paused) → `save` (so the recovered state survives an immediate re-crash) →
/// orphan sweep of `<cache_dir>/conversions`.
pub fn spawn(app: tauri::AppHandle) -> anyhow::Result<QueueHandle> {
    let queue_file = persist::queue_file_path().context("resolving queue.json path")?;
    let cache_dir = crate::util::paths::cache_dir().join("conversions");

    let recovered = persist::recover(persist::load(&queue_file));
    if let Err(e) = persist::save(&queue_file, &recovered) {
        tracing::warn!(event = "queue_recover_persist_failed", error = %e,
            "could not persist recovered queue; continuing with in-memory state");
    }
    sweep_orphans(&cache_dir, &recovered.jobs);

    let (tx, rx) = mpsc::unbounded_channel();
    let state = Arc::new(RwLock::new(QueueState {
        queue_paused: recovered.queue_paused,
        jobs: recovered.jobs,
    }));

    let runner: Arc<dyn dispatcher::JobRunner> = Arc::new(dispatcher::EngineRunner { cache_dir });
    let emit: Arc<dyn dispatcher::EventSink> = Arc::new(dispatcher::TauriEmit { app });
    let spawner: Arc<dyn dispatcher::BlockingSpawner> = Arc::new(dispatcher::TokioBlockingSpawner);

    // Engine limits are read here at boot and pushed again by
    // `set_app_settings` whenever they change (`QueueHandle::set_limits`).
    // `sanitized()` clamps hand-edited values so a bricked config (0 MB / 1s
    // timeout / 99 workers) can't reach the dispatcher — every consumer reads
    // through it.
    let engine = crate::settings::Config::load_or_default()
        .engine
        .sanitized();
    let limits = dispatcher::DispatcherLimits::from_engine(&engine);

    let d = dispatcher::Dispatcher::new(
        state.clone(),
        tx.clone(),
        runner,
        emit,
        spawner,
        queue_file,
        limits,
    );
    tauri::async_runtime::spawn(dispatcher::run(d, rx));

    Ok(QueueHandle { tx, state })
}

/// True iff `bytes` exceeds the `max_mb` megabyte ceiling. Pure `u64` math so the
/// `MB → bytes` multiplication cannot overflow for any realistic limit; the
/// settings validator keeps `max_mb` in `1..=4096`, so a legitimate limit never
/// rejects everything. Kept separate from [`MAX_PDF_PROBE_BYTES`], which gates
/// page-count probing rather than acceptance.
pub(crate) fn exceeds_size_limit(bytes: u64, max_mb: u32) -> bool {
    bytes > (max_mb as u64) * 1024 * 1024
}

/// Best-effort page count for the UI: PDFs are parsed with `lopdf` (unless
/// larger than [`MAX_PDF_PROBE_BYTES`]); supported images are always one page;
/// anything else is unknown.
pub(crate) fn probe_pages(path: &Path, ext: &str, bytes: u64) -> Option<u32> {
    match ext {
        "pdf" => {
            if bytes > MAX_PDF_PROBE_BYTES {
                return None;
            }
            lopdf::Document::load(path)
                .ok()
                .map(|d| d.get_pages().len() as u32)
        }
        "" => None,
        other if crate::engine::is_supported(other) => Some(1),
        _ => None,
    }
}

/// Delete cache PDFs left behind by a previous run that no live job references
/// (e.g. partial conversions interrupted by a crash). A file is orphaned iff no
/// job's `cache_pdf` points at it.
fn sweep_orphans(cache_dir: &Path, jobs: &[Job]) {
    let referenced: std::collections::HashSet<PathBuf> = jobs
        .iter()
        .filter_map(|j| j.cache_pdf.as_ref())
        .map(PathBuf::from)
        .collect();

    let entries = match std::fs::read_dir(cache_dir) {
        Ok(e) => e,
        Err(_) => return, // no cache dir yet — nothing to sweep
    };
    let mut removed = 0u32;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && !referenced.contains(&p) && std::fs::remove_file(&p).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        tracing::info!(event = "cache_orphans_swept", removed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::job::{JobSettings, PrintRequest};
    use tempfile::TempDir;

    fn handle_with_open_channel() -> (QueueHandle, mpsc::UnboundedReceiver<dispatcher::Cmd>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = Arc::new(RwLock::new(QueueState {
            queue_paused: false,
            jobs: Vec::new(),
        }));
        (QueueHandle { tx, state }, rx)
    }

    fn write_file(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn req(files: Vec<String>, settings: JobSettings) -> PrintRequest {
        PrintRequest {
            files,
            settings,
            profile_name: None,
        }
    }

    #[test]
    fn submit_rejects_empty_file_list() {
        let (h, _rx) = handle_with_open_channel();
        assert!(matches!(
            h.submit(req(vec![], JobSettings::default())),
            Err(IpcError::InvalidRequest(_))
        ));
    }

    #[test]
    fn submit_rejects_missing_file() {
        let (h, _rx) = handle_with_open_channel();
        assert!(matches!(
            h.submit(req(
                vec!["/no/such/file.pdf".into()],
                JobSettings::default()
            )),
            Err(IpcError::FileNotFound(_))
        ));
    }

    #[test]
    fn submit_rejects_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let f = write_file(&dir, "notes.txt", b"hi");
        let (h, _rx) = handle_with_open_channel();
        assert!(matches!(
            h.submit(req(vec![f], JobSettings::default())),
            Err(IpcError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn submit_rejects_zero_copies() {
        let dir = tempfile::tempdir().unwrap();
        let f = write_file(&dir, "a.pdf", b"%PDF-1.4");
        let (h, _rx) = handle_with_open_channel();
        let settings = JobSettings {
            copies: 0,
            ..Default::default()
        };
        assert!(matches!(
            h.submit(req(vec![f], settings)),
            Err(IpcError::InvalidRequest(_))
        ));
    }

    #[test]
    fn submit_rejects_bad_page_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let f = write_file(&dir, "a.pdf", b"%PDF-1.4");
        let (h, _rx) = handle_with_open_channel();
        let settings = JobSettings {
            page_ranges: Some("5-1".into()),
            ..Default::default()
        };
        assert!(matches!(
            h.submit(req(vec![f], settings)),
            Err(IpcError::InvalidRequest(_))
        ));
    }

    #[test]
    fn submit_builds_batch_and_enqueues_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = write_file(&dir, "a.pdf", b"%PDF-1.4\n%stub");
        let f2 = write_file(&dir, "b.png", b"\x89PNG stub");
        let (h, mut rx) = handle_with_open_channel();
        let batch = h
            .submit(PrintRequest {
                files: vec![f1, f2],
                settings: JobSettings::default(),
                profile_name: Some("Quick".into()),
            })
            .unwrap();

        assert_eq!(batch.job_ids.len(), 2);
        match rx.try_recv().unwrap() {
            dispatcher::Cmd::Submit { jobs } => {
                assert_eq!(jobs.len(), 2);
                assert!(jobs.iter().all(|j| j.batch_id == batch.batch_id));
                assert!(jobs.iter().all(|j| j.input_bytes > 0));
                assert_eq!(
                    jobs.iter().map(|j| j.id.clone()).collect::<Vec<_>>(),
                    batch.job_ids
                );
            }
            _ => panic!("expected a Submit command"),
        }
    }

    #[test]
    fn probe_pages_classifies_by_extension() {
        // A supported image is always one page.
        assert_eq!(probe_pages(Path::new("/x/a.png"), "png", 10), Some(1));
        // Unknown / unsupported types are unknown.
        assert_eq!(probe_pages(Path::new("/x/a.txt"), "txt", 10), None);
        // An oversized PDF is not probed.
        assert_eq!(
            probe_pages(Path::new("/x/a.pdf"), "pdf", MAX_PDF_PROBE_BYTES + 1),
            None
        );
    }

    #[test]
    fn exceeds_size_limit_boundaries() {
        let mb = 1024 * 1024;
        // Exactly at the limit is allowed; one byte over is rejected.
        assert!(!exceeds_size_limit(200 * mb, 200));
        assert!(exceeds_size_limit(200 * mb + 1, 200));
        assert!(!exceeds_size_limit(0, 1));
        assert!(!exceeds_size_limit(mb, 1));
        assert!(exceeds_size_limit(mb + 1, 1));
        // A large limit doesn't overflow the u64 math.
        assert!(!exceeds_size_limit(4096 * mb, 4096));
        assert!(exceeds_size_limit(4096 * mb + 1, 4096));
    }

    #[test]
    fn sweep_orphans_removes_unreferenced_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("conversions");
        std::fs::create_dir_all(&cache).unwrap();
        let referenced = cache.join("keep.pdf");
        let orphan = cache.join("orphan.pdf");
        std::fs::write(&referenced, b"x").unwrap();
        std::fs::write(&orphan, b"x").unwrap();

        let mut job = Job::new(
            "b",
            "/tmp/x.pdf",
            JobSettings::default(),
            None,
            now_rfc3339(),
        );
        job.cache_pdf = Some(referenced.to_string_lossy().into_owned());

        sweep_orphans(&cache, &[job]);
        assert!(referenced.exists(), "referenced cache is kept");
        assert!(!orphan.exists(), "unreferenced cache is swept");
    }
}
