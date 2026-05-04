import { useState } from 'react';
import { Icon, type IconName } from '../Icon';

interface MPIconButtonProps {
  icon: IconName;
  onClick?: (e: React.MouseEvent<HTMLButtonElement>) => void;
  size?: number;
  label?: string;
  active?: boolean;
  disabled?: boolean;
}

export function MPIconButton({
  icon,
  onClick,
  size = 28,
  label,
  active,
  disabled,
}: MPIconButtonProps) {
  const [hover, setHover] = useState(false);
  return (
    <button
      onClick={onClick}
      aria-label={label}
      disabled={disabled}
      title={label}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      className="mp-focus"
      style={{
        width: size,
        height: size,
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: active
          ? 'var(--mp-surface3)'
          : hover && !disabled
            ? 'var(--mp-surface2)'
            : 'transparent',
        color: active ? 'var(--mp-text)' : 'var(--mp-textMuted)',
        border: 'none',
        borderRadius: 6,
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.4 : 1,
        transition: 'background .12s, color .12s',
      }}
    >
      <Icon name={icon} size={Math.round(size * 0.55)} />
    </button>
  );
}
