import { describe, it, expect } from 'vitest';

import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  findPageRangeOverflow,
  findProfileIn,
  isValidPageRange,
  parsePageRange,
  nextUniqueName,
  toJobSettings,
  toWireTray,
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

  it('FR-041: an emptied range field overrides the profile back to all pages', () => {
    // '' is not nullish, so it must win over the profile's saved range — this
    // is the only way to print every page under a range-carrying profile, and
    // the screen's derived field relies on it.
    expect(
      toJobSettings(mk({ pageRange: '1-5' }), { range: '' }).page_ranges,
    ).toBeNull();
  });

  it('treats an all-pages profile as null page_ranges', () => {
    expect(toJobSettings(mk({}), {}).page_ranges).toBeNull();
  });

  it('FR-041: carries the profile quality and tray to the engine', () => {
    // The profile editor lets you set these and profiles.toml stores them, so
    // dropping them here would make both controls decorative.
    expect(
      toJobSettings(mk({ quality: 'high', tray: 'Tray 2' }), {}),
    ).toMatchObject({ quality: 'high', tray: 'Tray 2' });
    expect(toJobSettings(mk({ quality: 'draft' }), {}).quality).toBe('draft');
    // 'auto' is the UI's "no choice", which the wire spells as null.
    expect(toJobSettings(mk({ tray: 'auto' }), {}).tray).toBeNull();
  });

  it('applies scalar overrides on top of the profile baseline', () => {
    const p = mk({ copies: 1, color: 'color' });
    expect(toJobSettings(p, { copies: 3, color: 'monochrome' })).toMatchObject({
      copies: 3,
      color_mode: 'monochrome',
    });
  });
});

describe('toWireTray', () => {
  it('FR-041: keeps a driver bin name verbatim', () => {
    // Bin names are model-specific; normalizing them would select the wrong
    // tray on some device we have never seen.
    expect(toWireTray('Tray 2')).toBe('Tray 2');
    expect(toWireTray('Kassette 1')).toBe('Kassette 1');
    expect(toWireTray('  Drawer 1  ')).toBe('Drawer 1');
  });

  it('FR-041: maps every "no choice" spelling to null', () => {
    // A blank would otherwise reach the driver as an empty `InputSlot=`.
    for (const empty of [
      'auto',
      'AUTO',
      ' Auto ',
      '',
      '   ',
      null,
      undefined,
    ]) {
      expect(toWireTray(empty), String(empty)).toBeNull();
    }
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

describe('parsePageRange', () => {
  it('FR-042: returns the spans of a valid expression', () => {
    expect(parsePageRange('1-5,10')).toEqual([
      { start: 1, end: 5 },
      { start: 10, end: 10 },
    ]);
  });

  it('FR-042: an empty expression means "all pages", not an error', () => {
    expect(parsePageRange('')).toEqual([]);
    expect(parsePageRange('   ')).toEqual([]);
  });

  it('FR-042: returns null for every shape isValidPageRange rejects', () => {
    for (const bad of ['5-1', '0', '1-0', '5-', '-5', '1-2-3', '1,,3', 'abc']) {
      expect(parsePageRange(bad), bad).toBeNull();
      expect(isValidPageRange(bad), bad).toBe(false);
    }
  });
});

describe('findPageRangeOverflow', () => {
  const pdf = (name: string, pages: number | null) => ({
    name,
    ext: 'pdf',
    pages,
  });

  it('FR-042: names the first PDF the range runs past', () => {
    expect(
      findPageRangeOverflow('2-12', [pdf('long.pdf', 40), pdf('short.pdf', 8)]),
    ).toEqual({ name: 'short.pdf', pages: 8 });
  });

  it('FR-042: accepts a range that fits every document', () => {
    expect(
      findPageRangeOverflow('1-8', [pdf('a.pdf', 40), pdf('b.pdf', 8)]),
    ).toBeNull();
  });

  it('FR-042: a blank range never overflows', () => {
    expect(findPageRangeOverflow('', [pdf('a.pdf', 1)])).toBeNull();
  });

  it('FR-042: images are exempt — the engine drops the range for them', () => {
    expect(
      findPageRangeOverflow('2-3', [
        { name: 'chart.png', ext: 'png', pages: 1 },
      ]),
    ).toBeNull();
  });

  it('FR-042: an unknown page count cannot be bounded', () => {
    expect(findPageRangeOverflow('900', [pdf('huge.pdf', null)])).toBeNull();
    expect(
      findPageRangeOverflow('900', [{ name: 'huge.pdf', ext: 'pdf' }]),
    ).toBeNull();
  });

  it('FR-042: a syntactically invalid range reports nothing (one error at a time)', () => {
    expect(findPageRangeOverflow('5-1', [pdf('a.pdf', 1)])).toBeNull();
  });
});
