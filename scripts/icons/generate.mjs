#!/usr/bin/env node
/**
 * Draw the app icon ("Abanico") and rebuild every size each OS asks for.
 *
 * The mark is a printer releasing three sheets in a fan — left, up, right —
 * which is what the app does: many files out at once. This file is the single
 * source of truth for its geometry; `src/components/Brand.tsx` draws the same
 * shapes inside the app, so change both together.
 *
 * Why three sources and not one:
 *   - macOS does not mask app icons, so the art carries Apple's rounded tile
 *     itself, inset inside the canvas (about 80% of it), or it looks oversized
 *     next to every other Dock icon.
 *   - Windows draws whatever it is given edge to edge, so its tile fills the
 *     canvas.
 *   - Below ~48px the motion strokes and the text lines on the sheets are mud;
 *     the small sizes get a simplified, heavier drawing.
 *
 * Run: pnpm icons   (node scripts/icons/generate.mjs)
 */
import { execFileSync } from 'node:child_process';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const ICONS = join(REPO, 'src-tauri', 'icons');
const SOURCES = join(ICONS, 'source');

// Brand palette (src/styles/tokens.css: --mp-accent light/dark).
const ORANGE = '#C2410C';
const ORANGE_BRIGHT = '#F97316';
const INK = '#1C1917';
const INK_SOFT = '#44403C';
const SLOT = '#0C0A09';
const PAPER = '#FFFFFF';
const TILE_TOP = '#FFF7ED';
const TILE_BOTTOM = '#FED7AA';

// One sheet, anchored at the bottom centre of the printer slot and rotated.
function sheet(angle, { small, accent }) {
  const w = small ? 26 : 24;
  const h = small ? 30 : 32;
  const stroke = small ? 3 : 2;
  let lines = '';
  if (!small) {
    const widths = [w - 8, w - 8, w - 14];
    lines = widths
      .map(
        (lw, i) =>
          `<rect x="${-w / 2 + 4}" y="${-h + 6 + i * 6}" width="${lw}" height="2.6" rx="1.3" fill="${accent}"/>`,
      )
      .join('');
  }
  return `<g transform="translate(50 54) rotate(${angle})"><rect x="${-w / 2}" y="${-h}" width="${w}" height="${h}" rx="2.4" fill="${PAPER}" stroke="${INK}" stroke-width="${stroke}"/>${lines}</g>`;
}

function printer() {
  return [
    `<rect x="26" y="47" width="48" height="12" rx="4" fill="${INK_SOFT}"/>`,
    `<rect x="18" y="54" width="64" height="28" rx="7" fill="${INK}"/>`,
    `<rect x="30" y="52.5" width="40" height="3.2" rx="1.6" fill="${SLOT}"/>`,
    `<rect x="29" y="67" width="26" height="3.4" rx="1.7" fill="${INK_SOFT}"/>`,
    `<circle cx="70" cy="68.5" r="3" fill="${ORANGE_BRIGHT}"/>`,
  ].join('');
}

function mark({ small }) {
  const spread = small ? 36 : 34;
  const motion = small
    ? ''
    : `<path d="M17 24 l-5 -4 M22 15 l-3 -6 M83 24 l5 -4 M78 15 l3 -6 M50 9 v-4" stroke="${ORANGE}" stroke-width="2.4" stroke-linecap="round" fill="none"/>`;
  return (
    sheet(-spread, { small, accent: ORANGE }) +
    sheet(spread, { small, accent: ORANGE }) +
    sheet(0, { small, accent: ORANGE_BRIGHT }) +
    motion +
    printer()
  );
}

/** inset: how much of the canvas the tile leaves empty on each side (0–100 units). */
function icon({ inset, small }) {
  const size = 100 - inset * 2;
  const scale = size / 100;
  const radius = 22.5 * scale;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 100 100">
<defs><linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${TILE_TOP}"/><stop offset="1" stop-color="${TILE_BOTTOM}"/></linearGradient></defs>
<rect x="${inset}" y="${inset}" width="${size}" height="${size}" rx="${radius}" fill="url(#tile)"/>
<rect x="${inset + 0.3}" y="${inset + 0.3}" width="${size - 0.6}" height="${size - 0.6}" rx="${radius - 0.3}" fill="none" stroke="#000" stroke-opacity="0.08" stroke-width="0.6"/>
<g transform="translate(50 50) scale(${scale}) translate(-50 -50)">${mark({ small })}</g>
</svg>
`;
}

const VARIANTS = {
  // Apple's grid: the tile body is 824 of 1024 px.
  macos: icon({ inset: 9.8, small: false }),
  windows: icon({ inset: 0, small: false }),
  small: icon({ inset: 0, small: true }),
};

function tauriIcon(input, output) {
  execFileSync('pnpm', ['tauri', 'icon', input, '--output', output], {
    cwd: REPO,
    stdio: 'inherit',
  });
}

mkdirSync(SOURCES, { recursive: true });
for (const [name, svg] of Object.entries(VARIANTS)) {
  writeFileSync(join(SOURCES, `${name}.svg`), svg);
}

// 1. Everything from the macOS master (icns, PNGs, iOS, Android).
tauriIcon(join(SOURCES, 'macos.svg'), ICONS);

// 2. Windows assets drawn edge to edge.
const winOut = mkdtempSync(join(tmpdir(), 'mp-icons-win-'));
const smallOut = mkdtempSync(join(tmpdir(), 'mp-icons-small-'));
try {
  tauriIcon(join(SOURCES, 'windows.svg'), winOut);
  for (const f of [
    'icon.ico',
    'StoreLogo.png',
    'Square30x30Logo.png',
    'Square44x44Logo.png',
    'Square71x71Logo.png',
    'Square89x89Logo.png',
    'Square107x107Logo.png',
    'Square142x142Logo.png',
    'Square150x150Logo.png',
    'Square284x284Logo.png',
    'Square310x310Logo.png',
  ]) {
    copyFileSync(join(winOut, f), join(ICONS, f));
  }
  // 3. The sizes too small for the full drawing.
  tauriIcon(join(SOURCES, 'small.svg'), smallOut);
  for (const f of ['32x32.png', 'Square30x30Logo.png', 'Square44x44Logo.png']) {
    copyFileSync(join(smallOut, f), join(ICONS, f));
  }
} finally {
  rmSync(winOut, { recursive: true, force: true });
  rmSync(smallOut, { recursive: true, force: true });
}
console.log('icons regenerated in', ICONS);
