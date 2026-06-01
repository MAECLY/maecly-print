/**
 * IPC-shaped fixtures (snake_case, matching the Rust `Printer` serialization)
 * for `mockIPC`-driven smoke tests. Deliberately mixes a rich color MFP (with
 * CMYK supplies + resolution) and a bare mono laser (no supplies, no dpi) to
 * exercise graceful degradation.
 */

import type { DiscoveredPrinter, Printer } from '@/ipc/bindings';

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
