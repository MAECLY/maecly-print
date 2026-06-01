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
  type IpcError,
  type Printer as IpcPrinter,
  type PrinterPrefs,
  type Supplies as IpcSupplies,
} from '@/ipc/bindings';

export type { PrinterPrefs, PrinterOverride } from '@/ipc/bindings';
import type {
  UiDiscoveredPrinter,
  UiPrinter,
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
  const r = await p;
  if (r.status === 'ok') return r.data;
  throw new Error(r.error.message);
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
