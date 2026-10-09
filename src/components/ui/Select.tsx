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
  /** Accessible name: these dropdowns have no associated `<label>` element. */
  'aria-label'?: string;
  /** Id of the element naming the dropdown, when a caption is rendered. */
  'aria-labelledby'?: string;
  /** Id of the hint / error describing the dropdown. */
  'aria-describedby'?: string | undefined;
  id?: string | undefined;
  /** Native disabled: out of the tab order and announced as unavailable. */
  disabled?: boolean | undefined;
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
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  'aria-describedby': ariaDescribedBy,
  id,
  disabled,
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
        disabled={disabled}
        {...(id !== undefined ? { id } : {})}
        {...(ariaDescribedBy !== undefined
          ? { 'aria-describedby': ariaDescribedBy }
          : {})}
        {...(ariaLabel !== undefined ? { 'aria-label': ariaLabel } : {})}
        {...(ariaLabelledBy !== undefined
          ? { 'aria-labelledby': ariaLabelledBy }
          : {})}
        // `mp-focus` restores a visible keyboard focus ring; the custom
        // chevron requires `appearance: none`, which drops the native one.
        className="mp-focus"
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
          cursor: disabled ? 'default' : 'pointer',
          opacity: disabled ? 0.55 : 1,
          width: fullWidth ? '100%' : 'auto',
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
