/**
 * Typed IPC wrappers.
 *
 * Every wrapper calls the tauri-specta–generated typed `commands` in
 * `src/ipc/bindings.ts` and adapts the Rust (snake_case) wire shapes into the
 * UI `Ui*` types. Each rejects outside a Tauri host (e.g. the standalone Vite
 * preview) rather than returning fake data, so screens can render an explicit
 * "unavailable" state.
 */

import {
  commands,
  type AppInfo,
  type AppSettings,
  type DiscoveredPrinter as IpcDiscoveredPrinter,
  type FileInfo,
  type FolderExpansion,
  type FolderScan,
  type IpcError,
  type Job,
  type JobBatch,
  type PaperSize,
  type Printer as IpcPrinter,
  type PrinterPolicy,
  type PrinterPrefs,
  type PrintRequest,
  type Profile,
  type QueueSnapshot,
  type Selection,
  type Supplies as IpcSupplies,
  type UpdateCheck,
  type UpdateInstallResult,
  type UpdateState,
} from '@/ipc/bindings';

export type {
  PrinterPrefs,
  PrinterOverride,
  PrinterPolicy,
} from '@/ipc/bindings';
// Queue wire types, re-exported so screens/stores import them from one place
// (the print-dialog screen reuses PrintRequest/JobBatch/FileInfo/JobSettings).
export type {
  ExpansionLimits,
  FileInfo,
  FolderExpansion,
  FolderScan,
  Job,
  JobBatch,
  JobSettings,
  JobState,
  PrintRequest,
  QueueSnapshot,
  SkippedFile,
  SkipReason,
} from '@/ipc/bindings';
// Profiles/Settings/About wire types, re-exported for the same single-import
// convenience. `Profile` is the wire shape; screens render the adapted
// `UiProfile` (see `toUiProfile`).
// The context-menu handoff shapes (`cli::Selection`), re-exported so a screen
// acting on a shell-extension selection imports them from the same place.
export type { ContextAction, Selection } from '@/ipc/bindings';
export type {
  AppInfo,
  AppSettings,
  AppCfg,
  EngineCfg,
  UpdaterCfg,
  TelemetryCfg,
  UpdateChannel,
  UpdateCheck,
  UpdateInstallResult,
  UpdateState,
  Theme,
  Density,
  Profile,
  ColorMode,
  ProfileDuplex,
  PaperSize,
  Orientation,
  Scaling,
  Quality,
} from '@/ipc/bindings';
import type {
  UiDiscoveredPrinter,
  UiPrinter,
  UiProfile,
  UiQueueJob,
  UiTonerLevels,
} from '@/types/domain';
import { paperToWire } from '@/lib/profiles';

/** True when running inside the Tauri host (real IPC available). */
export function isTauri(): boolean {
  // Tauri injects __TAURI_INTERNALS__ when running inside the host.
  if (typeof window === 'undefined') return false;
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window;
}

const NO_HOST = 'Not running inside Tauri (standalone Vite preview)';

/**
 * A command that answered with an [`IpcError`]. Still an `Error` (with the
 * backend's message), so every existing `e.message` toast keeps working, but
 * it also carries the machine-readable `code` — the only safe thing to branch
 * on, since the message is prose and may name internal ids.
 */
export class IpcCallError extends Error {
  readonly code: IpcError['code'] | undefined;
  constructor(message: string, code: IpcError['code'] | undefined) {
    super(message);
    this.name = 'IpcCallError';
    this.code = code;
  }
}

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
  throw new IpcCallError(
    message ?? (code ? `IPC error: ${code}` : 'IPC error'),
    code,
  );
}

/** The IPC error code attached by {@link unwrap}, when there is one. */
export function ipcErrorCode(e: unknown): string | undefined {
  if (e instanceof Error && 'code' in e) {
    const code = (e as Error & { code?: unknown }).code;
    return typeof code === 'string' ? code : undefined;
  }
  return undefined;
}

/**
 * FR-084 "install now": download, verify (FR-082) and install (FR-083) the
 * newest release. `allowMajor` is FR-087's explicit opt-in for a major jump.
 * On Windows the app exits into the installer and this never resolves.
 */
