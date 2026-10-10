//! Tracing setup.
//!
//! Writes structured JSONL to `<log_dir>/pluryprint-YYYY-MM-DD.jsonl` and a
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
//! File names are `pluryprint-<date>.jsonl` for the day's first file and
//! `pluryprint-<date>.<n>.jsonl` (n ≥ 2) for each size roll after it, so the
//! documented daily name is what a user finds unless a single day is unusually
//! chatty.
//!
//! The sweep ([`sweep`]) runs at startup **and on every roll** — a resident
//! instance can stay up for weeks, so retention cannot wait for a relaunch. It
//! deletes anything older than [`Policy::retain_days`] and then keeps only the
//! newest [`Policy::retain_files`]. Both bounds apply — whichever bites first
//! wins.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use time::{Date, OffsetDateTime};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::util::paths;

/// The writer thread's guard. In a `Mutex<Option<_>>` rather than a bare
/// `OnceLock` so [`flush`] can drop it: a headless run ends in
/// `std::process::exit`, which never runs destructors, and the lines still
/// buffered in the non-blocking writer would be lost with it.
static GUARD: OnceLock<Mutex<Option<WorkerGuard>>> = OnceLock::new();

/// Flush and stop the file writer. For a process about to `exit` (the CLI);
/// anything logged afterwards is dropped, so call it last.
pub fn flush() {
    if let Some(slot) = GUARD.get() {
        let guard = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        drop(guard);
    }
}

/// Leading part of every log file name, before the date.
const FILE_PREFIX: &str = "pluryprint-";
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
    /// Files older than this many days are swept at startup and on every roll
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

/// The level filter when `RUST_LOG` is not set: `info` for everything.
///
/// It used to be `info,pluryprint=debug`. An `EnvFilter` target is a string
/// prefix, so `pluryprint` also matched the library crate `pluryprint_lib`,
/// where every event lives: DEBUG was on by default and full paths (the
/// shell-menu cache, handoff files) reached the default log, which
/// `security-model.md` §6.2 and NFR-082 rule out.
pub const DEFAULT_FILTER: &str = "info";

/// Remove the `log.*` fields `tracing-log` attaches to records bridged from
/// the `log` crate (dependencies such as the updater plugin log that way):
/// `log.file` and `log.module_path` are source paths **on the build machine**
/// (`C:\Users\<builder>\.cargo\registry\…`), and `log.target`/`log.line`
/// repeat what the record already says. `tracing-subscriber`'s JSON formatter
/// drops them for debug-formatted values only, not for strings. Pure.
///
/// `line` is one JSONL record (with or without its newline). A line that is
/// not a JSON object with such fields is returned unchanged.
pub fn scrub_bridged_log_fields(line: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    use std::borrow::Cow;
    const MARKER: &[u8] = b"\"log.";
    if !line.windows(MARKER.len()).any(|w| w == MARKER) {
        return Cow::Borrowed(line);
    }
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    let Ok(serde_json::Value::Object(mut record)) = serde_json::from_slice(body) else {
        return Cow::Borrowed(line);
    };
    let scrub = |map: &mut serde_json::Map<String, serde_json::Value>| {
        let before = map.len();
        map.retain(|k, _| !k.starts_with("log."));
        map.len() != before
    };
    let mut changed = scrub(&mut record);
    if let Some(serde_json::Value::Object(fields)) = record.get_mut("fields") {
        changed |= scrub(fields);
    }
    if !changed {
        return Cow::Borrowed(line);
    }
    let mut out = serde_json::to_vec(&record).unwrap_or_else(|_| body.to_vec());
    if line.ends_with(b"\n") {
        out.push(b'\n');
    }
    Cow::Owned(out)
}

/// Logging for the GUI: the JSONL file plus a compact human stream on stdout
/// (what `pnpm tauri dev` shows).
pub fn init() -> io::Result<()> {
    init_with(true)
}

/// Logging for a headless CLI run: the **same** JSONL file, same directory,
/// same rotation and record schema as the GUI — and nothing on stdout.
///
/// stdout is the CLI's output channel: `--json` callers parse it, and a
/// tracing line interleaved with the payload would break every script that
/// does. Without this a CLI run left no trace at all in the app log, so a
/// failed scripted print could not be explained after the fact (FR-035).
pub fn init_cli() -> io::Result<()> {
    init_with(false)
}

