import type { ChangeEvent } from 'react';
import { Icon } from '../Icon';

export type SelectSize = 'sm' | 'md' | 'lg';

export interface SelectOption {
  value: string;
  label: string;
}

interface MPSelectProps {
  value?: string;
  onChange?: (e: ChangeEvent<HTMLSelectElement>) => void;
  options: SelectOption[];
  size?: SelectSize;
  fullWidth?: boolean;
}

const SIZES = {
  sm: { h: 24, fs: 12, px: 8 },
  md: { h: 30, fs: 13, px: 10 },
  lg: { h: 36, fs: 14, px: 12 },
};

export function MPSelect({
  value,
  onChange,
  options,
  size = 'md',
  fullWidth,
}: MPSelectProps) {
  const sizes = SIZES[size];
  return (
    <div
      style={{
        position: 'relative',
        display: 'inline-block',
        width: fullWidth ? '100%' : 'auto',
      }}
    >
      <select
        value={value}
        onChange={onChange}
        style={{
          appearance: 'none',
          WebkitAppearance: 'none',
          height: sizes.h,
          padding: `0 ${sizes.px + 18}px 0 ${sizes.px}px`,
          fontSize: sizes.fs,
          color: 'var(--mp-text)',
          background: 'var(--mp-surface)',
          border: '1px solid var(--mp-border)',
          borderRadius: 6,
          cursor: 'pointer',
          width: fullWidth ? '100%' : 'auto',
          outline: 'none',
        }}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <span
        style={{
          position: 'absolute',
          right: 8,
          top: '50%',
          transform: 'translateY(-50%)',
          pointerEvents: 'none',
          display: 'flex',
          color: 'var(--mp-textMuted)',
        }}
      >
        <Icon name="chevd" size={13} />
      </span>
    </div>
  );
}