export async function installUpdate(
  allowMajor: boolean,
): Promise<UpdateInstallResult> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.installUpdate(allowMajor));
}

/** Relaunch the app so an installed update takes effect. */
export async function restartApp(): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await commands.restartApp();
}

/**
 * FR-072: show a native OS notification when the window is in the background.
 * The backend decides whether one is warranted (window focus, the user's
 * `app.native_notifications` switch) and resolves to whether it showed one.
 */
export async function notifyNative(
  title: string,
  body: string,
): Promise<boolean> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.notifyNative(title, body));
}

/** Returns the live AppInfo from Rust, or rejects in non-Tauri contexts. */
export async function fetchAppInfo(): Promise<AppInfo> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.appInfo());
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
    ...(p.max_dpi != null ? { maxDpi: p.max_dpi } : {}),
    papers: p.papers,
    trays: p.trays.map((t) => ({
      name: t.name,
      ...(t.paper != null ? { paper: t.paper } : {}),
      ...(t.level != null ? { level: t.level } : {}),
    })),
    levels: toLevels(p.supplies),
    ...(p.default_tray != null ? { defTray: p.default_tray } : {}),
    ...(p.default_paper != null ? { defPaper: p.default_paper } : {}),
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

/**
 * Walk dropped/picked folders into printable files with the user's FR-004
 * answer (`top_level` vs `recursive`), one result per folder.
 *
 * The discarded files come back in `skipped` with a reason rather than
 * vanishing (FR-005), and the backend logs them against the app log (FR-059),
 * so the caller only has to *show* them.
 */
export async function expandFolders(
  paths: string[],
  scan: FolderScan,
): Promise<FolderExpansion[]> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.expandFolders(paths, scan));
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

/** Elapsed wall-clock time between two RFC 3339 stamps, or `null`. */
function elapsedMs(from: string | null, to: string | null): number | null {
  if (!from || !to) return null;
  const start = new Date(from).getTime();
  const end = new Date(to).getTime();
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return null;
  return end - start;
}

/**
 * The job shape the queue views render: `UiQueueJob` plus three facts the
 * shared shape cannot express.
 *
 * - `indeterminate` — the running phase reports NO completion fraction at all.
 *   The backend says so explicitly (`JobProgress { phase, percent: null }`),
 *   because no spooler reports one while printing; a bar frozen at 0 % would
 *   be a lie, so the views paint an indeterminate one instead.
 * - `durationMs` — how long a finished job took. `startedAt`/`finishedAt` are
 *   already formatted to a local clock here, so the views can no longer
 *   subtract them (the Timeline view promises durations).
 * - `noteKey` — which advisory note to show, as a key rather than English
 *   prose, so the views translate it (FR-092). A job the OS queue is holding,
 *   or one waiting on a printer that needs attention, is still `printing` —
 *   the backend reports that through the progress phase (`held`/`stopped`)
 *   and the note says it out loud.
 *
 * It is declared next to the adapter that fills it because `@/types/domain` is
 * shared by every screen, and these three fields concern only the queue.
 */
export interface UiQueueJobView extends UiQueueJob {
  indeterminate?: boolean;
  durationMs?: number;
  noteKey?:
    | 'readyToPrint'
    | 'interruptedByShutdown'
    | 'heldByPrinter'
    | 'printerNeedsAttention';
  /**
   * The raw RFC 3339 `finished_at`, for surfaces that must show a *date* in
   * the app's own locale (Home's recent activity outlives the day: it is
   * restored from `queue.json`). `finishedAt` is only a local clock.
   */
  finishedAtIso?: string;
}

/**
 * Adapt a wire [`Job`] into the flat `UiQueueJobView` the queue views render.
 *
 * Status mapping: the wire `JobState` and the UI status union are the same
 * eight states, so `state` passes through verbatim (StatusDot, the list/screen
 * "active" group, and the timeline already render `converted`). The only view
 * that lacks a `converted` bucket is the board — see QueueBoardView, which
 * folds it into the "queued" column. To keep the obscure `converted` state
 * ("PDF cached, waiting for a free printer") legible we attach a
 * `readyToPrint` note. Shutdown-recovered jobs come back `paused` with
 * `error.code === 'interrupted_by_shutdown'`; we surface that as a resumable
 * note rather than a hard error (the state is paused, not failed).
 */
