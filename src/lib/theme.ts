/**
 * Appearance (theme + density) — a localStorage-backed, instant-boot cache.
 *
 * Dual-write contract: appearance lives in two places. `localStorage` (the
 * keys below) is an INSTANT-BOOT CACHE — it lets the very first paint pick the
 * right theme before any IPC round-trip resolves, so the window never flashes.
 * The backend config (`app.theme` / `app.density`, via the settings store) is
 * the SOURCE OF TRUTH. On divergence the BACKEND WINS: after the settings
 * store loads, the Settings screen (and later App) reconciles the cache to the
 * persisted value. Every user change is written to BOTH — the hooks here update
 * the cache + in-memory state, and the screen persists the same value to the
 * backend. These hooks stay DOM/localStorage-only; they never call IPC, so the
 * two sides can be swapped or tested independently.
 *
 * Both halves of appearance are applied to the document root here — the theme
 * as a class ({@link applyTheme}) and the density as an attribute plus spacing
 * variables ({@link applyDensity}) — so any screen or stylesheet can follow the
 * setting without being handed it as a prop.
 */

import { useEffect, useState, useCallback, useSyncExternalStore } from 'react';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';
export type Density = 'comfortable' | 'compact';

const THEME_KEY = 'pluryprint:theme';
const DENSITY_KEY = 'pluryprint:density';

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined' || !window.matchMedia) return 'light';
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light';
}

/** Calls `onChange` whenever the OS switches between light and dark. */
function subscribeSystemTheme(onChange: () => void): () => void {
  if (typeof window === 'undefined' || !window.matchMedia) return () => {};
  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  mq.addEventListener('change', onChange);
  return () => mq.removeEventListener('change', onChange);
}

function readStoredTheme(): ThemeMode {
  if (typeof window === 'undefined') return 'system';
  let v: string | null = null;
  try {
    v = window.localStorage.getItem(THEME_KEY);
  } catch {
    /* storage blocked: fall back to the system theme */
  }
  if (v === 'light' || v === 'dark' || v === 'system') return v;
  return 'system';
}

/**
 * Put the cached appearance on the document **before React renders**.
 *
 * Every `--mp-*` colour token exists only under `.mp-light` / `.mp-dark`, and
 * the hooks below apply the class in an effect — which React runs after the
 * first paint. Without this call that first frame painted with no tokens at
 * all (a white or transparent canvas, default text) before snapping to the
 * theme. `main.tsx` calls it synchronously, so the very first frame already
 * has the right class, colour scheme and density.
 */
export function bootAppearance(): void {
  const mode = readStoredTheme();
  applyTheme(mode === 'system' ? getSystemTheme() : mode);
  applyDensity(readStoredDensity());
}

function applyTheme(resolved: ResolvedTheme) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.classList.remove('mp-light', 'mp-dark');
  root.classList.add(resolved === 'dark' ? 'mp-dark' : 'mp-light');
  // Also tell the OS so native scrollbars / form controls follow.
  root.style.colorScheme = resolved;
}

export function useTheme() {
  const [mode, setMode] = useState<ThemeMode>(readStoredTheme);
  // The OS theme is external state: read it as a store, so a change re-renders
  // with the new value instead of being copied into state by an effect.
  const system = useSyncExternalStore(
    subscribeSystemTheme,
    getSystemTheme,
    () => 'light' as const,
  );
  const resolved: ResolvedTheme = mode === 'system' ? system : mode;

  useEffect(() => {
    applyTheme(resolved);
  }, [resolved]);

  const set = useCallback((m: ThemeMode) => {
    setMode(m);
    try {
      window.localStorage.setItem(THEME_KEY, m);
    } catch {
      /* localStorage unavailable; in-memory only */
    }
  }, []);

  return { mode, resolved, setMode: set };
}

function readStoredDensity(): Density {
  if (typeof window === 'undefined') return 'comfortable';
  try {
    return window.localStorage.getItem(DENSITY_KEY) === 'compact'
      ? 'compact'
      : 'comfortable';
  } catch {
    return 'comfortable';
  }
}

/**
 * Row spacing per density, published as custom properties on the document root
 * so a screen (or a stylesheet) can follow the setting without being handed the
 * value as a prop. Density used to reach exactly one list — the Print Job file
 * table, which reads the `density` prop — and every other surface ignored it;
 * these variables are the app-wide hook that was missing.
 */
const DENSITY_VARS: Record<Density, Record<string, string>> = {
  comfortable: {
    '--mp-row-py': '14px',
    '--mp-row-gap': '20px',
    '--mp-list-py': '10px',
  },
  compact: {
    '--mp-row-py': '8px',
    '--mp-row-gap': '12px',
    '--mp-list-py': '6px',
  },
};

/**
 * Apply `d` to the document root: a `data-density` attribute (for CSS
 * selectors and for tests/E2E to assert on) plus the spacing variables above.
 * Mirrors {@link applyTheme} — appearance is applied to the DOM in one place.
 */
export function applyDensity(d: Density) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.dataset.density = d;
  for (const [name, value] of Object.entries(DENSITY_VARS[d])) {
    root.style.setProperty(name, value);
  }
}

export function useDensity() {
  const [density, setDensityState] = useState<Density>(readStoredDensity);

  // Applied on mount as well as on change, so the cached value is on the
  // document from the first paint (no reflow once the backend config lands).
  useEffect(() => {
    applyDensity(density);
  }, [density]);

  const setDensity = useCallback((d: Density) => {
    setDensityState(d);
    try {
      window.localStorage.setItem(DENSITY_KEY, d);
    } catch {
      /* localStorage unavailable */
    }
  }, []);

  return { density, setDensity };
}
