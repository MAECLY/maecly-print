/**
 * Built-in print profiles and the profile → wire-settings mapping.
 *
 * `BUILT_IN_PROFILES` are local presets only in M1: they seed the Print Job
 * screen's settings panel, but the per-invocation overrides panel is the real
 * settings source (FR-040/041). A profile is just a starting point; whatever
 * the user tweaks in the overrides wins. `buildPrintRequest` collapses a
 * profile baseline plus its overrides into the exact [`PrintRequest`] the
 * backend expects (see `src/ipc/bindings.ts`).
 */

import type {
  Duplex,
  JobSettings,
  PaperSize,
  PrintRequest,
} from '@/ipc/bindings';
import type { UiProfile } from '@/types/domain';

export const BUILT_IN_PROFILES: UiProfile[] = [
  {
    id: 'p1',
    name: 'Quick',
    builtin: true,
    default: true,
    printer: '<default>',
    color: 'color',
    duplex: 'off',
    copies: 1,
    paper: 'A4',
    orientation: 'auto',
    scaling: 'fit',
    quality: 'normal',
    tray: 'auto',
  },
  {
    id: 'p2',
    name: 'Duplex BW',
    builtin: true,
    printer: '<default>',
    color: 'monochrome',
    duplex: 'long-edge',
    copies: 1,
    paper: 'A4',
    orientation: 'auto',
    scaling: 'fit',
    quality: 'normal',
    tray: 'auto',
  },
  {
    id: 'p3',
    name: 'Photo Color',
    builtin: true,
    printer: '<default>',
    color: 'color',
    duplex: 'off',
    copies: 1,
    paper: 'A4',
    orientation: 'auto',
    scaling: 'none',
    quality: 'high',
    tray: 'auto',
  },
  {
    id: 'p4',
    name: 'Press A · Duplex Long',
    builtin: false,
    printer: 'Sharp MX-3071',
    color: 'color',
    duplex: 'long-edge',
    copies: 1,
    paper: 'A3',
    orientation: 'auto',
    scaling: 'fit',
    quality: 'high',
    tray: 'Tray 2',
  },
  {
    id: 'p5',
    name: 'Letterhead Stationery',
    builtin: false,
    printer: 'HP LaserJet Pro M404dn',
    color: 'monochrome',
    duplex: 'off',
    copies: 1,
    paper: 'Letter',
    orientation: 'portrait',
    scaling: 'fit',
    quality: 'high',
    tray: 'Tray 2',
  },
  {
    id: 'p6',
    name: 'Zebra Labels',
    builtin: false,
    printer: 'Zebra ZD420',
    color: 'monochrome',
    duplex: 'off',
    copies: 1,
    paper: 'Letter',
    orientation: 'portrait',
    scaling: 'none',
    quality: 'normal',
    tray: 'Roll',
  },
];

/** Sentinel used in the UI for "use the app/OS default printer". */
export const DEFAULT_PRINTER = '<default>';

/** Per-invocation tweaks layered on top of the selected profile's baseline. */
export interface PrintJobOverrides {
  printer?: string;
  duplex?: UiProfile['duplex'];
  color?: UiProfile['color'];
  copies?: number;
  paper?: string;
  orientation?: UiProfile['orientation'];
  scaling?: UiProfile['scaling'];
  range?: string;
}

/** Resolve a profile id to its object, falling back to the first profile. */
export function findProfile(id: string): UiProfile {
  return BUILT_IN_PROFILES.find((p) => p.id === id) ?? BUILT_IN_PROFILES[0]!;
}

/** Copies must be an integer in `1..=999` (the server is authoritative too). */
export function isValidCopies(raw: string): boolean {
  const s = raw.trim();
  if (!/^\d+$/.test(s)) return false;
  const n = Number(s);
  return n >= 1 && n <= 999;
}

/**
 * Client-side mirror of the engine's page-range grammar: comma-separated
 * tokens, each `N` or `N-M` (1-based, `M >= N`). Empty means "all pages". The
 * server re-validates; this only gates the UI so the button reflects intent.
 */
export function isValidPageRange(raw: string): boolean {
  // Whitespace is insignificant anywhere ("1 - 3, 7" is valid), matching the
  // Rust grammar; pages are 1-based u32s, so cap what the backend can hold.
  const MAX_PAGE = 0xffff_ffff;
  const s = raw.replace(/\s+/g, '');
  if (s === '') return true;
  for (const token of s.split(',')) {
    const m = /^(\d+)(?:-(\d+))?$/.exec(token);
    if (!m) return false;
    const start = Number(m[1]);
    if (start < 1 || start > MAX_PAGE) return false;
    if (m[2] !== undefined) {
      const end = Number(m[2]);
      if (end < 1 || end > MAX_PAGE || end < start) return false;
    }
  }
  return true;
}

/** UI duplex vocabulary → wire `Duplex`. */
const DUPLEX_TO_WIRE: Record<UiProfile['duplex'], Duplex> = {
  off: 'none',
  'long-edge': 'long',
  'short-edge': 'short',
};

/**
 * Collapse a profile baseline plus its overrides into a wire [`JobSettings`].
 * The mapping is intentionally faithful — no hidden flags — so the backend's
 * `preview_command` and the actual dispatch agree with what the UI shows.
 */
export function toJobSettings(
  profileId: string,
  overrides: PrintJobOverrides,
): JobSettings {
  const merged = { ...findProfile(profileId), ...overrides };
  const printer =
    !merged.printer || merged.printer === DEFAULT_PRINTER
      ? null
      : merged.printer;
  const range = overrides.range?.trim();
  return {
    printer,
    duplex: DUPLEX_TO_WIRE[merged.duplex],
    color_mode: merged.color,
    copies: merged.copies,
    paper_size: merged.paper.toLowerCase() as PaperSize,
    orientation: merged.orientation,
    scaling: merged.scaling,
    page_ranges: range ? range : null,
  };
}

/** Build the full [`PrintRequest`] for `paths` under the given profile/overrides. */
export function buildPrintRequest(
  paths: string[],
  profileId: string,
  overrides: PrintJobOverrides,
): PrintRequest {
  return {
    files: paths,
    settings: toJobSettings(profileId, overrides),
    profile_name: findProfile(profileId).name,
  };
}
