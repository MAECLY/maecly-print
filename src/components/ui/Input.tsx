import {
  useState,
  type ChangeEvent,
  type KeyboardEvent,
  type ReactNode,
} from 'react';

export type InputSize = 'sm' | 'md' | 'lg';

interface MPInputProps {
  value?: string;
  onChange?: (e: ChangeEvent<HTMLInputElement>) => void;
  placeholder?: string;
  type?: string;
  size?: InputSize;
  mono?: boolean;
  fullWidth?: boolean;
  suffix?: ReactNode;
  prefix?: ReactNode;
  autoFocus?: boolean;
  onKeyDown?: (e: KeyboardEvent<HTMLInputElement>) => void;
  error?: boolean;
}

const SIZES = {
  sm: { h: 24, fs: 12, px: 8 },
  md: { h: 30, fs: 13, px: 10 },
  lg: { h: 36, fs: 14, px: 12 },
};

export function MPInput({
  value,
  onChange,
  placeholder,
  type = 'text',
  size = 'md',
  mono,
  fullWidth,
  suffix,
  prefix,
  autoFocus,
  onKeyDown,
  error,
}: MPInputProps) {
  const sizes = SIZES[size];
  const [focus, setFocus] = useState(false);
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        padding: `0 ${sizes.px}px`,
        height: sizes.h,
        background: 'var(--mp-surface)',
        border: `1px solid ${
          error
            ? 'var(--mp-error)'
            : focus
              ? 'var(--mp-focus)'
              : 'var(--mp-border)'
        }`,
        borderRadius: 6,
        width: fullWidth ? '100%' : 'auto',
        transition: 'border-color .12s, box-shadow .12s',
        boxShadow: focus ? '0 0 0 3px var(--mp-accentSoft)' : 'none',
      }}
    >
      {prefix && (
        <span style={{ color: 'var(--mp-textMuted)', display: 'flex' }}>
          {prefix}
        </span>
      )}
      <input
        value={value}
        onChange={onChange}
        placeholder={placeholder}
        type={type}
        autoFocus={autoFocus}
        onKeyDown={onKeyDown}
        onFocus={() => setFocus(true)}
        onBlur={() => setFocus(false)}
        className={mono ? 'mp-mono' : ''}
        style={{
          flex: 1,
          height: '100%',
          border: 'none',
          outline: 'none',
          background: 'transparent',
          fontSize: sizes.fs,
          color: 'var(--mp-text)',
          minWidth: 0,
        }}
      />
      {suffix && (
        <span style={{ color: 'var(--mp-textMuted)', fontSize: sizes.fs - 1 }}>
          {suffix}
        </span>
      )}
    </div>
  );
}
