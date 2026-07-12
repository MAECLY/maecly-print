/**
 * Typed IPC wrappers.
 *
 * Printer commands call the tauri-specta–generated typed `commands` in
 * `src/ipc/bindings.ts` and adapt the Rust (snake_case) shapes into the UI
 * `Ui*` types. `app_info` still uses a hand-written wrapper. Every wrapper
 * rejects outside a Tauri host (e.g. the standalone Vite preview) rather than
 * returning fake data, so screens can render an explicit "unavailable" state.
 */

import { invoke } from '@tauri-apps/api/core';

import {
  commands,
  type DiscoveredPrinter as IpcDiscoveredPrinter,
  type FileInfo,
  type IpcError,
  type Job,
  type JobBatch,
  type Printer as IpcPrinter,
  type PrinterPolicy,
  type PrinterPrefs,
  type PrintRequest,
  type QueueSnapshot,
  type Supplies as IpcSupplies,
} from '@/ipc/bindings';

export type {
  PrinterPrefs,
  PrinterOverride,
  PrinterPolicy,
} from '@/ipc/bindings';
// Queue wire types, re-exported so screens/stores import them from one place
// (the print-dialog lane reuses PrintRequest/JobBatch/FileInfo/JobSettings).
export type {
  FileInfo,
  Job,
  JobBatch,
  JobSettings,
  JobState,
  PrintRequest,
  QueueSnapshot,
} from '@/ipc/bindings';
import type {
  UiDiscoveredPrinter,
  UiPrinter,
  UiQueueJob,
  UiTonerLevels,
} from '@/types/domain';

/**
 * Shape returned by the Rust `app_info` command.
 * Mirrors `src-tauri/src/ipc/diag.rs::AppInfo`.
 */
export interface AppInfoLike {
  version: string;
  build_date: string;
  config_dir: string;
  data_dir: string;
  log_dir: string;
}

/** True when running inside the Tauri host (real IPC available). */
export function isTauri(): boolean {
  // Tauri injects __TAURI_INTERNALS__ when running inside the host.
  if (typeof window === 'undefined') return false;
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
}

const NO_HOST = 'Not running inside Tauri (standalone Vite preview)';

/** Unwrap a tauri-specta `Result`, turning the error branch into a throw. */
async function unwrap<T>(
  p: Promise<{ status: 'ok'; data: T } | { status: 'error'; error: IpcError }>,
): Promise<T> {
  // The generated bindings resolve to the error branch for command-returned
  // IpcErrors, but the invoke itself can also REJECT with a plain string (e.g.
  // "command not allowed" from the Tauri layer) — normalize both into a
  // readable Error instead of surfacing "undefined" toasts.
  let r;
  try {
    r = await p;
  } catch (e) {
    throw e instanceof Error ? e : new Error(String(e));
  }
  if (r.status === 'ok') return r.data;
  const { message, code } = {
    message: r.error?.message,
    code: r.error?.code,
  };
  throw new Error(message ?? (code ? `IPC error: ${code}` : 'IPC error'));
}

/** Returns the live AppInfo from Rust, or rejects in non-Tauri contexts. */
export async function fetchAppInfo(): Promise<AppInfoLike> {
  if (!isTauri()) throw new Error(NO_HOST);
  return invoke<AppInfoLike>('app_info');
}

// ---------------------------------------------------------------------------
// Printers
// ---------------------------------------------------------------------------

function toLevels(s: IpcSupplies): UiTonerLevels {
  const out: UiTonerLevels = {};
  if (s.cyan != null) out.cyan = s.cyan;
  if (s.magenta != null) out.magenta = s.magenta;
  if (s.yellow != null) out.yellow = s.yellow;
  if (s.black != null) out.black = s.black;
  return out;
}

function toUiPrinter(p: IpcPrinter): UiPrinter {
  return {
    id: p.id,
    name: p.name,
    type: p.kind,
    color: p.color,
    duplex: p.duplex,
    status: p.status,
    isDefault: p.is_default,
    location: p.location,
    conn: p.connection,
    driver: p.driver,
    shared: p.shared,
    queue: p.queued_jobs,
    maxDpi: p.max_dpi ?? undefined,
    papers: p.papers,
    trays: p.trays.map((t) => ({
      name: t.name,
      paper: t.paper ?? undefined,
      level: t.level ?? undefined,
    })),
    levels: toLevels(p.supplies),
    defTray: p.default_tray ?? undefined,
    defPaper: p.default_paper ?? undefined,
  };
}

function toUiDiscovered(d: IpcDiscoveredPrinter): UiDiscoveredPrinter {
  return {
    id: d.id,
    name: d.name,
    conn: d.connection,
    type: d.kind,
    via: d.via,
  };
}

/** Every installed printer with its OS-reported capabilities. */
export async function listPrinters(): Promise<UiPrinter[]> {
  if (!isTauri()) throw new Error(NO_HOST);
  return (await unwrap(commands.listPrinters())).map(toUiPrinter);
}

/** Id of the OS default printer, or null when none is configured. */
export async function fetchDefaultPrinter(): Promise<string | null> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.defaultPrinter());
}

