//! Tracing setup.
//!
//! Writes structured JSONL to `<log_dir>/maeclyprint.jsonl` with daily rotation.
//! Stdout receives a human-friendly compact format for `pnpm tauri dev`.
//! Schema is documented in `docs/04-architecture/data-model.md` §5.

use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::util::paths;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn init() -> std::io::Result<()> {
    let log_dir = paths::log_dir();
    paths::ensure_dir(&log_dir)?;

    let appender = tracing_appender::rolling::daily(&log_dir, "maeclyprint.jsonl");
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    // Hold the guard for the lifetime of the process so the writer thread
    // doesn't shut down mid-write.
    let _ = GUARD.set(guard);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,maeclyprint=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().json().with_writer(non_blocking))
        .with(fmt::layer().compact().with_target(false))
        .init();

    Ok(())
}
