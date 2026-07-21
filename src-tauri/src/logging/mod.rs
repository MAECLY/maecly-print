//! Tracing setup.
//!
//! Writes structured JSONL to `<log_dir>/maeclyprint-YYYY-MM-DD.jsonl` and a
//! human-friendly compact format to stdout for `pnpm tauri dev`. The record
//! schema is documented in `docs/04-architecture/data-model.md` §5.
//!
//! ## Rotation and retention (NFR-081, FR-113)
//!
//! `tracing-appender`'s rolling appender only rotates on a clock boundary and
//! never deletes anything, so logs grew without bound. [`RollingJsonl`] does
//! both halves the spec asks for — roll on the **UTC date changing or the file
//! passing [`Policy::max_bytes`], whichever comes first**, and drop old files —
//! and it is a plain [`Write`], so the non-blocking writer thread and the JSON
//! layer are wired exactly as before.
//!
//! File names are `maeclyprint-<date>.jsonl` for the day's first file and
//! `maeclyprint-<date>.<n>.jsonl` (n ≥ 2) for each size roll after it, so the
//! documented daily name is what a user finds unless a single day is unusually
//! chatty.
//!
//! The sweep runs once at startup ([`sweep`]): it deletes anything older than
//! [`Policy::retain_days`] and then keeps only the newest
//! [`Policy::retain_files`]. Both bounds apply — whichever bites first wins.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use time::{Date, OffsetDateTime};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::util::paths;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Leading part of every log file name, before the date.
const FILE_PREFIX: &str = "maeclyprint-";
/// Trailing part of every log file name, after the date (and part number).
const FILE_SUFFIX: &str = ".jsonl";

/// Rotation and retention limits.
///
/// These are the spec's numbers, not tunables: `config.toml` has no `[logging]`
/// section yet, so the defaults below are the whole policy. They are a struct
/// rather than three constants so the tests can shrink them (a 10 MB rotation
/// test would be absurd) and so wiring them to settings later is a one-liner.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Size at which the current file rolls to the next part (NFR-081: 10 MB).
    pub max_bytes: u64,
    /// Files older than this many days are swept at startup
    /// (`data-model.md` §5.2: 14).
    pub retain_days: u16,
    /// Newest files kept after the age sweep (NFR-081: 7).
    pub retain_files: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_bytes: 10 * 1024 * 1024,
            retain_days: 14,
            retain_files: 7,
        }
    }
}

pub fn init() -> io::Result<()> {
    let log_dir = paths::log_dir();
    paths::ensure_dir(&log_dir)?;

    let policy = Policy::default();
    // Sweep before the writer opens today's file, so a file we are about to
    // append to can never be counted as one of the ones to drop.
    let swept = sweep(&log_dir, today_utc(), &policy);

    let writer = RollingJsonl::new(log_dir, policy);
    let (non_blocking, guard) = tracing_appender::non_blocking(writer);

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

    if swept > 0 {
        tracing::info!(
            event = "log_retention_swept",
            removed = swept,
            retain_days = policy.retain_days,
            retain_files = policy.retain_files
        );
    }
    Ok(())
}

/// Today's date in UTC — every log file name and the retention cutoff use UTC so
/// a machine crossing a timezone (or DST) never re-opens yesterday's file.
fn today_utc() -> Date {
    OffsetDateTime::now_utc().date()
}

