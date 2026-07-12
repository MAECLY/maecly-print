/**
 * Brand identity components.
 * Ported 1:1 from `claude design/maeclyprint/project/brand.jsx:104-160`.
 */

interface MPGlyphProps {
  size?: number;
  color?: string;
  bg?: string;
}

function MPGlyph({ size = 24, color, bg }: MPGlyphProps) {
  const c = color || 'currentColor';
  const b = bg || 'var(--mp-bg)';
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
    >
      <rect
        x="2.5"
        y="3.5"
        width="13"
        height="17"
        rx="1.5"
        fill={b}
        stroke={c}
        strokeWidth="1.5"
      />
      <rect
        x="8.5"
        y="3.5"
        width="13"
        height="17"
        rx="1.5"
        fill={b}
        stroke={c}
        strokeWidth="1.5"
      />
      <path
        d="M11 8 L15 13 L19 8"
        stroke="var(--mp-accent)"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
      <line
        x1="11"
        y1="16"
        x2="19"
        y2="16"
        stroke={c}
        strokeWidth="1.2"
        strokeLinecap="round"
        opacity="0.5"
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
  return (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: rounded ? size * 0.22 : 0,
        background: 'linear-gradient(155deg, #FAF9F6 0%, #ECE7DC 100%)',
        boxShadow: `inset 0 0 0 1px rgba(0,0,0,0.08), 0 ${size * 0.04}px ${size * 0.12}px rgba(0,0,0,0.18)`,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        position: 'relative',
        overflow: 'hidden',
      }}
    >
      <svg
        width={size * 0.62}
        height={size * 0.62}
        viewBox="0 0 24 24"
        fill="none"
      >
        <rect
          x="2.5"
          y="3.5"
          width="13"
          height="17"
          rx="1.5"
          fill="#FFFFFF"
          stroke="#1A1814"
          strokeWidth="1.4"
        />
        <rect
          x="8.5"
          y="3.5"
          width="13"
          height="17"
          rx="1.5"
          fill="#FFFFFF"
          stroke="#1A1814"
          strokeWidth="1.4"
        />
        <path
          d="M11 8 L15 13 L19 8"
          stroke="#C2410C"
          strokeWidth="1.6"
          strokeLinecap="round"
          strokeLinejoin="round"
          fill="none"
        />
        <line
          x1="11"
          y1="16"
          x2="19"
          y2="16"
          stroke="#1A1814"
          strokeWidth="1.2"
          strokeLinecap="round"
          opacity="0.5"
        />
      </svg>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          pointerEvents: 'none',
          background:
            'radial-gradient(120% 80% at 30% 0%, rgba(255,255,255,0.7), transparent 60%)',
        }}
      />
    </div>
  );
}
