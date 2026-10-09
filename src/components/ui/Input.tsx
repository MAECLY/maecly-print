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
  /** Marks the value invalid: red border and `aria-invalid` (NFR-043). */
  error?: boolean | undefined;
  /** Native disabled: out of the tab order and announced as unavailable. */
  disabled?: boolean | undefined;
  id?: string | undefined;
  /**
   * Accessible name. Most captions in the app are rendered apart from the
   * control (a row label, a section heading), so callers pass the caption
   * here or point at it with `aria-labelledby` (NFR-043).
   */
  'aria-label'?: string | undefined;
  'aria-labelledby'?: string | undefined;
  /** Id of the inline error / hint that describes the field. */
  'aria-describedby'?: string | undefined;
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
  disabled,
  id,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  'aria-describedby': ariaDescribedBy,
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
              : 'var(--mp-borderStrong)'
        }`,
        borderRadius: 6,
        width: fullWidth ? '100%' : 'auto',
        transition: 'border-color .12s, box-shadow .12s',
        boxShadow: focus ? '0 0 0 3px var(--mp-accentSoft)' : 'none',
        opacity: disabled ? 0.55 : 1,
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
        disabled={disabled}
        {...(id !== undefined ? { id } : {})}
        {...(ariaLabel !== undefined ? { 'aria-label': ariaLabel } : {})}
        {...(ariaLabelledBy !== undefined
          ? { 'aria-labelledby': ariaLabelledBy }
          : {})}
        {...(ariaDescribedBy !== undefined
          ? { 'aria-describedby': ariaDescribedBy }
          : {})}
        {...(error ? { 'aria-invalid': true } : {})}
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
