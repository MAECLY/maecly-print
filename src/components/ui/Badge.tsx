import type { ReactNode } from 'react';
import { Icon, type IconName } from '../Icon';

export type BadgeTone =
  | 'neutral'
  | 'success'
  | 'warn'
  | 'error'
  | 'info'
  | 'accent';

// Optional props take explicit `| undefined` so callers can pass computed
// values like `icon={maybe}` under `exactOptionalPropertyTypes`.
interface MPBadgeProps {
  children: ReactNode;
  tone?: BadgeTone | undefined;
  icon?: IconName | undefined;
}

const TONES: Record<BadgeTone, { bg: string; fg: string; bd: string }> = {
  neutral: {
    bg: 'var(--mp-surface2)',
    fg: 'var(--mp-textMuted)',
    bd: 'var(--mp-border)',
  },
  success: {
    bg: 'var(--mp-successSoft)',
    fg: 'var(--mp-success)',
    bd: 'transparent',
  },
  warn: {
    bg: 'var(--mp-warnSoft)',
    fg: 'var(--mp-warn)',
    bd: 'transparent',
  },
  error: {
    bg: 'var(--mp-errorSoft)',
    fg: 'var(--mp-error)',
    bd: 'transparent',
  },
  info: { bg: 'var(--mp-infoSoft)', fg: 'var(--mp-info)', bd: 'transparent' },
  accent: {
    bg: 'var(--mp-accentSoft)',
    fg: 'var(--mp-accent)',
    bd: 'transparent',
  },
};

export function MPBadge({ children, tone = 'neutral', icon }: MPBadgeProps) {
  const tones = TONES[tone];
  return (
    <span
      className="mp-mono"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        padding: '2px 6px',
        fontSize: 10.5,
        fontWeight: 500,
        background: tones.bg,
        color: tones.fg,
        border: `1px solid ${tones.bd}`,
        borderRadius: 4,
        textTransform: 'uppercase',
        letterSpacing: '0.04em',
      }}
    >
      {icon && <Icon name={icon} size={11} />}
      {children}
    </span>
  );
}
