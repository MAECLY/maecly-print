import { afterEach, describe, expect, it } from 'vitest';

import { bootAppearance } from './theme';

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
