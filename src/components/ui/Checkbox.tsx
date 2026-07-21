import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FocusEvent,
  type ReactNode,
} from 'react';
import { Icon } from '../Icon';

interface MPCheckboxProps {
  checked: boolean;
  onChange?: (e: ChangeEvent<HTMLInputElement>) => void;
  label?: ReactNode;
  indeterminate?: boolean;
  /** Accessible name when the caption is rendered outside the control. */
  'aria-label'?: string;
  /** Id of the element naming the control, when the caption lives elsewhere. */
  'aria-labelledby'?: string;
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

export function MPCheckbox({
  checked,
  onChange,
  label,
  indeterminate,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
}: MPCheckboxProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [ring, setRing] = useState(false);

  // `indeterminate` is a DOM property, not an attribute: setting it is what
  // makes the control expose `aria-checked="mixed"` to assistive tech.
  useEffect(() => {
    if (inputRef.current) inputRef.current.indeterminate = !!indeterminate;
  }, [indeterminate]);

  const onFocus = (e: FocusEvent<HTMLInputElement>) => {
    setRing(isFocusVisible(e.currentTarget));
  };

  return (
    <label
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        cursor: 'pointer',
        fontSize: 13,
        color: 'var(--mp-text)',
        position: 'relative',
      }}
    >
      {/*
        Visually hidden but still focusable and exposed: `display: none` would
        drop the box out of the tab order and the accessibility tree.
      */}
      <input
        ref={inputRef}
        type="checkbox"
        checked={checked}
        onChange={onChange}
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
          outline: ring ? '2px solid var(--mp-focus)' : 'none',
          outlineOffset: 2,
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
      {label}
    </label>
  );
}
