import type { ChangeEvent, ReactNode } from 'react';

interface MPSwitchProps {
  checked: boolean;
  onChange?: (e: ChangeEvent<HTMLInputElement>) => void;
  label?: ReactNode;
}

export function MPSwitch({ checked, onChange, label }: MPSwitchProps) {
  return (
    <label
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 10,
        cursor: 'pointer',
        fontSize: 13,
      }}
    >
      <span
        style={{
          width: 30,
          height: 17,
          padding: 2,
          position: 'relative',
          background: checked ? 'var(--mp-accent)' : 'var(--mp-surface3)',
          border: `1px solid ${
            checked ? 'var(--mp-accent)' : 'var(--mp-borderStrong)'
          }`,
          borderRadius: 99,
          transition: 'background .15s',
          display: 'inline-block',
        }}
      >
        <span
          style={{
            position: 'absolute',
            top: 1.5,
            left: checked ? 14 : 1.5,
            width: 13,
            height: 13,
            background: '#FFF',
            borderRadius: '50%',
            transition: 'left .15s',
            boxShadow: '0 1px 2px rgba(0,0,0,0.2)',
          }}
        />
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={onChange}
        style={{ display: 'none' }}
      />
      {label && <span style={{ color: 'var(--mp-text)' }}>{label}</span>}
    </label>
  );
}
