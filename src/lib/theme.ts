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
 */

import { useEffect, useState, useCallback } from 'react';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';
export type Density = 'comfortable' | 'compact';

const THEME_KEY = 'maeclyprint:theme';
const DENSITY_KEY = 'maeclyprint:density';

function getSystemTheme(): ResolvedTheme {
  if (typeof window === 'undefined' || !window.matchMedia) return 'light';
  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light';
}

function readStoredTheme(): ThemeMode {
  if (typeof window === 'undefined') return 'system';
  const v = window.localStorage.getItem(THEME_KEY);
  if (v === 'light' || v === 'dark' || v === 'system') return v;
  return 'system';
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
  const [resolved, setResolved] = useState<ResolvedTheme>(() =>
    readStoredTheme() === 'system'
      ? getSystemTheme()
      : (readStoredTheme() as ResolvedTheme),
  );

  useEffect(() => {
    const next: ResolvedTheme = mode === 'system' ? getSystemTheme() : mode;
    setResolved(next);
    applyTheme(next);
  }, [mode]);

  useEffect(() => {
    if (mode !== 'system') return;
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => {
      const next: ResolvedTheme = mq.matches ? 'dark' : 'light';
      setResolved(next);
      applyTheme(next);
    };
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, [mode]);

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
  const v = window.localStorage.getItem(DENSITY_KEY);
  return v === 'compact' ? 'compact' : 'comfortable';
}

export function useDensity() {
  const [density, setDensityState] = useState<Density>(readStoredDensity);

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