/** Best-effort network discovery of not-yet-installed printers. */
export async function discoverPrinters(): Promise<UiDiscoveredPrinter[]> {
  if (!isTauri()) throw new Error(NO_HOST);
  return (await unwrap(commands.discoverPrinters())).map(toUiDiscovered);
}

// ---------------------------------------------------------------------------
// Printer preferences (persisted) + OS actions
// ---------------------------------------------------------------------------

/** Read the persisted MaeclyPrint printer preferences. */
export async function getPrinterPrefs(): Promise<PrinterPrefs> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.getPrinterPrefs());
}

/** Persist printer preferences and return the saved value. */
export async function setPrinterPrefs(
  prefs: PrinterPrefs,
): Promise<PrinterPrefs> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.setPrinterPrefs(prefs));
}

/** Set the OS user-default printer (no elevation required). */
export async function setDefaultPrinter(name: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.setDefaultPrinter(name));
}

/** Send a test page to the named printer. */
export async function printTestPage(name: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.printTestPage(name));
}

/** Open the OS "Printers" settings pane. */
export async function openPrinterSettings(): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.openPrinterSettings());
}

/**
 * Install a print queue into the OS spooler from a device URI (lpadmin IPP
 * Everywhere on macOS/Linux; Microsoft IPP Class Driver on Windows). On
 * failure the spooler's own diagnostic is surfaced verbatim.
 */
export async function addPrinter(name: string, uri: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.addPrinter(name, uri));
}

/** Read the effective organization printer policy (`policy.json`). */
export async function getPrinterPolicy(): Promise<PrinterPolicy> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.getPrinterPolicy());
}

// ---------------------------------------------------------------------------
// Queue — job lifecycle. Every wrapper follows the printer pattern: reject
// outside the Tauri host, else unwrap the tauri-specta Result (error → throw).
// ---------------------------------------------------------------------------

/** Submit a print request; returns the batch id and per-file job ids. */
export async function printFiles(req: PrintRequest): Promise<JobBatch> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.printFiles(req));
}

/** A consistent snapshot of the whole queue for the initial paint. */
export async function queueStatus(): Promise<QueueSnapshot> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.queueStatus());
}

/** Cancel a single job by id (any non-terminal state). */
export async function cancelJob(id: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.cancelJob(id));
}

/** Cancel every non-terminal job; resolves with how many were cancelled. */
export async function cancelAll(): Promise<number> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.cancelAll());
}

/** Pause a single job (queued/converting/converted, not printing). */
export async function pauseJob(id: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.pauseJob(id));
}

/** Resume a single paused job. */
export async function resumeJob(id: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.resumeJob(id));
}

/** Retry a single failed job from the start. */
export async function retryJob(id: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.retryJob(id));
}

/** Pause the whole queue (stops starting new work; in-flight jobs finish). */
export async function pauseQueue(): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.pauseQueue());
}

/** Resume the whole queue. */
export async function resumeQueue(): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.resumeQueue());
}

/** Remove all terminal jobs; resolves with how many were removed. */
export async function clearDone(): Promise<number> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.clearDone());
}

/** Inspect a list of file paths for the print dialog. */
export async function inspectFiles(paths: string[]): Promise<FileInfo[]> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.inspectFiles(paths));
}

/** The exact command MaeclyPrint would run for `req` (honesty preview). */
export async function previewCommand(req: PrintRequest): Promise<string> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.previewCommand(req));
}

/**
 * Format an RFC 3339 UTC timestamp to a local `HH:MM:SS` clock, matching the
 * preformatted strings the queue views render. `null`/unparseable → `—`.
 */
function toClock(rfc3339: string | null): string {
  if (!rfc3339) return '—';
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return '—';
  return d.toLocaleTimeString([], {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

/**
 * Adapt a wire [`Job`] into the flat `UiQueueJob` the queue views render.
 *
 * Status mapping: the wire `JobState` and the UI status union are the same
 * eight states, so `state` passes through verbatim (StatusDot, the list/screen
 * "active" group, and the timeline already render `converted`). The only view
 * that lacks a `converted` bucket is the board — see QueueBoardView, which
 * folds it into the "queued" column. To keep the obscure `converted` state
 * ("PDF cached, waiting for a free printer") legible we attach a
 * `Ready to print` note. Shutdown-recovered jobs come back `paused` with
 * `error.code === 'interrupted_by_shutdown'`; we surface that as a resumable
 * note rather than a hard error (the state is paused, not failed).
 */
export function toUiQueueJob(job: Job): UiQueueJob {
  const ui: UiQueueJob = {
    id: job.id,
    file: job.file_name,
    ext: job.input_ext,
    status: job.state,
    progress: job.progress?.percent ?? (job.state === 'done' ? 1 : 0),
    pages: job.pages ?? 0,
    profile: job.profile_name ?? '—',
    printer: job.printer_resolved ?? job.settings.printer ?? 'Default printer',
    startedAt: toClock(job.started_at),
    sizeKb: Math.round(job.input_bytes / 1024),
  };
  if (job.finished_at) ui.finishedAt = toClock(job.finished_at);
  if (job.error) {
    if (job.error.code === 'interrupted_by_shutdown') {
      ui.note = 'Interrupted by shutdown · resume to continue';
    } else {
      ui.error = job.error.message;
    }
  } else if (job.state === 'converted') {
    ui.note = 'Ready to print';
  }
  return ui;
}
