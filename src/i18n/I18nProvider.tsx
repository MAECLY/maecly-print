/**
 * React binding for the i18n core: publishes the active locale and hands
 * screens a namespace-bound `t()` via {@link useT}.
 *
 * The active locale comes from the persisted app config (`settings.app.locale`,
 * FR-094 — the Settings screen writes it). FR-093: on the FIRST run the config
 * still holds the backend's default (`en-US`), which says nothing about the
 * user's language, so we detect it from the OS and persist that instead.
 *
 * The OS language is read from `navigator.language`, not from the backend: the
 * webview inherits the user's system/UI language from the host OS, so it is
 * already the right answer, and reading it needs no IPC round-trip — the very
 * first paint is in the right language instead of flashing English while a
 * command resolves. There is no Rust-side locale command to call either.
 */

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  type ReactNode,
} from 'react';

import { useSettingsStore } from '@/stores/settingsStore';

import {
  CATALOGS,
  DEFAULT_LOCALE,
  matchLocale,
  translate,
  type Locale,
  type Namespace,
  type TranslateParams,
  type TranslationKey,
} from './index';

/** Marks that the first-run OS detection already happened (FR-093, once ever). */
const DETECTED_KEY = 'maeclyprint:locale-detected';

interface I18nContextValue {
  locale: Locale;
}

// Outside a provider (isolated component tests, the error boundary before the
// app mounts) the default locale keeps rendering deterministic instead of
// throwing.
const I18nContext = createContext<I18nContextValue>({ locale: DEFAULT_LOCALE });

/** The OS/browser UI language, narrowed to a supported locale. */
export function detectSystemLocale(): Locale {
  if (typeof navigator === 'undefined') return DEFAULT_LOCALE;
  return matchLocale(navigator.language) ?? DEFAULT_LOCALE;
}

function readDetectedFlag(): boolean {
  try {
    return window.localStorage.getItem(DETECTED_KEY) === '1';
  } catch {
    // Private-mode / opaque-origin webviews can throw; detecting again is
    // harmless, so treat it as "not detected yet".
    return false;
  }
}

function writeDetectedFlag() {
  try {
    window.localStorage.setItem(DETECTED_KEY, '1');
  } catch {
    /* noop */
  }
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const settings = useSettingsStore((s) => s.settings);
  const patch = useSettingsStore((s) => s.patch);
  const configured = settings?.app.locale;

  // First run: the config still carries the backend default, so prefer what the
  // OS says and write it back once (so Settings shows the real value, and the
  // choice survives even if the OS language later changes).
  useEffect(() => {
    if (!configured || readDetectedFlag()) return;
    writeDetectedFlag();
    const system = detectSystemLocale();
    if (configured === DEFAULT_LOCALE && system !== DEFAULT_LOCALE) {
      void patch({ app: { locale: system } });
    }
  }, [configured, patch]);

  // Until the config is loaded (and outside the desktop app, where it never
  // loads) the OS language is the best guess — better than flashing English.
  const locale = useMemo<Locale>(
    () => matchLocale(configured) ?? detectSystemLocale(),
    [configured],
  );

  const value = useMemo<I18nContextValue>(() => ({ locale }), [locale]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/** The active locale, for callers that format dates/numbers themselves. */
export function useLocale(): Locale {
  return useContext(I18nContext).locale;
}

/** A translation function bound to `ns`, e.g. `const t = useT('common')`. */
export type TFunction<N extends Namespace> = (
  key: TranslationKey<N>,
  params?: TranslateParams,
) => string;

/**
 * Namespace-bound translator. Re-renders with the rest of the tree whenever the
 * active locale changes, so no screen needs to watch the setting itself.
 */
export function useT<N extends Namespace>(ns: N): TFunction<N> {
  const locale = useLocale();
  return useMemo<TFunction<N>>(
    () => (key, params) => translate(CATALOGS[locale], ns, key, params),
    [locale, ns],
  );
}
