import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { act, renderHook } from '@testing-library/react';

import { bootAppearance, useTheme } from './theme';

describe('bootAppearance', () => {
  afterEach(() => {
    window.localStorage.removeItem('pluryprint:theme');
    window.localStorage.removeItem('pluryprint:density');
    document.documentElement.classList.remove('mp-light', 'mp-dark');
    delete document.documentElement.dataset.density;
  });

  it('puts the cached theme and density on the document synchronously (no flash)', () => {
    window.localStorage.setItem('pluryprint:theme', 'dark');
    window.localStorage.setItem('pluryprint:density', 'compact');

    bootAppearance();

    // No render, no effect: the class is there for the very first paint.
    expect(document.documentElement.classList.contains('mp-dark')).toBe(true);
    expect(document.documentElement.style.colorScheme).toBe('dark');
    expect(document.documentElement.dataset.density).toBe('compact');
  });

  it('falls back to a themed document when nothing is cached', () => {
    bootAppearance();
    const cls = document.documentElement.classList;
    expect(cls.contains('mp-light') || cls.contains('mp-dark')).toBe(true);
    expect(document.documentElement.dataset.density).toBe('comfortable');
  });
});

describe('useTheme', () => {
  const original = window.matchMedia;
  let dark = false;
  const listeners = new Set<() => void>();

  beforeEach(() => {
    dark = false;
    listeners.clear();
    window.matchMedia = ((query: string) => ({
      get matches() {
        return dark;
      },
      media: query,
      addEventListener: (_: string, cb: () => void) => listeners.add(cb),
      removeEventListener: (_: string, cb: () => void) => listeners.delete(cb),
    })) as unknown as typeof window.matchMedia;
  });

  afterEach(() => {
    window.matchMedia = original;
    window.localStorage.removeItem('pluryprint:theme');
    document.documentElement.classList.remove('mp-light', 'mp-dark');
  });

  const osTurns = (next: boolean) =>
    act(() => {
      dark = next;
      listeners.forEach((cb) => cb());
    });

  it('follows the OS theme while the mode is system', () => {
    const { result } = renderHook(() => useTheme());
    expect(result.current.mode).toBe('system');
    expect(result.current.resolved).toBe('light');
    osTurns(true);
    expect(result.current.resolved).toBe('dark');
    expect(document.documentElement.classList.contains('mp-dark')).toBe(true);
  });

  it('ignores the OS theme once a mode is chosen', () => {
    const { result } = renderHook(() => useTheme());
    act(() => result.current.setMode('light'));
    osTurns(true);
    expect(result.current.resolved).toBe('light');
    expect(document.documentElement.classList.contains('mp-light')).toBe(true);
    act(() => result.current.setMode('system'));
    expect(result.current.resolved).toBe('dark');
  });
});
