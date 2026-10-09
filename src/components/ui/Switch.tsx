import {
  useState,
  type ChangeEvent,
  type FocusEvent,
  type ReactNode,
} from 'react';

interface MPSwitchProps {
  checked: boolean;
  onChange?: (e: ChangeEvent<HTMLInputElement>) => void;
  label?: ReactNode;
  /**
   * Accessible name for screens that render the caption outside the control
   * (Settings puts it in a separate column, so no visible `label` is passed).
   */
  'aria-label'?: string;
  /** Id of the element naming the control, when the caption lives elsewhere. */
  'aria-labelledby'?: string;
  /** Native disabled: out of the tab order, announced, ignores clicks. */
  disabled?: boolean | undefined;
}

/**
 * Whether a focus event should paint a ring: keyboard focus only, so a pointer
 * press on the label does not leave one behind. `:focus-visible` is not
 * supported everywhere; when the selector throws we keep the ring.
 */
function isFocusVisible(el: Element): boolean {
  try {
    return el.matches(':focus-visible');
  } catch {
    return true;
  }
}

export function MPSwitch({
  checked,
  onChange,
  label,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  disabled = false,
}: MPSwitchProps) {
  const [ring, setRing] = useState(false);
  const onFocus = (e: FocusEvent<HTMLInputElement>) => {
    setRing(isFocusVisible(e.currentTarget));
  };
  return (
    <label
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 10,
        cursor: disabled ? 'default' : 'pointer',
        fontSize: 13,
        position: 'relative',
        opacity: disabled ? 0.55 : 1,
      }}
    >
      {/*
        The input stays in the tab order and in the accessibility tree and is
        only pushed out of sight. `display: none` would make every toggle
        unreachable by keyboard and invisible to screen readers (NFR-041).
      */}
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        onChange={onChange}
        disabled={disabled}
        onFocus={onFocus}
        onBlur={() => setRing(false)}
        {...(ariaLabel !== undefined ? { 'aria-label': ariaLabel } : {})}
        {...(ariaLabelledBy !== undefined
          ? { 'aria-labelledby': ariaLabelledBy }
          : {})}
        style={{
          position: 'absolute',
          width: 1,
          height: 1,
          margin: 0,
          padding: 0,
          opacity: 0,
          pointerEvents: 'none',
        }}
      />
      <span
        aria-hidden="true"
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
          outline: ring ? '2px solid var(--mp-focus)' : 'none',
          outlineOffset: 2,
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
      {label && <span style={{ color: 'var(--mp-text)' }}>{label}</span>}
    </label>
  );
}
