/**
 * Job error localization (FR-092).
 *
 * The queue stores every job failure as a stable `code` (plus, for codes with
 * variants, a stable `reason`) and the `params` its sentence needs. This maps
 * them to the active locale, mirroring {@link localizeIpcError}:
 *
 * 1. `code` + `reason` with a catalog entry → `queue:jobErrors.reasons.<code>.<reason>`;
 * 2. else `code` with a catalog entry → `queue:jobErrors.codes.<code>`;
 * 3. else — or when a sentence needs a param the stored error lacks (a job
 *    persisted before params existed) — the backend's English `message`.
 */

import {
  CATALOGS,
  getActiveLocale,
  hasTranslation,
  translate,
  type Locale,
  type TranslationKey,
} from '@/i18n';
import type { JobError } from '@/ipc/bindings';

/** Optional display-name lookup for the `printer` param (an OS queue id). */
export type PrinterNameOf = (id: string) => string | undefined;

/** Translate one stored job error into the active (or given) locale. */
export function localizeJobError(
  error: JobError,
  opts: { locale?: Locale; printerName?: PrinterNameOf } = {},
): string {
  const catalog = CATALOGS[opts.locale ?? getActiveLocale()];
  const params: Record<string, string> = {};
  for (const [k, v] of Object.entries(error.params ?? {})) {
    if (typeof v === 'string') params[k] = v;
  }
  if (params.printer && opts.printerName) {
    params.printer = opts.printerName(params.printer) ?? params.printer;
  }

  const keys: string[] = [];
  if (error.reason) {
    keys.push(`jobErrors.reasons.${error.code}.${error.reason}`);
  }
  keys.push(`jobErrors.codes.${error.code}`);

  for (const key of keys) {
    if (!hasTranslation(catalog, 'queue', key)) continue;
    const text = translate(
      catalog,
      'queue',
      key as TranslationKey<'queue'>,
      params,
    );
    // An unfilled placeholder means the stored error predates the params this
    // sentence needs: the English detail beats a sentence with a hole in it.
    if (/\{\{\w+\}\}/.test(text)) continue;
    return text;
  }
  return error.message;
}
