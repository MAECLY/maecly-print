import { describe, it, expect } from 'vitest';

import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  findProfileIn,
  isValidPageRange,
  nextUniqueName,
  toJobSettings,
  validateProfileName,
} from './profiles';
import type { UiProfile } from '@/types/domain';

/** Build a UiProfile with sensible custom defaults; override per test. */
function mk(over: Partial<UiProfile> = {}): UiProfile {
  return {
    id: 'c1',
    name: 'Custom',
    builtin: false,
    printer: '<default>',
    color: 'color',
    duplex: 'off',
    copies: 1,
    paper: 'A4',
    orientation: 'auto',
    scaling: 'fit',
    quality: 'normal',
    tray: 'auto',
    ...over,
  };
}

describe('findProfileIn', () => {
  it('returns the exact match when the id is present', () => {
    const a = mk({ id: 'a', name: 'A' });
    const b = mk({ id: 'b', name: 'B', default: true });
    expect(findProfileIn([a, b], 'a')).toBe(a);
  });

  it('falls back to the default profile when the id is absent', () => {
    const a = mk({ id: 'a' });
    const b = mk({ id: 'b', default: true });
    expect(findProfileIn([a, b], 'nope')).toBe(b);
    // A null/undefined id skips straight to the default.
    expect(findProfileIn([a, b], null)).toBe(b);
  });

  it('falls back to the first profile when none is marked default', () => {
    const a = mk({ id: 'a' });
    const b = mk({ id: 'b' });
    expect(findProfileIn([a, b], 'nope')).toBe(a);
  });

  it('falls back to the first built-in for an empty list', () => {
    expect(findProfileIn([], 'anything')).toBe(BUILT_IN_PROFILES[0]);
  });
});

describe('toJobSettings', () => {
  it('maps duplex, lowercases paper, and nulls the default printer', () => {
    const p = mk({
      printer: '<default>',
      duplex: 'long-edge',
      color: 'monochrome',
      paper: 'Letter',
    });
    expect(toJobSettings(p, {})).toMatchObject({
      printer: null,
      duplex: 'long',
      color_mode: 'monochrome',
      paper_size: 'letter',
    });
  });

  it('passes a concrete printer through unchanged', () => {
    expect(toJobSettings(mk({ printer: 'HL_L2350' }), {}).printer).toBe(
      'HL_L2350',
    );
  });

  it('uses the profile page range when no override is given', () => {
    expect(toJobSettings(mk({ pageRange: '1-5' }), {}).page_ranges).toBe('1-5');
  });

  it('lets an override range take precedence over the profile range', () => {
    expect(
      toJobSettings(mk({ pageRange: '1-5' }), { range: '2-4' }).page_ranges,
    ).toBe('2-4');
  });

  it('treats an all-pages profile as null page_ranges', () => {
    expect(toJobSettings(mk({}), {}).page_ranges).toBeNull();
  });

  it('applies scalar overrides on top of the profile baseline', () => {
    const p = mk({ copies: 1, color: 'color' });
    expect(toJobSettings(p, { copies: 3, color: 'monochrome' })).toMatchObject({
      copies: 3,
      color_mode: 'monochrome',
    });
  });
});

describe('buildPrintRequest', () => {
  it('carries the files and names the profile', () => {
    const req = buildPrintRequest(['a.pdf'], mk({ name: 'Quick' }), {});
    expect(req.profile_name).toBe('Quick');
    expect(req.files).toEqual(['a.pdf']);
  });
});

describe('nextUniqueName', () => {
  it('returns the base name when it is free', () => {
    expect(nextUniqueName('New profile', [])).toBe('New profile');
  });

  it('appends the first free numeric suffix', () => {
    const existing = [
      mk({ name: 'New profile' }),
      mk({ name: 'New profile 2' }),
    ];
    expect(nextUniqueName('New profile', existing)).toBe('New profile 3');
  });

  it('compares case-insensitively', () => {
    expect(
      nextUniqueName('Copy of Quick', [mk({ name: 'copy of quick' })]),
    ).toBe('Copy of Quick 2');
  });
});

describe('isValidPageRange (mirrors the Rust engine grammar)', () => {
  it('treats blank / whitespace-only as "all pages"', () => {
    expect(isValidPageRange('')).toBe(true);
    expect(isValidPageRange('   ')).toBe(true);
  });

  it('accepts single pages, spans, and comma lists', () => {
    expect(isValidPageRange('1')).toBe(true);
    expect(isValidPageRange('1-5')).toBe(true);
    expect(isValidPageRange('1-5,10,12-14')).toBe(true);
  });

  it('tolerates whitespace around commas and the dash', () => {
    expect(isValidPageRange(' 1 - 3 , 7 ')).toBe(true);
    expect(isValidPageRange('  1-5 ,  10  ')).toBe(true);
  });

  it('rejects whitespace INSIDE a page number (the engine parses per token)', () => {
    // Previously the client stripped all whitespace and read "1 2" as "12",
    // which the Rust parser rejects — a client/server divergence.
    expect(isValidPageRange('1 2')).toBe(false);
    expect(isValidPageRange('1 2-3')).toBe(false);
    expect(isValidPageRange('12 3-40')).toBe(false);
  });

  it('rejects descending spans, zero, open-ended spans, and empty entries', () => {
    expect(isValidPageRange('5-1')).toBe(false);
    expect(isValidPageRange('0')).toBe(false);
    expect(isValidPageRange('1-0')).toBe(false);
    expect(isValidPageRange('5-')).toBe(false);
    expect(isValidPageRange('-5')).toBe(false);
    expect(isValidPageRange('1-2-3')).toBe(false);
    expect(isValidPageRange('1,,3')).toBe(false);
    expect(isValidPageRange('1,')).toBe(false);
    expect(isValidPageRange('abc')).toBe(false);
  });
});

describe('validateProfileName', () => {
  it('rejects an empty or whitespace-only name', () => {
    expect(validateProfileName('', [])).toMatch(/enter a profile name/i);
    expect(validateProfileName('   ', [])).toMatch(/enter a profile name/i);
  });

  it('rejects a name over 100 characters', () => {
    expect(validateProfileName('x'.repeat(101), [])).toMatch(/100 characters/i);
  });

  it('rejects a case-insensitive duplicate', () => {
    const existing = [mk({ id: 'a', name: 'Contracts' })];
    expect(validateProfileName('contracts', existing)).toMatch(
      /already exists/i,
    );
  });

  it('allows a profile to keep its own name (selfId excluded)', () => {
    const existing = [mk({ id: 'a', name: 'Contracts' })];
    expect(validateProfileName('Contracts', existing, 'a')).toBeNull();
  });

  it('accepts a unique, trimmed, in-length name', () => {
    expect(
      validateProfileName('  Fresh name  ', [mk({ name: 'Other' })]),
    ).toBeNull();
  });
});
