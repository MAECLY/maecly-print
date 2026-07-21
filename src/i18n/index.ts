/**
 * i18n core (FR-091 … FR-094) — a small, dependency-free translation layer.
 *
 * Resources live in `locales/<lang>/<namespace>.json`, one file per namespace
 * and language so that screens can be translated independently without ever
 * touching the same file. English is the SOURCE OF TRUTH: the key type of every
 * namespace is derived from the English JSON (so `t()` autocompletes and a
 * misspelled key fails to compile), and a missing translation falls back to the
 * English string instead of showing a raw key to the user.
 *
 * Everything here is pure and DOM-free; React binding lives in `I18nProvider`.
 */

import enAbout from './locales/en/about.json';
import enCommon from './locales/en/common.json';
import enHome from './locales/en/home.json';
import enPrintJob from './locales/en/printJob.json';
import enPrinters from './locales/en/printers.json';
import enProfiles from './locales/en/profiles.json';
import enQueue from './locales/en/queue.json';
import enSettings from './locales/en/settings.json';

import esAbout from './locales/es/about.json';
import esCommon from './locales/es/common.json';
import esHome from './locales/es/home.json';
import esPrintJob from './locales/es/printJob.json';
import esPrinters from './locales/es/printers.json';
import esProfiles from './locales/es/profiles.json';
import esQueue from './locales/es/queue.json';
import esSettings from './locales/es/settings.json';

// ---------------------------------------------------------------------------
// Locales
// ---------------------------------------------------------------------------

/** BCP-47 tags the app ships translations for. */
export const SUPPORTED_LOCALES = ['en-US', 'es-ES'] as const;

export type Locale = (typeof SUPPORTED_LOCALES)[number];

/**
 * Fallback locale, and the same default the backend writes into `config.toml`
 * (`AppCfg::locale`). Also the language every missing key falls back to.
 */
export const DEFAULT_LOCALE: Locale = 'en-US';

/** Narrow an arbitrary string (a config value, a browser tag) to a `Locale`. */
export function isLocale(value: string): value is Locale {
  return (SUPPORTED_LOCALES as readonly string[]).includes(value);
}

/**
 * Best-effort match of a BCP-47 tag onto a supported locale: exact tag first
 * (`es-ES`), then the language subtag (`es`, `es-MX` → `es-ES`). Returns
 * `undefined` when nothing matches, so callers decide their own default.
 */
export function matchLocale(tag: string | undefined): Locale | undefined {
  if (!tag) return undefined;
  if (isLocale(tag)) return tag;
  const language = tag.split('-')[0]?.toLowerCase();
  if (!language) return undefined;
  return SUPPORTED_LOCALES.find((l) => l.split('-')[0] === language);
}

// ---------------------------------------------------------------------------
// Catalogs
// ---------------------------------------------------------------------------

const EN = {
  common: enCommon,
  home: enHome,
  printJob: enPrintJob,
  queue: enQueue,
  printers: enPrinters,
  profiles: enProfiles,
  settings: enSettings,
  about: enAbout,
};

/** One language's resources: every namespace, same shape as English. */
export type LocaleCatalog = typeof EN;

/** Namespace names — one JSON file per screen, plus the shared `common`. */
export type Namespace = keyof LocaleCatalog;

export const NAMESPACES = Object.keys(EN) as readonly Namespace[];

// Annotating Spanish as `LocaleCatalog` makes a key present in English but
// missing in Spanish a COMPILE error, not a runtime fallback. The parity test
// in `i18n.test.ts` covers the other direction (extra/empty Spanish keys).
const ES: LocaleCatalog = {
  common: esCommon,
  home: esHome,
  printJob: esPrintJob,
  queue: esQueue,
  printers: esPrinters,
  profiles: esProfiles,
  settings: esSettings,
  about: esAbout,
};

export const CATALOGS: Record<Locale, LocaleCatalog> = {
  'en-US': EN,
  'es-ES': ES,
};

// ---------------------------------------------------------------------------
// Key types
// ---------------------------------------------------------------------------

/** `count`-aware plural keys (`file_one`) are addressed by their base name. */
type PluralBase<K extends string> = K extends `${infer B}_one`
  ? B
  : K extends `${infer B}_other`
    ? B
    : never;

/** Dot-joined paths to every leaf string, with plural suffixes collapsed. */
type Paths<T> = {
  [K in keyof T & string]: T[K] extends string
    ? K | PluralBase<K>
    : T[K] extends object
      ? `${K}.${Paths<T[K]>}`
      : never;
}[keyof T & string];

/** Every key `t()` accepts for a namespace, e.g. `'actions.cancel'`. */
export type TranslationKey<N extends Namespace> = Paths<LocaleCatalog[N]>;

/** Interpolation values; a numeric `count` also selects the plural form. */
export type TranslateParams = Readonly<Record<string, string | number>>;

// ---------------------------------------------------------------------------
// translate()
// ---------------------------------------------------------------------------

/** Walk a dot path through a catalog, returning the leaf string if there is one. */
function lookup(root: unknown, path: string): string | undefined {
  let node: unknown = root;
  for (const segment of path.split('.')) {
    if (typeof node !== 'object' || node === null) return undefined;
    node = (node as Record<string, unknown>)[segment];
  }
  return typeof node === 'string' ? node : undefined;
}

/**
 * Resolve `key` in one namespace, honouring the `_one` / `_other` suffixes when
 * a numeric `count` is supplied. English pluralization rules (`1` is singular)
 * also hold for Spanish, so one selector serves both shipped locales.
 */
function resolve(
  bundle: unknown,
  key: string,
  count: number | undefined,
): string | undefined {
  if (count !== undefined) {
    const suffixed = lookup(bundle, `${key}${count === 1 ? '_one' : '_other'}`);
    if (suffixed !== undefined) return suffixed;
  }
  return lookup(bundle, key);
}

/** Replace every `{{param}}` with its value; unknown params are left as-is. */
function interpolate(template: string, params: TranslateParams | undefined) {
  if (!params) return template;
  return template.replace(/\{\{(\w+)\}\}/g, (match, name: string) => {
    const value = params[name];
    return value === undefined ? match : String(value);
  });
}

/** Missing keys are reported once per key so a render loop can't spam the console. */
const warned = new Set<string>();

function warnMissing(ns: string, key: string) {
  if (!import.meta.env.DEV) return;
  const id = `${ns}.${key}`;
  if (warned.has(id)) return;
  warned.add(id);
  console.warn(`[i18n] missing translation for "${id}"`);
}

/**
 * Translate `key` of `ns` using `catalog`, falling back to English and finally
 * to the literal key. Never throws: a missing key is a cosmetic defect, not a
 * reason to blank the window.
 */
export function translate<N extends Namespace>(
  catalog: LocaleCatalog,
  ns: N,
  key: TranslationKey<N>,
  params?: TranslateParams,
): string {
  const count = typeof params?.count === 'number' ? params.count : undefined;
  const hit = resolve(catalog[ns], key, count) ?? resolve(EN[ns], key, count);
  if (hit === undefined) {
    warnMissing(ns, key);
    return key;
  }
  return interpolate(hit, params);
}
