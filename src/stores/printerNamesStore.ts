/**
 * Printer display names, by OS queue id.
 *
 * Jobs name their printer by the id the OS spooler uses (`EPSON_L15160`),
 * while Print Job and Printers show the driver's display name
 * (`EPSON L15160 Series`). The queue views, Home's recent activity and job
 * error messages look names up here so a printer reads the same everywhere.
 *
 * The list is fetched lazily on first use and again when a job names a printer
 * the last list did not have (one refetch per unknown id, so a printer that is
 * really gone never loops). Screens that already enumerate printers prime it
 * with {@link primePrinterNames}. Outside the Tauri host, or when enumeration
 * fails, lookups simply return `undefined` and callers fall back to the id.
 */

import { create } from 'zustand';

import { isTauri, listPrinters } from '@/lib/ipc';
import type { UiPrinter } from '@/types/domain';

interface PrinterNamesState {
  names: Readonly<Record<string, string>>;
  loading: boolean;
}

export const usePrinterNamesStore = create<PrinterNamesState>(() => ({
  names: {},
  loading: false,
}));

/** Ids already refetched for, so an unknown id triggers at most one reload. */
const askedFor = new Set<string>();
let loadedOnce = false;

/** Record the names from a printer list another screen already fetched. */
export function primePrinterNames(printers: readonly UiPrinter[]): void {
  const names: Record<string, string> = {};
  for (const p of printers) names[p.id] = p.name;
  loadedOnce = true;
  usePrinterNamesStore.setState({ names });
}

async function load(): Promise<void> {
  if (!isTauri() || usePrinterNamesStore.getState().loading) return;
  usePrinterNamesStore.setState({ loading: true });
  try {
    primePrinterNames(await listPrinters());
  } catch {
    // Names are cosmetic: keep showing ids rather than an error.
  } finally {
    loadedOnce = true;
    usePrinterNamesStore.setState({ loading: false });
  }
}

/**
 * Ensure `ids` can be named: load the list on first use, and reload once for
 * an id the current list does not know.
 */
export function requestPrinterNames(ids: Iterable<string>): void {
  if (!isTauri()) return;
  if (!loadedOnce) {
    void load();
    return;
  }
  const { names } = usePrinterNamesStore.getState();
  let missing = false;
  for (const id of ids) {
    if (names[id] === undefined && !askedFor.has(id)) {
      askedFor.add(id);
      missing = true;
    }
  }
  if (missing) void load();
}

/** Test hook: forget everything. */
export function resetPrinterNames(): void {
  askedFor.clear();
  loadedOnce = false;
  usePrinterNamesStore.setState({ names: {}, loading: false });
}
