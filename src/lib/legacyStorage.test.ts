import { beforeEach, describe, expect, it } from 'vitest';
import { migrateLegacyStorage } from './legacyStorage';

describe('migrateLegacyStorage (ADR-0009)', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it('moves every maeclyprint: key to pluryprint: and drops the old one', () => {
    window.localStorage.setItem('maeclyprint:theme', 'dark');
    window.localStorage.setItem('maeclyprint:density', 'compact');
    window.localStorage.setItem('maeclyprint:onboarding-complete', '1');
    window.localStorage.setItem('maeclyprint:locale-detected', '1');
    window.localStorage.setItem('unrelated', 'x');

    const moved = migrateLegacyStorage();

    expect(moved.sort()).toEqual([
      'pluryprint:density',
      'pluryprint:locale-detected',
      'pluryprint:onboarding-complete',
      'pluryprint:theme',
    ]);
    expect(window.localStorage.getItem('pluryprint:theme')).toBe('dark');
    expect(window.localStorage.getItem('pluryprint:density')).toBe('compact');
    expect(window.localStorage.getItem('maeclyprint:theme')).toBeNull();
    expect(window.localStorage.getItem('unrelated')).toBe('x');
  });

  it('never overwrites a value the new name already has', () => {
    window.localStorage.setItem('maeclyprint:theme', 'dark');
    window.localStorage.setItem('pluryprint:theme', 'light');

    expect(migrateLegacyStorage()).toEqual([]);
    expect(window.localStorage.getItem('pluryprint:theme')).toBe('light');
    expect(window.localStorage.getItem('maeclyprint:theme')).toBeNull();
  });

  it('is a no-op on a second run and without storage', () => {
    window.localStorage.setItem('maeclyprint:theme', 'dark');
    migrateLegacyStorage();
    expect(migrateLegacyStorage()).toEqual([]);
    expect(migrateLegacyStorage(null)).toEqual([]);
  });
});
