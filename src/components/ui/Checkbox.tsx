import type { ChangeEvent, ReactNode } from 'react';
import { Icon } from '../Icon';

interface MPCheckboxProps {
  checked: boolean;
  onChange?: (e: ChangeEvent<HTMLInputElement>) => void;
  label?: ReactNode;
  indeterminate?: boolean;
}

export function MPCheckbox({
  checked,
  onChange,
  label,
  indeterminate,
}: MPCheckboxProps) {
  return (
    <label
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        cursor: 'pointer',
        fontSize: 13,
        color: 'var(--mp-text)',
      }}
    >
      <span
        style={{
          width: 15,
          height: 15,
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          background:
            checked || indeterminate ? 'var(--mp-accent)' : 'var(--mp-surface)',
          border: `1px solid ${
            checked || indeterminate
              ? 'var(--mp-accent)'
              : 'var(--mp-borderStrong)'
          }`,
          borderRadius: 3.5,
          transition: 'all .12s',
        }}
      >
        {checked && <Icon name="check" size={11} color="#FFF" stroke={2.5} />}
        {indeterminate && !checked && (
          <span
            style={{
              width: 7,
              height: 1.5,
              background: '#FFF',
              borderRadius: 1,
            }}
          />
        )}
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={onChange}
        style={{ display: 'none' }}
      />
      {label}
    </label>
  );
}
