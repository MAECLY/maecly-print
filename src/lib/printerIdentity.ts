/**
 * Printer identity on the frontend.
 *
 * Every printer has a spooler **queue id** (`HP_LaserJet_Pro_M404`) and a
 * human **display name** (`HP LaserJet Pro M404`, CUPS `printer-info`). Only
 * the id is ever stored or sent to the spooler; the name is for people. A
 * reference typed by a person (an admin's `policy.json`, an old profile) may
 * hold either, so it is resolved here with the same rules as the backend's
 * `ipc::printers::match_installed`.
 */

/** What a printer dropdown needs: the value to store and the text to show. */
export interface PrinterChoice {
  id: string;
  name: string;
}

/**
 * The queue id a printer reference means, or `null` when it matches no
 * installed printer or is ambiguous. In order: an exact id, an id ignoring
 * case (CUPS queue names are case-insensitive), then a display name (exact,
 * then ignoring case) carried by exactly one printer.
 */
export function resolvePrinterId(
  printers: readonly PrinterChoice[],
  ref: string | null | undefined,
): string | null {
  const needle = ref?.trim();
  if (!needle) return null;
  const exact = printers.find((p) => p.id === needle);
  if (exact) return exact.id;
  const lower = needle.toLowerCase();
  const byId = printers.filter((p) => p.id.toLowerCase() === lower);
  if (byId.length === 1) return byId[0]!.id;
  for (const same of [
    (p: PrinterChoice) => p.name === needle,
    (p: PrinterChoice) => p.name.toLowerCase() === lower,
  ]) {
    const hits = printers.filter(same);
    if (hits.length === 1) return hits[0]!.id;
    if (hits.length > 1) return null;
  }
  return null;
}