fn init_with(stdout: bool) -> io::Result<()> {
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
    let _ = GUARD.set(Mutex::new(Some(guard)));

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    // `Option<Layer>` is itself a layer, so the stdout stream is simply absent
    // for a CLI run instead of being redirected somewhere.
    let console = stdout.then(|| fmt::layer().compact().with_target(false));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().json().with_writer(non_blocking))
        .with(console)
        .try_init()
        .map_err(io::Error::other)?;

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

/// Record every panic in the log file (NFR-033, FR-113).
///
/// Release builds use `panic = "abort"`, and the Windows GUI binary has no
/// console, so without this a panic anywhere — a command, `setup`, the
/// dispatcher — killed the process and left nothing behind for the user or a
/// bug report. The hook runs *before* the abort, and writes **synchronously**
/// to the newest log file: the non-blocking tracing writer lives on another
/// thread whose buffer an abort would discard, so routing the record through
/// `tracing` would lose exactly the line that matters. The previous hook
/// (stderr in dev) still runs afterwards.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());
        let thread = std::thread::current().name().map(str::to_string);
        let line = panic_record(
            &crate::queue::job::now_rfc3339(),
            location.as_deref(),
            &payload,
            thread.as_deref(),
        );
        // Nothing useful can be done with a failure here: we are already
        // panicking, and stderr below is the last resort.
        let _ = append_panic_record(&paths::log_dir(), today_utc(), &line);
        previous(info);
    }));
}

/// One panic as a JSONL record shaped like the `tracing` JSON layer's own
/// (`timestamp`/`level`/`fields`/`target`), so log tooling reads it the same
/// way. Pure, for testing.
fn panic_record(
    timestamp: &str,
    location: Option<&str>,
    payload: &str,
    thread: Option<&str>,
) -> String {
    let record = serde_json::json!({
        "timestamp": timestamp,
        "level": "ERROR",
        "fields": {
            "event": "panic",
            "message": "Pluryprint panicked",
            "location": location,
            "payload": payload,
            "thread": thread,
        },
        "target": "pluryprint::panic",
    });
    format!("{record}\n")
}