export function toUiQueueJob(job: Job): UiQueueJobView {
  // `percent: null` means "running, but no fraction is knowable" — distinct
  // from "no progress reported at all" (`progress: null`) and from 0 %.
  const percent = job.progress?.percent ?? null;
  const ui: UiQueueJobView = {
    id: job.id,
    file: job.file_name,
    ext: job.input_ext,
    status: job.state,
    progress: percent ?? (job.state === 'done' ? 1 : 0),
    pages: job.pages ?? null,
    profile: job.profile_name ?? '—',
    printer: job.printer_resolved ?? job.settings.printer ?? 'Default printer',
    startedAt: toClock(job.started_at),
    sizeKb: Math.round(job.input_bytes / 1024),
  };
  if (job.progress != null && percent === null) ui.indeterminate = true;
  const duration = elapsedMs(job.started_at, job.finished_at);
  if (duration !== null) ui.durationMs = duration;
  if (job.finished_at) {
    ui.finishedAt = toClock(job.finished_at);
    ui.finishedAtIso = job.finished_at;
  }
  if (job.error) {
    if (job.error.code === 'interrupted_by_shutdown') {
      ui.noteKey = 'interruptedByShutdown';
    } else {
      ui.error = job.error.message;
    }
  } else if (job.state === 'converted') {
    ui.noteKey = 'readyToPrint';
  } else if (job.state === 'printing' && job.progress?.phase === 'held') {
    ui.noteKey = 'heldByPrinter';
  } else if (job.state === 'printing' && job.progress?.phase === 'stopped') {
    ui.noteKey = 'printerNeedsAttention';
  }
  return ui;
}

// ---------------------------------------------------------------------------
// Profiles — the wire `Profile` (lowercase enums) is adapted to the display-
// cased `UiProfile` the editor renders, and back. `builtin`/`default` are
// server-computed: whatever we send for them is ignored, and the returned
// value always reflects the current truth.
// ---------------------------------------------------------------------------

/** Wire paper size (lowercase) → the display case the UI shows. */
const PAPER_TO_UI: Record<PaperSize, UiProfile['paper']> = {
  a4: 'A4',
  letter: 'Letter',
  legal: 'Legal',
  a3: 'A3',
};

/** Adapt a wire [`Profile`] into the display-cased `UiProfile` the editor uses. */
export function toUiProfile(p: Profile): UiProfile {
  // color_mode/duplex/orientation/scaling/quality share the profile vocabulary
  // with the UI, so they pass through verbatim; only paper is re-cased and
  // page_range is renamed (and dropped when absent).
  const ui: UiProfile = {
    id: p.id,
    name: p.name,
    builtin: p.builtin,
    default: p.default,
    printer: p.printer,
    color: p.color_mode,
    duplex: p.duplex,
    copies: p.copies,
    paper: PAPER_TO_UI[p.paper_size] ?? p.paper_size,
    orientation: p.orientation,
    scaling: p.scaling,
    quality: p.quality,
    tray: p.tray,
    collate: p.collate ?? true,
  };
  if (p.page_range != null) ui.pageRange = p.page_range;
  return ui;
}

/**
 * Adapt a `UiProfile` back into the wire [`Profile`]. Inverse of
 * {@link toUiProfile}: `pageRange` is omitted when empty/absent (the wire omits
 * `page_range` for "all pages"), and `builtin`/`default` are passed as-is since
 * the server ignores them.
 */
function toWireProfile(p: UiProfile): Profile {
  const wire: Profile = {
    id: p.id,
    name: p.name,
    builtin: p.builtin,
    default: p.default ?? false,
    printer: p.printer,
    color_mode: p.color,
    duplex: p.duplex,
    copies: p.copies,
    paper_size: paperToWire(p.paper),
    orientation: p.orientation,
    scaling: p.scaling,
    quality: p.quality,
    tray: p.tray,
    collate: p.collate ?? true,
  };
  if (p.pageRange) wire.page_range = p.pageRange;
  return wire;
}

