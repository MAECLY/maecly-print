import { useState, type CSSProperties, type ReactNode } from 'react';
import { Icon, type IconName } from '../Icon';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md' | 'lg';

// Optional props take explicit `| undefined` so callers can pass computed
// values like `disabled={maybe}` under `exactOptionalPropertyTypes`.
interface MPButtonProps {
  children?: ReactNode | undefined;
  variant?: ButtonVariant | undefined;
  size?: ButtonSize | undefined;
  icon?: IconName | undefined;
  iconRight?: IconName | undefined;
  onClick?: ((e: React.MouseEvent<HTMLButtonElement>) => void) | undefined;
  disabled?: boolean | undefined;
  fullWidth?: boolean | undefined;
  type?: 'button' | 'submit' | 'reset' | undefined;
  style?: CSSProperties | undefined;
}

const SIZES = {
  sm: { px: 8, py: 4, fs: 12, h: 24, gap: 5, rad: 5 },
  md: { px: 12, py: 6, fs: 13, h: 30, gap: 6, rad: 6 },
  lg: { px: 16, py: 9, fs: 14, h: 36, gap: 8, rad: 7 },
};

const VARIANTS: Record<ButtonVariant, { bg: string; fg: string; bd: string }> =
  {
    primary: { bg: 'var(--mp-accent)', fg: '#FFF', bd: 'var(--mp-accent)' },
    secondary: {
      bg: 'var(--mp-surface)',
      fg: 'var(--mp-text)',
      bd: 'var(--mp-border)',
    },
    ghost: { bg: 'transparent', fg: 'var(--mp-text)', bd: 'transparent' },
    danger: {
      bg: 'var(--mp-surface)',
      fg: 'var(--mp-error)',
      bd: 'var(--mp-border)',
    },
  };

export function MPButton({
  children,
  variant = 'secondary',
  size = 'md',
  icon,
  iconRight,
  onClick,
  disabled,
  fullWidth,
  type = 'button',
  style,
}: MPButtonProps) {
  const sizes = SIZES[size];
  const variants = VARIANTS[variant];
  const [hover, setHover] = useState(false);
  const hoverBg =
    variant === 'primary'
      ? 'var(--mp-accent)'
      : variant === 'ghost'
        ? 'var(--mp-surface3)'
        : 'var(--mp-surface2)';
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      className="mp-focus"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: sizes.gap,
        padding: `${sizes.py}px ${sizes.px}px`,
        height: sizes.h,
        fontSize: sizes.fs,
        fontWeight: 500,
        lineHeight: 1,
        background: hover && !disabled ? hoverBg : variants.bg,
        color: variants.fg,
        border: `1px solid ${variants.bd}`,
        borderRadius: sizes.rad,
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.5 : 1,
        width: fullWidth ? '100%' : 'auto',
        transition: 'background .12s, border-color .12s, transform .08s',
        boxShadow:
          variant === 'primary'
            ? '0 1px 0 rgba(0,0,0,0.04), inset 0 1px 0 rgba(255,255,255,0.12)'
            : 'none',
        whiteSpace: 'nowrap',
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={sizes.fs + 1} />}
      {children}
      {iconRight && <Icon name={iconRight} size={sizes.fs + 1} />}
    </button>
  );
}
