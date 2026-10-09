/**
 * WCAG 2.1 AA guard for the design tokens (NFR-045). The contrast rules are
 * stated in tokens.css; this keeps a future palette tweak from silently
 * dropping a text colour or a control outline below the threshold.
 */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const css = readFileSync(join(process.cwd(), 'src/styles/tokens.css'), 'utf8');

function block(theme: 'mp-light' | 'mp-dark'): Record<string, string> {
  const m = new RegExp(`\\.${theme}\\s*\\{([^}]*)\\}`).exec(css);
  if (!m?.[1]) throw new Error(`no .${theme} block`);
  const out: Record<string, string> = {};
  for (const [, name, value] of m[1].matchAll(
    /--mp-(\w+):\s*(#[0-9a-fA-F]{6})\s*;/g,
  )) {
    if (name && value) out[name] = value;
  }
  return out;
}

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [
    number,
    number,
  ];
  return (hi + 0.05) / (lo + 0.05);
}

const BACKGROUNDS = ['bg', 'surface', 'surface2'] as const;

describe.each(['mp-light', 'mp-dark'] as const)('%s tokens', (theme) => {
  const t = block(theme);
  const get = (name: string) => {
    const v = t[name];
    if (!v) throw new Error(`--mp-${name} missing in .${theme}`);
    return v;
  };

  it.each(['text', 'textMuted', 'textDim'])(
    'NFR-045: %s reaches 4.5:1 on every background text sits on',
    (fg) => {
      for (const bg of BACKGROUNDS) {
        expect(contrast(get(fg), get(bg))).toBeGreaterThanOrEqual(4.5);
      }
    },
  );

  it('NFR-045: borderStrong (form-control outline) reaches 3:1', () => {
    for (const bg of BACKGROUNDS) {
      expect(contrast(get('borderStrong'), get(bg))).toBeGreaterThanOrEqual(3);
    }
  });

  it('NFR-045: onAccent text on the accent fill reaches 4.5:1', () => {
    expect(contrast(get('onAccent'), get('accent'))).toBeGreaterThanOrEqual(
      4.5,
    );
  });
});
