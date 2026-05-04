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
}

export function MPSegmented<T extends string = string>({
  value,
  onChange,
  options,
  size = 'md',
}: MPSegmentedProps<T>) {
  const h = size === 'sm' ? 24 : size === 'lg' ? 36 : 28;
  return (
    <div
      style={{
        display: 'inline-flex',
        padding: 2,
        height: h + 4,
        background: 'var(--mp-surface2)',
        border: '1px solid var(--mp-border)',
        borderRadius: 7,
      }}
    >
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            onClick={() => onChange(o.value)}
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
