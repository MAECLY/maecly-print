/**
 * Job error localization (FR-092).
 *
 * The queue stores every job failure as a stable `code` (plus, for codes with
 * variants, a stable `reason`) and the `params` its sentence needs. This maps
 * them to the active locale, mirroring {@link localizeIpcError}:
 *
 * 1. `code` + `reason` with a catalog entry → `queue:jobErrors.reasons.<code>.<reason>`
 *    — or, for `dependency_missing` (whose reason is the dependency id), the
 *    shared remediation `common:deps.<id>.remediation` the Print Job screen
 *    shows too;
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
import { sentenceParams } from '@/lib/ipcError';

/** Optional display-name lookup for the `printer` param (an OS queue id). */
export type PrinterNameOf = (id: string) => string | undefined;

/** Translate one stored job error into the active (or given) locale. */
export function localizeJobError(
  error: JobError,
  opts: { locale?: Locale; printerName?: PrinterNameOf } = {},
): string {
  const catalog = CATALOGS[opts.locale ?? getActiveLocale()];
  // A numeric `count` selects a plural sentence (`_one` / `_other`).
  const params = sentenceParams(error.params);
  if (typeof params.printer === 'string' && opts.printerName) {
    params.printer = opts.printerName(params.printer) ?? params.printer;
  }

  const keys: ['queue' | 'common', string][] = [];
  if (error.reason) {
    if (error.code === 'dependency_missing') {
      keys.push(['common', `deps.${error.reason}.remediation`]);
    }
    keys.push(['queue', `jobErrors.reasons.${error.code}.${error.reason}`]);
  }
  keys.push(['queue', `jobErrors.codes.${error.code}`]);

  for (const [ns, key] of keys) {
    if (
      !hasTranslation(catalog, ns, key) &&
      !hasTranslation(catalog, ns, `${key}_other`)
    ) {
      continue;
    }
    const text =
      ns === 'common'
        ? translate(catalog, ns, key as TranslationKey<'common'>, params)
        : translate(catalog, ns, key as TranslationKey<'queue'>, params);
    // An unfilled placeholder means the stored error predates the params this
    // sentence needs: the English detail beats a sentence with a hole in it.
    if (/\{\{\w+\}\}/.test(text)) continue;
    return text;
  }
  return error.message;
}
