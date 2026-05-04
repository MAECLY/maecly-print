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

  // Apply on mount and when mode changes.
  useEffect(() => {
    const next: ResolvedTheme = mode === 'system' ? getSystemTheme() : mode;
    setResolved(next);
    applyTheme(next);
  }, [mode]);

  // Watch system preference when mode === 'system'.
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
