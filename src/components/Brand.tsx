/**
 * Brand identity components.
 * The mark follows the app icon drawn by `scripts/icons/generate.mjs`.
 */

import { useId } from 'react';

interface MPGlyphProps {
  size?: number;
  color?: string;
  bg?: string;
}

/**
 * The "Abanico" mark: a printer releasing three sheets in a fan. Same geometry
 * as the app icon (`scripts/icons/generate.mjs`, the small variant), so the
 * mark in the window and the one in the Dock cannot drift apart.
 */
function FanMark({
  ink,
  paper,
  accent,
  accentBright,
  detail,
}: {
  ink: string;
  paper: string;
  accent: string;
  accentBright: string;
  detail: boolean;
}) {
  const w = detail ? 24 : 26;
  const h = detail ? 32 : 30;
  const sheet = (angle: number, lines: string) => (
    <g transform={`translate(50 54) rotate(${angle})`}>
      <rect
        x={-w / 2}
        y={-h}
        width={w}
        height={h}
        rx="2.4"
        fill={paper}
        stroke={ink}
        strokeWidth={detail ? 2 : 4}
      />
      {detail &&
        [w - 8, w - 8, w - 14].map((lw, i) => (
          <rect
            key={i}
            x={-w / 2 + 4}
            y={-h + 6 + i * 6}
            width={lw}
            height="2.6"
            rx="1.3"
            fill={lines}
          />
        ))}
    </g>
  );
  const spread = detail ? 34 : 36;
  return (
    <>
      {sheet(-spread, accent)}
      {sheet(spread, accent)}
      {sheet(0, accentBright)}
      {detail && (
        <path
          d="M17 24 l-5 -4 M22 15 l-3 -6 M83 24 l5 -4 M78 15 l3 -6 M50 9 v-4"
          stroke={accent}
          strokeWidth="2.4"
          strokeLinecap="round"
          fill="none"
        />
      )}
      <rect
        x="26"
        y="47"
        width="48"
        height="12"
        rx="4"
        fill={ink}
        opacity="0.8"
      />
      <rect x="18" y="54" width="64" height="28" rx="7" fill={ink} />
      <circle cx="70" cy="68.5" r={detail ? 3 : 5} fill={accentBright} />
    </>
  );
}

function MPGlyph({ size = 24, color, bg }: MPGlyphProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="8 2 84 84"
      fill="none"
      aria-hidden="true"
    >
      <FanMark
        ink={color || 'currentColor'}
        paper={bg || 'var(--mp-bg)'}
        accent="var(--mp-accent)"
        accentBright="var(--mp-accent)"
        detail={size >= 40}
      />
    </svg>
  );
}

interface MPWordmarkProps {
  size?: number;
  showGlyph?: boolean;
}

export function MPWordmark({ size = 16, showGlyph = true }: MPWordmarkProps) {
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: size * 0.4,
      }}
    >
      {showGlyph && <MPGlyph size={size * 1.3} />}
      <span
        style={{
          fontFamily: 'var(--mp-font-ui)',
          fontSize: size,
          fontWeight: 600,
          letterSpacing: '-0.02em',
          color: 'var(--mp-text)',
        }}
      >
        Maecly<span style={{ color: 'var(--mp-accent)' }}>Print</span>
      </span>
    </span>
  );
}

interface MPAppIconProps {
  size?: number;
  rounded?: boolean;
}

export function MPAppIcon({ size = 64, rounded = true }: MPAppIconProps) {
  const tileId = useId();
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      aria-hidden="true"
      style={{
        display: 'block',
        borderRadius: rounded ? size * 0.225 : 0,
        boxShadow: `0 ${size * 0.04}px ${size * 0.12}px rgba(0,0,0,0.18)`,
      }}
    >
      <defs>
        <linearGradient id={tileId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FFF7ED" />
          <stop offset="1" stopColor="#FED7AA" />
        </linearGradient>
      </defs>
      <rect width="100" height="100" fill={`url(#${tileId})`} />
      <FanMark
        ink="#1C1917"
        paper="#FFFFFF"
        accent="#C2410C"
        accentBright="#F97316"
        detail={size >= 48}
      />
    </svg>
  );
}
