import { useRef, type KeyboardEvent } from 'react';
import { Icon, type IconName } from '../Icon';

export type SegmentedSize = 'sm' | 'md' | 'lg';

export interface SegmentedOption<T extends string = string> {
  value: T;
  label: string;
  icon?: IconName;
}

interface MPSegmentedProps<T extends string = string> {
  value: T;
  onChange: (v: T) => void;
  options: SegmentedOption<T>[];
  size?: SegmentedSize;
  /** Accessible name of the group; screens render their caption separately. */
  'aria-label'?: string;
  /** Id of the element naming the group (e.g. the row caption). */
  'aria-labelledby'?: string;
}

/**
 * Single-choice segmented control. It follows the ARIA radio group pattern:
 * one tab stop for the whole group and arrow keys to move between options
 * (NFR-041), with `aria-checked` carrying the selection so it is not signalled
 * by colour alone (NFR-045).
 */
export function MPSegmented<T extends string = string>({
  value,
  onChange,
  options,
  size = 'md',
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
}: MPSegmentedProps<T>) {
  const h = size === 'sm' ? 24 : size === 'lg' ? 36 : 28;
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const selected = options.findIndex((o) => o.value === value);
  // With no match (value outside `options`) the first option holds the tab
  // stop, so the group is never unreachable.
  const tabStop = selected === -1 ? 0 : selected;

  const move = (from: number, delta: number) => {
    const count = options.length;
    if (count === 0) return;
    const next = (from + delta + count) % count;
    const option = options[next];
    if (!option) return;
    onChange(option.value);
    refs.current[next]?.focus();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    switch (e.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        e.preventDefault();
        move(index, 1);
        break;
      case 'ArrowLeft':
      case 'ArrowUp':
        e.preventDefault();
        move(index, -1);
        break;
      case 'Home':
        e.preventDefault();
        move(-1, 1);
        break;
      case 'End':
        e.preventDefault();
        move(0, -1);
        break;
      default:
        break;
    }
  };

  return (
    <div
      role="radiogroup"
      {...(ariaLabel !== undefined ? { 'aria-label': ariaLabel } : {})}
      {...(ariaLabelledBy !== undefined
        ? { 'aria-labelledby': ariaLabelledBy }
        : {})}
      style={{
        display: 'inline-flex',
        padding: 2,
        height: h + 4,
        background: 'var(--mp-surface2)',
        border: '1px solid var(--mp-border)',
        borderRadius: 7,
      }}
    >
      {options.map((o, i) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={i === tabStop ? 0 : -1}
            ref={(el) => {
              refs.current[i] = el;
            }}
            onClick={() => onChange(o.value)}
            onKeyDown={(e) => onKeyDown(e, i)}
            className="mp-focus"
            style={{
              height: h,
              padding: `0 ${size === 'sm' ? 8 : 12}px`,
              fontSize: size === 'sm' ? 11.5 : 12.5,
              fontWeight: active ? 500 : 400,
              background: active ? 'var(--mp-surface)' : 'transparent',
              color: active ? 'var(--mp-text)' : 'var(--mp-textMuted)',
              border: 'none',
              borderRadius: 5,
              cursor: 'pointer',
              boxShadow: active
                ? '0 1px 2px rgba(0,0,0,0.05), 0 0 0 1px var(--mp-border)'
                : 'none',
              display: 'inline-flex',
              alignItems: 'center',
              gap: 5,
              transition: 'background .12s, color .12s',
              whiteSpace: 'nowrap',
            }}
          >
            {o.icon && <Icon name={o.icon} size={13} />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
