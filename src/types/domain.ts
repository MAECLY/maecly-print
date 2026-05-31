/**
 * Domain types — frontend mirror of the IPC contract.
 *
 * These shapes match the documented schema in
 * `docs/04-architecture/ipc-and-cli.md` §2.2 and `data-model.md`.
 *
 * In M1 these are superseded by the auto-generated `bindings.ts` produced
 * by tauri-specta. For this scaffold (visual-only), they are the source
 * of truth for component props and mock data.
 */

export type PrinterStatus =
  | 'ready'
  | 'offline'
  | 'paused'
  | 'out_of_paper'
  | 'out_of_toner'
  | 'jammed'
  | 'needs_attention';

export interface PrinterInfo {
  name: string;
  port: string;
  driver: string;
  is_default: boolean;
  is_local: boolean;
  status: PrinterStatus;
}

export type ColorMode = 'color' | 'monochrome';
export type Duplex = 'off' | 'long-edge' | 'short-edge';
export type Scaling = 'fit' | 'shrink' | 'actual' | 'none';
export type Quality = 'draft' | 'normal' | 'high';
export type Orientation = 'portrait' | 'landscape' | 'auto';

export interface Profile {
  id: string;
  name: string;
  /** "<default>" sentinel or a printer name. */
  printer: string;
  color_mode: ColorMode;
  duplex: Duplex;
  copies: number;
  paper_size: string;
  orientation: Orientation;
  scaling: Scaling;
  page_range: string;
  collate: boolean;
  quality: Quality;
  tray: string;
  margins_mm: { top: number; right: number; bottom: number; left: number };
  notes: string;
  /** UI-only marker — not part of TOML schema. */
  builtin?: boolean;
  /** UI-only marker — not part of TOML schema. */
  default?: boolean;
}

export interface PrintRequest {
  files: string[];
  profile_id: string;
  overrides?: Partial<Omit<Profile, 'id' | 'name' | 'builtin' | 'default'>>;
}

export interface JobBatch {
  batch_id: string;
  job_ids: string[];
}

export type JobState =
  | 'queued'
  | 'converting'
  | 'converted'
  | 'printing'
  | 'done'
  | 'failed'
  | 'paused'
  | 'cancelled';

export interface Job {
  id: string;
  batch_id?: string;
  state: JobState;
  input_path: string;
  input_mime: string;
  input_bytes: number;
  cache_pdf: string | null;
  profile_id: string;
  attempts: number;
  error: { code: string; message: string } | null;
  progress: { phase: string; percent: number } | null;
  submitted_at: string;
  started_at: string | null;
  finished_at: string | null;
}

export interface QueueSnapshot {
  schema_version: number;
  jobs: Job[];
}

export interface UpdateInfo {
  version: string;
  notes: string;
  pub_date: string;
  size_bytes: number;
}

/**
 * Lightweight UI-only representations used by the mock data and the
 * mockup-faithful screens in this scaffold milestone. M1 will collapse
 * these into the real `Job` and `Profile` shapes once IPC is wired.
 */
export type UiPrinterType = 'laser' | 'mfp' | 'wide' | 'label' | 'virtual';

/** Status the Printers screen renders (distinct from the IPC `PrinterStatus`). */
export type UiPrinterStatus = 'ready' | 'offline' | 'error' | 'busy';

export interface UiTray {
  name: string;
  /** Loaded paper/media, or "—" when not applicable. */
  paper: string;
  /** Fill level, 0–1. */
  level: number;
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
  /** Max resolution in DPI. */
  maxDpi: number;
  papers: string[];
  trays: UiTray[];
  levels: UiTonerLevels;
  defTray: string;
  defPaper: string;
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
}

export interface UiFile {
  id: string;
  name: string;
  ext: string;
  kb: number;
  status?: JobState;
  pages?: number | null;
  progress?: number;
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
