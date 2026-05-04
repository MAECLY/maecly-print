import type { ReactNode } from 'react';

interface MPTopBarProps {
  title: string;
  subtitle?: ReactNode;
  right?: ReactNode;
}

export function MPTopBar({ title, subtitle, right }: MPTopBarProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '14px 20px',
        borderBottom: '1px solid var(--mp-border)',
        flexShrink: 0,
        background: 'var(--mp-bg)',
      }}
    >
      <div>
        <div
          style={{
            fontSize: 15,
            fontWeight: 600,
            letterSpacing: '-0.01em',
          }}
        >
          {title}
        </div>
        {subtitle && (
          <div
            style={{
              fontSize: 12,
              color: 'var(--mp-textMuted)',
              marginTop: 2,
            }}
          >
            {subtitle}
          </div>
        )}
      </div>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        {right}
      </div>
    </div>
  );
}