/// Append `line` to the newest log file for `today` (creating part 1 when the
/// day has none yet), bypassing the non-blocking writer.
fn append_panic_record(dir: &Path, today: Date, line: &str) -> io::Result<()> {
    paths::ensure_dir(dir)?;
    let date = date_string(today);
    let mut part = 1;
    while part < u32::MAX && log_path(dir, &date, part + 1).exists() {
        part += 1;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(dir, &date, part))?;
    file.write_all(line.as_bytes())?;
    file.flush()
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
    /// A writer that dates files by the real UTC clock.
    pub fn new(dir: PathBuf, policy: Policy) -> Self {
        Self::with_clock(dir, policy, Box::new(today_utc))
    }

    /// A writer that dates files by `clock` instead of the wall clock, so tests
    /// can step through a daily roll.
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
            // NFR-081: the app is resident (it waits for context-menu
            // hand-offs), so a startup-only sweep let a long session pile up
            // parts and stale days until the next launch. Every roll re-applies
            // the policy. It runs *after* the new part is open so the file we
            // are about to write is the newest one and can never be dropped.
            // Deliberately silent: this is the log writer itself, and emitting
            // a tracing event from inside it would re-enter it.
            sweep(&self.dir, (self.clock)(), &self.policy);
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
    /// One call carries one JSONL record (the JSON layer formats a whole event
    /// and writes it at once). It is written whole, after
    /// [`scrub_bridged_log_fields`], and reported as fully consumed.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let record = scrub_bridged_log_fields(buf);
        let open = self.ensure_open(record.len() as u64)?;
        open.file.write_all(&record)?;
        open.written += record.len() as u64;
        Ok(buf.len())
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

    /// NFR-082: the default filter must not switch DEBUG on for the library
    /// crate (`pluryprint_lib`), where every event lives.
    #[test]
    fn nfr_082_the_default_level_is_info_for_the_library_too() {
        use tracing_subscriber::layer::SubscriberExt;
        let filter = EnvFilter::new(DEFAULT_FILTER);
        let subscriber = tracing_subscriber::registry().with(filter);
        tracing::subscriber::with_default(subscriber, || {
            assert!(
                !tracing::enabled!(target: "pluryprint_lib::shell_menu", tracing::Level::DEBUG)
            );
            assert!(tracing::enabled!(target: "pluryprint_lib::queue", tracing::Level::INFO));
        });
    }

    /// NFR-082: a record bridged from the `log` crate loses the build
    /// machine's source paths; everything else is kept byte-for-byte.
    #[test]
    fn nfr_082_bridged_log_records_lose_build_machine_paths() {
        let line = br#"{"timestamp":"t","level":"ERROR","fields":{"message":"update failed","log.target":"tauri_plugin_updater","log.module_path":"tauri_plugin_updater::updater","log.file":"C:\\Users\\builder\\.cargo\\registry\\updater.rs","log.line":42},"target":"tauri_plugin_updater"}
"#;
        let out = scrub_bridged_log_fields(line);
        let text = std::str::from_utf8(&out).unwrap();
        assert!(!text.contains("log."), "{text}");
        assert!(!text.contains("builder"), "{text}");
        assert!(text.ends_with('\n'));
        let v: serde_json::Value = serde_json::from_str(text.trim_end()).unwrap();
        assert_eq!(v["fields"]["message"], "update failed");
        assert_eq!(v["target"], "tauri_plugin_updater");

        let plain = b"{\"fields\":{\"event\":\"app_started\"}}\n";
        assert!(matches!(
            scrub_bridged_log_fields(plain),
            std::borrow::Cow::Borrowed(_)
        ));
        let not_json = b"not json \"log.file\"\n";
        assert_eq!(&*scrub_bridged_log_fields(not_json), not_json);
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
            dir.join("pluryprint-2026-07-20.jsonl")
        );
        assert_eq!(
            log_path(dir, "2026-07-20", 3),
            dir.join("pluryprint-2026-07-20.3.jsonl")
        );
        assert_eq!(
            parse_log_name("pluryprint-2026-07-20.jsonl"),
            Some(("2026-07-20".to_string(), 1))
        );
        assert_eq!(
            parse_log_name("pluryprint-2026-07-20.3.jsonl"),
            Some(("2026-07-20".to_string(), 3))
        );
        // Anything else is somebody else's file and must be left alone.
        for stranger in [
            "pluryprint.jsonl",
            "pluryprint-2026-07-20.jsonl.bak",
            "pluryprint-not-a-date.jsonl",
            "pluryprint-2026-07-20.x.jsonl",
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
                "pluryprint-2026-07-20.2.jsonl",
                "pluryprint-2026-07-20.3.jsonl",
                "pluryprint-2026-07-20.jsonl",
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
            vec!["pluryprint-2026-07-20.jsonl", "pluryprint-2026-07-21.jsonl"]
        );
    }

    #[test]
    fn nfr_081_writer_appends_to_an_existing_file_after_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "pluryprint-2026-07-20.jsonl");
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
            touch(tmp.path(), &format!("pluryprint-{d}.jsonl"));
        }
        let stranger = touch(tmp.path(), "notes.txt");

        let removed = sweep(tmp.path(), date("2026-07-20"), &policy);

        assert_eq!(removed, 2);
        assert_eq!(
            names(tmp.path()),
            vec![
                "notes.txt",
                "pluryprint-2026-07-06.jsonl",
                "pluryprint-2026-07-20.jsonl",
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
            touch(tmp.path(), &format!("pluryprint-{d}.jsonl"));
        }
        // Same-day parts count individually, newest part last.
        touch(tmp.path(), "pluryprint-2026-07-20.jsonl");
        touch(tmp.path(), "pluryprint-2026-07-20.2.jsonl");

        let removed = sweep(tmp.path(), date("2026-07-20"), &policy);

        assert_eq!(removed, 2);
        assert_eq!(
            names(tmp.path()),
            vec![
                "pluryprint-2026-07-19.jsonl",
                "pluryprint-2026-07-20.2.jsonl",
                "pluryprint-2026-07-20.jsonl",
            ]
        );
    }

    /// A resident app never restarts, so retention must not depend on a launch:
    /// every roll (size or date) re-applies the policy.
    #[test]
    fn nfr_081_writer_sweeps_on_every_roll_not_only_at_startup() {
        let tmp = tempfile::tempdir().unwrap();
        let policy = Policy {
            max_bytes: 10,
            retain_days: 14,
            retain_files: 2,
        };
        let now = Arc::new(Mutex::new(date("2026-07-01")));
        let clock_src = now.clone();
        // Old files left by an earlier run, already past the age bound by the
        // time the date below advances.
        touch(tmp.path(), "pluryprint-2026-06-01.jsonl");
        let mut w = RollingJsonl::with_clock(
            tmp.path().to_path_buf(),
            policy,
            Box::new(move || *clock_src.lock().unwrap()),
        );

        // Ten size rolls in one long-running session.
        for _ in 0..10 {
            w.write_all(b"0123456789").unwrap();
        }
        w.flush().unwrap();
        assert_eq!(
            names(tmp.path()).len(),
            2,
            "the count bound holds while the app keeps running: {:?}",
            names(tmp.path())
        );

        // Three weeks later, without a restart: the date roll sweeps too.
        *now.lock().unwrap() = date("2026-07-22");
        w.write_all(b"later\n").unwrap();
        w.flush().unwrap();
        let left = names(tmp.path());
        assert!(
            left.iter().all(|n| n.as_str() >= "pluryprint-2026-07-08"),
            "nothing older than retain_days survives a date roll: {left:?}"
        );
        assert!(left.contains(&"pluryprint-2026-07-22.jsonl".to_string()));
    }

    #[test]
    fn nfr_033_panic_record_is_a_jsonl_error_with_location_and_payload() {
        let line = panic_record(
            "2026-07-20T10:00:00Z",
            Some("src/queue/dispatcher.rs:42:7"),
            "index out of bounds",
            Some("queue-dispatcher"),
        );
        assert!(line.ends_with('\n'), "one record per line");
        assert_eq!(line.matches('\n').count(), 1, "a single line: {line}");
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["level"], "ERROR");
        assert_eq!(v["timestamp"], "2026-07-20T10:00:00Z");
        assert_eq!(v["target"], "pluryprint::panic");
        assert_eq!(v["fields"]["event"], "panic");
        assert_eq!(v["fields"]["location"], "src/queue/dispatcher.rs:42:7");
        assert_eq!(v["fields"]["payload"], "index out of bounds");
        assert_eq!(v["fields"]["thread"], "queue-dispatcher");
    }

    #[test]
    fn nfr_033_panic_record_survives_hostile_payloads() {
        // Quotes and newlines in a panic message must not break the JSONL line.
        let line = panic_record("t", None, "bad \"value\"\nsecond line", None);
        assert_eq!(line.matches('\n').count(), 1);
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["fields"]["payload"], "bad \"value\"\nsecond line");
        assert!(v["fields"]["location"].is_null());
    }

    #[test]
    fn nfr_033_panic_record_lands_in_the_newest_part_of_today() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "pluryprint-2026-07-20.jsonl");
        touch(tmp.path(), "pluryprint-2026-07-20.2.jsonl");
        append_panic_record(tmp.path(), date("2026-07-20"), "{\"x\":1}\n").unwrap();
        let newest = std::fs::read_to_string(log_path(tmp.path(), "2026-07-20", 2)).unwrap();
        assert_eq!(newest, "{}\n{\"x\":1}\n");

        // No file yet today (a panic before the first record): part 1 is created.
        append_panic_record(tmp.path(), date("2026-07-21"), "{\"y\":2}\n").unwrap();
        let fresh = std::fs::read_to_string(log_path(tmp.path(), "2026-07-21", 1)).unwrap();
        assert_eq!(fresh, "{\"y\":2}\n");
    }

    #[test]
    fn sweep_on_a_missing_directory_is_a_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert_eq!(sweep(&missing, date("2026-07-20"), &Policy::default()), 0);
    }
}
