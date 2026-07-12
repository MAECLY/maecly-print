/**
 * IPC-shaped fixtures (snake_case, matching the Rust `Printer` serialization)
 * for `mockIPC`-driven smoke tests. Deliberately mixes a rich color MFP (with
 * CMYK supplies + resolution) and a bare mono laser (no supplies, no dpi) to
 * exercise graceful degradation.
 */

import type {
  DiscoveredPrinter,
  Job,
  JobError,
  JobProgress,
  JobSettings,
  JobState,
  Printer,
  QueueSnapshot,
} from '@/ipc/bindings';

export const fixturePrinters: Printer[] = [
  {
    id: 'EPSON_L15160',
    name: 'EPSON L15160 Series',
    kind: 'mfp',
    color: true,
    duplex: true,
    status: 'ready',
    is_default: true,
    location: 'Front office',
    connection: 'EPSON L15160 Series (IPP)',
    driver: 'EPSON L15160 Series-AirPrint',
    shared: false,
    queued_jobs: 0,
    max_dpi: 1200,
    papers: ['A4', 'Letter', 'Legal'],
    trays: [{ name: 'Tray 1', paper: null, level: null }],
    supplies: { cyan: 0.52, magenta: 0.51, yellow: 0.55, black: 0.38 },
    default_tray: null,
    default_paper: 'A4',
  },
  {
    id: 'HL_L2350',
    name: 'Brother HL-L2350DW',
    kind: 'laser',
    color: false,
    duplex: true,
    status: 'ready',
    is_default: false,
    location: '',
    connection: 'USB',
    driver: 'Brother HL-L2350DW',
    shared: false,
    queued_jobs: 0,
    max_dpi: null,
    papers: ['A4', 'Letter'],
    trays: [{ name: 'Tray 1', paper: null, level: null }],
    // No reported supplies — the UI must hide the supply section.
    supplies: { cyan: null, magenta: null, yellow: null, black: null },
    default_tray: null,
    default_paper: null,
  },
];

export const fixtureDiscovered: DiscoveredPrinter[] = [
  {
    id: 'ipp://canon.local/ipp/print',
    name: 'Canon TR8600',
    connection: 'Canon TR8600 (IPP)',
    kind: 'mfp',
    via: 'mDNS · IPPS',
  },
];

/**
 * A virtual printer (e.g. macOS "Save as PDF" / Windows "Microsoft Print to
 * PDF"). `kind: 'virtual'` maps to `type: 'virtual'` in the UI, which must
 * hide the Remove action but keep Restore/soft-disable working.
 */
export const fixtureVirtualPrinter: Printer = {
  id: 'SAVE_AS_PDF',
  name: 'Save as PDF',
  kind: 'virtual',
  color: true,
  duplex: false,
  status: 'ready',
  is_default: false,
  location: '',
  connection: 'Virtual',
  driver: 'Save as PDF',
  shared: false,
  queued_jobs: 0,
  max_dpi: null,
  papers: ['A4', 'Letter'],
  trays: [{ name: 'Tray 1', paper: null, level: null }],
  supplies: { cyan: null, magenta: null, yellow: null, black: null },
  default_tray: null,
  default_paper: null,
};

/** The base fixtures plus a virtual printer (3 configured). */
export const fixturePrintersWithVirtual: Printer[] = [
  ...fixturePrinters,
  fixtureVirtualPrinter,
];

// ---------------------------------------------------------------------------
// Queue — wire-shaped `Job`s (snake_case, matching the Rust serialization).
// ---------------------------------------------------------------------------

const baseSettings: JobSettings = {
  printer: null,
  duplex: 'none',
  color_mode: 'monochrome',
  copies: 1,
  paper_size: 'a4',
  orientation: 'auto',
  scaling: 'fit',
  page_ranges: null,
};

interface JobOverrides {
  id: string;
  state: JobState;
  file_name: string;
  pages?: number | null;
  progress?: JobProgress | null;
  error?: JobError | null;
  profile_name?: string | null;
  printer_resolved?: string | null;
  started_at?: string | null;
  finished_at?: string | null;
}

/** Build a wire `Job` with realistic defaults; override only what a test varies. */
export function mkJob(o: JobOverrides): Job {
  const ext = o.file_name.includes('.')
    ? (o.file_name.split('.').pop() ?? '')
    : '';
  return {
    id: o.id,
    batch_id: 'batch-1',
    state: o.state,
    input_path: `/tmp/${o.file_name}`,
    file_name: o.file_name,
    input_ext: ext,
    input_bytes: 102400,
    pages: o.pages ?? 3,
    cache_pdf: null,
    settings: baseSettings,
    profile_name: o.profile_name ?? 'Duplex BW',
    printer_resolved: o.printer_resolved ?? 'HP LaserJet Pro M404dn',
    attempts: 1,
    error: o.error ?? null,
    progress: o.progress ?? null,
    submitted_at: '2026-07-11T10:42:00Z',
    started_at:
      o.started_at === undefined ? '2026-07-11T10:42:01Z' : o.started_at,
    finished_at: o.finished_at ?? null,
  };
}

/** One job in every lifecycle state (incl. converted + interrupted-by-shutdown). */
export const fixtureJobs: Job[] = [
  mkJob({
    id: 'j-printing',
    state: 'printing',
    file_name: 'Q3-Financial-Report.pdf',
    progress: { phase: 'printing', percent: 0.62 },
  }),
  mkJob({
    id: 'j-converting',
    state: 'converting',
    file_name: 'Contract-AcmeCorp.docx',
    progress: { phase: 'converting', percent: 0.34 },
  }),
  mkJob({ id: 'j-converted', state: 'converted', file_name: 'Exhibit-A.pdf' }),
  mkJob({
    id: 'j-queued',
    state: 'queued',
    file_name: 'Exhibit-B.pdf',
    started_at: null,
  }),
  mkJob({
    id: 'j-paused',
    state: 'paused',
    file_name: 'Schedule-Q4.xlsx',
    started_at: null,
  }),
  mkJob({
    id: 'j-interrupted',
    state: 'paused',
    file_name: 'Floorplan-Ground.pdf',
    error: {
      code: 'interrupted_by_shutdown',
      message: 'Interrupted while the app was closing',
    },
  }),
  mkJob({
    id: 'j-done',
    state: 'done',
    file_name: 'Site-Photo-North.jpg',
    profile_name: 'Photo Color',
    finished_at: '2026-07-11T10:39:31Z',
  }),
  mkJob({
    id: 'j-failed',
    state: 'failed',
    file_name: 'Report-Broken.pdf',
    error: { code: 'print_failed', message: 'Paper jam in Tray 2' },
    finished_at: '2026-07-11T10:38:55Z',
  }),
  mkJob({
    id: 'j-cancelled',
    state: 'cancelled',
    file_name: 'Draft-Old.pdf',
    finished_at: '2026-07-11T10:40:00Z',
  }),
];

/** A full snapshot: 3 active · 3 pending · 3 done, queue running. */
export const fixtureQueueSnapshot: QueueSnapshot = {
  schema_version: 1,
  queue_paused: false,
  jobs: fixtureJobs,
};
