/**
 * Domain types — the UI-facing (`Ui*`) shapes the screens render.
 *
 * The wire contract lives in the auto-generated `src/ipc/bindings.ts`
 * (snake_case, matching the Rust serialization). The adapters in
 * `src/lib/ipc.ts` (`toUiPrinter`, `toUiQueueJob`, `toUiProfile`, …) translate
 * those wire shapes into the `Ui*` types below — display-cased enums, flattened
 * fields, and camelCase names. Treat these as the source of truth for component
 * props; the wire types are re-exported from `src/lib/ipc.ts` where a screen
 * needs them directly.
 */

// Note: the wire types for the queue and the print dialog (Job, JobSettings,
// QueueSnapshot, JobBatch, PrintRequest, PaperSize, …) live in the
// auto-generated `src/ipc/bindings.ts` and are re-exported from
// `src/lib/ipc.ts`. `JobState` is kept here because the `Ui*` shapes below and
// `StatusDot` render it directly.
export type JobState =
  | 'queued'
  | 'converting'
  | 'converted'
  | 'printing'
  | 'done'
  | 'failed'
  | 'paused'
  | 'cancelled';

/**
 * Lightweight UI-only representations the screens render. They are populated
 * from live IPC via the `toUi*` adapters in `src/lib/ipc.ts`.
 */
export type UiPrinterType = 'laser' | 'mfp' | 'wide' | 'label' | 'virtual';

/** Status the Printers screen renders (distinct from the IPC `PrinterStatus`). */
export type UiPrinterStatus = 'ready' | 'offline' | 'error' | 'busy';

/** MaeclyPrint-managed state for a printer (independent of the OS spooler). */
export type UiPrinterState = 'default' | 'active' | 'disabled';

export interface UiTray {
  name: string;
  /** Loaded paper/media, if the OS reports it (most don't per-tray). */
  paper?: string;
  /** Fill level, 0–1, if the device reports it. */
  level?: number;
}

/** Toner/ink supply levels (0–1). Empty for devices with no reported supplies. */
export type UiTonerLevels = Partial<
  Record<'cyan' | 'magenta' | 'yellow' | 'black', number>
>;

export interface UiPrinter {
  id: string;
  name: string;
  type: UiPrinterType;
  color: boolean;
  duplex: boolean;
  status: UiPrinterStatus;
  isDefault: boolean;
  location: string;
  /** Connection / port string, e.g. "USB001" or "10.0.0.41 (IPP)". */
  conn: string;
  driver: string;
  shared: boolean;
  queue: number;
  /** Max resolution in DPI — absent when the OS/driver doesn't report it. */
  maxDpi?: number;
  papers: string[];
  trays: UiTray[];
  levels: UiTonerLevels;
  defTray?: string;
  defPaper?: string;
}

/** A printer surfaced by a network scan but not yet added. */
export interface UiDiscoveredPrinter {
  id: string;
  name: string;
  conn: string;
  type: UiPrinterType;
  /** Discovery transport, e.g. "mDNS / Bonjour" or "WSD". */
  via: string;
}

export interface UiProfile {
  id: string;
  name: string;
  builtin: boolean;
  default?: boolean;
  printer: string;
  color: 'color' | 'monochrome';
  duplex: 'off' | 'long-edge' | 'short-edge';
  copies: number;
  paper: 'A4' | 'A3' | 'Letter' | 'Legal' | string;
  orientation: 'portrait' | 'landscape' | 'auto';
  scaling: 'fit' | 'shrink' | 'none';
  quality: 'draft' | 'normal' | 'high';
  tray: string;
  /**
   * Collate multiple copies (whole sets, 1-2-3 1-2-3) rather than grouping each
   * page (1-1 2-2 3-3). Absent means collated, the backend's default.
   */
  collate?: boolean;
  /** Page-range expression (e.g. "1-5,8"); absent means all pages. */
  pageRange?: string;
}

export interface UiFile {
  id: string;
  name: string;
  ext: string;
  kb: number;
  pages?: number | null;
  /** Absolute path on disk — the print order is this list's order. */
  path: string;
}

export interface UiQueueJob {
  id: string;
  file: string;
  ext: string;
  status: JobState;
  progress: number;
  pages: number;
  profile: string;
  printer: string;
  startedAt: string;
  finishedAt?: string;
  sizeKb: number;
  note?: string;
  error?: string;
}
