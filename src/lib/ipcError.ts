/**
 * Backend error localization (FR-092).
 *
 * Every IPC failure carries a stable `code` (its family) and, for the failures
 * the backend knows how to explain, a stable `reason` plus `params`. This module
 * maps them to the active locale:
 *
 * 1. `code` + `reason` with a catalog entry → the translated sentence, filled
 *    with `params` (`ipcErrors.reasons.<code>.<reason>`).
 * 2. A known `code` without a translated reason → the translated family
 *    sentence, with the backend's English detail as `{{detail}}`
 *    (`ipcErrors.codes.<code>`).
 * 3. An unknown `code` → the raw message, unchanged.
 *
 * The English `message` is never lost: it stays on {@link IpcFailure.detail}
 * for logs and diagnostics.
 */

import {
  CATALOGS,
  getActiveLocale,
  hasTranslation,
  translate,
  type Locale,
  type TranslationKey,
} from '@/i18n';
import type { IpcError } from '@/ipc/bindings';

/** The wire shape, loosened: an old or foreign payload may lack any field. */
export type IpcErrorPayload = Partial<
  Pick<IpcError, 'code' | 'message' | 'reason' | 'params'>
>;

/** A thrown IPC failure whose `message` is already localized. */
export class IpcFailure extends Error {
  readonly code: string | undefined;
  readonly reason: string | undefined;
  readonly params: Readonly<Record<string, string>>;
  /** The backend's English detail (logs, diagnostics). */
  readonly detail: string;

  constructor(payload: IpcErrorPayload, locale?: Locale) {
    super(localizeIpcError(payload, locale));
    this.name = 'IpcFailure';
    this.code = payload.code ?? undefined;
    this.reason = payload.reason ?? undefined;
    this.params = cleanParams(payload.params);
    this.detail = payload.message ?? '';
  }
}

function cleanParams(
  params: IpcErrorPayload['params'],
): Record<string, string> {
  const out: Record<string, string> = {};
  if (!params) return out;
  for (const [k, v] of Object.entries(params)) {
    if (typeof v === 'string') out[k] = v;
  }
  return out;
}

/** Translate one IPC error payload into the active (or given) locale. */
export function localizeIpcError(
  payload: IpcErrorPayload,
  locale: Locale = getActiveLocale(),
): string {
  const catalog = CATALOGS[locale];
  const code = payload.code;
  const detail = payload.message ?? '';
  if (!code) return detail || translate(catalog, 'common', 'errors.unknown');

  const reason = payload.reason;
  if (reason) {
    const key = `ipcErrors.reasons.${code}.${reason}`;
    if (hasTranslation(catalog, 'common', key)) {
      return translate(
        catalog,
        'common',
        key as TranslationKey<'common'>,
        cleanParams(payload.params),
      );
    }
  }

  const familyKey = `ipcErrors.codes.${code}`;
  if (hasTranslation(catalog, 'common', familyKey)) {
    return translate(catalog, 'common', familyKey as TranslationKey<'common'>, {
      detail,
    });
  }
  return detail || `IPC error: ${code}`;
}

/** A user-facing message for anything a promise rejected with. */
export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