/// `YYYY-MM-DD`, formatted by hand so no format description has to be parsed on
/// every write.
fn date_string(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Absolute path of the log file for `date`/`part`. Part 1 is the documented
/// bare daily name; later parts of the same day get a numeric infix.
fn log_path(dir: &Path, date: &str, part: u32) -> PathBuf {
    if part <= 1 {
        dir.join(format!("{FILE_PREFIX}{date}{FILE_SUFFIX}"))
    } else {
        dir.join(format!("{FILE_PREFIX}{date}.{part}{FILE_SUFFIX}"))
    }
}

/// Split one of our log file names back into `(date, part)`, or `None` for a
/// file we did not write (the sweep must never delete a stranger's file).
fn parse_log_name(name: &str) -> Option<(String, u32)> {
    let rest = name.strip_prefix(FILE_PREFIX)?.strip_suffix(FILE_SUFFIX)?;
    let (date, part) = match rest.split_once('.') {
        Some((date, part)) => (date, part.parse::<u32>().ok().filter(|&p| p >= 2)?),
        None => (rest, 1),
    };
    is_iso_date(date).then(|| (date.to_string(), part))
}

/// `YYYY-MM-DD` shape check. Only the shape matters: dates are compared as
/// strings (ISO dates sort chronologically), never parsed.
fn is_iso_date(s: &str) -> bool {
    s.len() == 10
        && s.as_bytes().iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
}

/// Delete log files that fall outside `policy`, returning how many went.
///
/// Two passes, in this order: anything dated before `today - retain_days`, then
/// everything past the newest `retain_files` of what is left. Files we did not
/// write are ignored, and a delete that fails is skipped rather than
/// propagated — losing the sweep must never stop the app from starting.
pub fn sweep(dir: &Path, today: Date, policy: &Policy) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0; // no log dir yet — nothing to sweep
    };

    let mut logs: Vec<(String, u32, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            let (date, part) = parse_log_name(name)?;
            Some((date, part, path))
        })
        .collect();

    let cutoff = date_string(
        today
            .checked_sub(time::Duration::days(policy.retain_days as i64))
            .unwrap_or(today),
    );

    let mut removed = 0usize;
    logs.retain(|(date, _, path)| {
        if *date >= cutoff {
            return true;
        }
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
        false
    });

    // Newest first: same day, higher part number is the more recent file.
    logs.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (_, _, path) in logs.iter().skip(policy.retain_files) {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The currently-open file plus the bookkeeping needed to decide when to roll.
struct Open {
    date: String,
    part: u32,
    file: File,
    written: u64,
}

/// Append-only JSONL writer that rolls daily and by size (see the module docs).
pub struct RollingJsonl {
    dir: PathBuf,
    policy: Policy,
    /// Injectable clock: production reads the wall clock, tests step it so a
    /// daily roll can be observed without waiting for midnight.
    clock: Box<dyn Fn() -> Date + Send>,
    open: Option<Open>,
}

impl RollingJsonl {
    pub fn new(dir: PathBuf, policy: Policy) -> Self {
        Self::with_clock(dir, policy, Box::new(today_utc))
    }

    pub fn with_clock(dir: PathBuf, policy: Policy, clock: Box<dyn Fn() -> Date + Send>) -> Self {
        Self {
            dir,
            policy,
            clock,
            open: None,
        }
    }

    /// Open (or reopen) the file this record belongs in.
    ///
    /// A record is never split across two files: if appending `len` bytes would
    /// push the current part past the size cap, the next part is opened first.
    /// A file that is already at the cap when we attach to it — the app
    /// restarting on the same day — rolls the same way, which is why the search
    /// walks parts upward instead of trusting part 1.
    fn ensure_open(&mut self, len: u64) -> io::Result<&mut Open> {
        let today = date_string((self.clock)());

        let roll = match &self.open {
            None => true,
            Some(o) => o.date != today || o.written + len > self.policy.max_bytes,
        };

        if roll {
            let start_part = match &self.open {
                Some(o) if o.date == today => o.part + 1,
                _ => 1,
            };
            self.open = Some(self.open_part(&today, start_part)?);
        }
        Ok(self.open.as_mut().expect("just opened"))
    }

    /// Open the first part at or after `part` that still has room.
    fn open_part(&self, date: &str, mut part: u32) -> io::Result<Open> {
        loop {
            let path = log_path(&self.dir, date, part);
            let written = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if written < self.policy.max_bytes || part == u32::MAX {
                let file = OpenOptions::new().create(true).append(true).open(&path)?;
                return Ok(Open {
                    date: date.to_string(),
                    part,
                    file,
                    written,
                });
            }
            part += 1;
        }
    }
}

impl Write for RollingJsonl {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let open = self.ensure_open(buf.len() as u64)?;
        let n = open.file.write(buf)?;
        open.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.open {
            Some(o) => o.file.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn date(s: &str) -> Date {
        let p: Vec<i32> = s.split('-').map(|x| x.parse().unwrap()).collect();
        Date::from_calendar_date(p[0], time::Month::try_from(p[1] as u8).unwrap(), p[2] as u8)
            .unwrap()
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"{}\n").unwrap();
        p
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn fr_113_log_file_name_matches_the_documented_shape() {
        let dir = Path::new("/logs");
        assert_eq!(
            log_path(dir, "2026-07-20", 1),
            dir.join("maeclyprint-2026-07-20.jsonl")
        );
        assert_eq!(
            log_path(dir, "2026-07-20", 3),
            dir.join("maeclyprint-2026-07-20.3.jsonl")
        );
        assert_eq!(
            parse_log_name("maeclyprint-2026-07-20.jsonl"),
            Some(("2026-07-20".to_string(), 1))
        );
        assert_eq!(
            parse_log_name("maeclyprint-2026-07-20.3.jsonl"),
            Some(("2026-07-20".to_string(), 3))
        );
        // Anything else is somebody else's file and must be left alone.
        for stranger in [
            "maeclyprint.jsonl",
            "maeclyprint-2026-07-20.jsonl.bak",
            "maeclyprint-not-a-date.jsonl",
            "maeclyprint-2026-07-20.x.jsonl",
            "queue.json",
        ] {
            assert_eq!(parse_log_name(stranger), None, "{stranger}");
        }
    }

    #[test]
    fn nfr_081_writer_rolls_on_size_and_keeps_records_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let policy = Policy {
            max_bytes: 32,
            ..Policy::default()
        };
        let clock = Box::new(|| date("2026-07-20"));
        let mut w = RollingJsonl::with_clock(tmp.path().to_path_buf(), policy, clock);

        // Three 20-byte records: the second one would overflow the 32-byte cap,
        // so it starts part 2 rather than being split.
        for _ in 0..3 {
            w.write_all(b"12345678901234567890").unwrap();
        }
        w.flush().unwrap();

        assert_eq!(
            names(tmp.path()),
            vec![
                "maeclyprint-2026-07-20.2.jsonl",
                "maeclyprint-2026-07-20.3.jsonl",
                "maeclyprint-2026-07-20.jsonl",
            ]
        );
        for part in 1..=3 {
            let p = log_path(tmp.path(), "2026-07-20", part);
            assert_eq!(
                std::fs::read(&p).unwrap().len(),
                20,
                "whole record in {p:?}"
            );
        }
    }

    #[test]
    fn nfr_081_writer_rolls_when_the_date_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Arc::new(Mutex::new(date("2026-07-20")));
        let clock_src = now.clone();
        let mut w = RollingJsonl::with_clock(
            tmp.path().to_path_buf(),
            Policy::default(),
            Box::new(move || *clock_src.lock().unwrap()),
        );

        w.write_all(b"day one\n").unwrap();
        *now.lock().unwrap() = date("2026-07-21");
        w.write_all(b"day two\n").unwrap();
        w.flush().unwrap();

        assert_eq!(
            names(tmp.path()),
            vec![
                "maeclyprint-2026-07-20.jsonl",
                "maeclyprint-2026-07-21.jsonl"
            ]
        );
    }

    #[test]
    fn nfr_081_writer_appends_to_an_existing_file_after_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "maeclyprint-2026-07-20.jsonl");
        let mut w = RollingJsonl::with_clock(
            tmp.path().to_path_buf(),
            Policy::default(),
            Box::new(|| date("2026-07-20")),
        );
        w.write_all(b"more\n").unwrap();
        w.flush().unwrap();

        let body = std::fs::read_to_string(log_path(tmp.path(), "2026-07-20", 1)).unwrap();
        assert_eq!(body, "{}\nmore\n", "restart appends, never truncates");
    }

    #[test]
    fn fr_113_sweep_drops_files_older_than_retain_days() {
        let tmp = tempfile::tempdir().unwrap();
        let policy = Policy {
            retain_days: 14,
            retain_files: 100, // isolate the age rule
            ..Policy::default()
        };
        // 2026-07-06 is exactly the cutoff (today - 14) and stays.
        for d in ["2026-06-01", "2026-07-05", "2026-07-06", "2026-07-20"] {
            touch(tmp.path(), &format!("maeclyprint-{d}.jsonl"));
        }
        let stranger = touch(tmp.path(), "notes.txt");

        let removed = sweep(tmp.path(), date("2026-07-20"), &policy);

        assert_eq!(removed, 2);
        assert_eq!(
            names(tmp.path()),
            vec![
                "maeclyprint-2026-07-06.jsonl",
                "maeclyprint-2026-07-20.jsonl",
                "notes.txt",
            ]
        );
        assert!(
            stranger.exists(),
            "files we did not write are never deleted"
        );
    }

    #[test]
    fn nfr_081_sweep_keeps_only_the_newest_retained_files() {
        let tmp = tempfile::tempdir().unwrap();
        let policy = Policy {
            retain_days: 3650, // isolate the count rule
            retain_files: 3,
            ..Policy::default()
        };
        for d in ["2026-07-17", "2026-07-18", "2026-07-19"] {
            touch(tmp.path(), &format!("maeclyprint-{d}.jsonl"));
        }
        // Same-day parts count individually, newest part last.
        touch(tmp.path(), "maeclyprint-2026-07-20.jsonl");
        touch(tmp.path(), "maeclyprint-2026-07-20.2.jsonl");

        let removed = sweep(tmp.path(), date("2026-07-20"), &policy);

        assert_eq!(removed, 2);
        assert_eq!(
            names(tmp.path()),
            vec![
                "maeclyprint-2026-07-19.jsonl",
                "maeclyprint-2026-07-20.2.jsonl",
                "maeclyprint-2026-07-20.jsonl",
            ]
        );
    }

    #[test]
    fn sweep_on_a_missing_directory_is_a_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert_eq!(sweep(&missing, date("2026-07-20"), &Policy::default()), 0);
    }
}