/** Every profile: built-ins first, then customs, each with its computed flags. */
export async function listProfiles(): Promise<UiProfile[]> {
  if (!isTauri()) throw new Error(NO_HOST);
  return (await unwrap(commands.listProfiles())).map(toUiProfile);
}

/**
 * Create or update a custom profile (upsert), returning the saved profile with
 * its server-computed flags. An empty id creates; an unknown id creates
 * preserving that id (client-side undo); a built-in id is rejected.
 */
export async function saveProfile(p: UiProfile): Promise<UiProfile> {
  if (!isTauri()) throw new Error(NO_HOST);
  return toUiProfile(await unwrap(commands.saveProfile(toWireProfile(p))));
}

/** Delete a custom profile (rejects a built-in or the current default). */
export async function deleteProfile(id: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.deleteProfile(id));
}

/** Set the app's default profile (id must name a known built-in or custom). */
export async function setDefaultProfile(id: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.setDefaultProfile(id));
}

// ---------------------------------------------------------------------------
// App settings — the four wire sections of `config.toml`. Several fields skip-
// serialize when unset (telemetry consent, density), so we normalize them to
// concrete values the UI controls can bind to without undefined checks.
// ---------------------------------------------------------------------------

/** Fill in the optional serde-default fields so the UI never sees `undefined`. */
function normalizeAppSettings(s: AppSettings): AppSettings {
  return {
    ...s,
    app: { ...s.app, density: s.app.density ?? 'comfortable' },
    telemetry: {
      crash_reports: s.telemetry.crash_reports ?? false,
      anonymous_usage_metrics: s.telemetry.anonymous_usage_metrics ?? false,
    },
  };
}

/** Read the current app settings, with optional fields normalized. */
export async function getAppSettings(): Promise<AppSettings> {
  if (!isTauri()) throw new Error(NO_HOST);
  return normalizeAppSettings(await unwrap(commands.getAppSettings()));
}

/** Validate and persist app settings; returns the saved (normalized) value. */
export async function setAppSettings(s: AppSettings): Promise<AppSettings> {
  if (!isTauri()) throw new Error(NO_HOST);
  return normalizeAppSettings(await unwrap(commands.setAppSettings(s)));
}

// ---------------------------------------------------------------------------
// Updates & app links
// ---------------------------------------------------------------------------

/** Check GitHub for a newer release than the running build. */
export async function checkForUpdates(): Promise<UpdateCheck> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.checkForUpdates());
}

/**
 * The persisted update bookkeeping: when the last check ran and which release
 * the user skipped (FR-084).
 */
export async function getUpdateState(): Promise<UpdateState> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.getUpdateState());
}

/**
 * Record the release the user chose to skip, or `null` to clear it.
 *
 * Skipping is per version: a newer release prompts again, so this is a "not
 * this one" rather than an opt-out of updates.
 */
export async function setSkippedUpdateVersion(
  version: string | null,
): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.setSkippedUpdateVersion(version));
}

// ---------------------------------------------------------------------------
// Shell-extension selections (FR-001, NFR-014)
// ---------------------------------------------------------------------------

/**
 * Drain the file selections the OS context menu handed this instance, oldest
 * first.
 *
 * Covers both arrival routes: a selection the shell extension put on this
 * process' command line before the webview existed, and one a *second* launch
 * (or the macOS Finder service) forwarded into an already-running window. The
 * backend parks both in the same store, so this call is the single point of
 * delivery — and it is destructive, so a selection is acted on exactly once no
 * matter how many times it is called.
 */
export async function takePendingSelections(): Promise<Selection[]> {
  if (!isTauri()) throw new Error(NO_HOST);
  return unwrap(commands.takePendingSelections());
}

/** Reveal the app's log directory in the OS file browser. */
export async function openLogsFolder(): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.openLogsFolder());
}

/** Open a prefilled "new issue" page on the project's GitHub, seeded with `text`. */
export async function reportIssue(text: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.reportIssue(text));
}

/** Open an external link in the default browser (allow-listed project hosts). */
export async function openExternal(url: string): Promise<void> {
  if (!isTauri()) throw new Error(NO_HOST);
  await unwrap(commands.openExternal(url));
}
