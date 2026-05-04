import type { ReactNode } from 'react';

interface MPKbdProps {
  children: ReactNode;
}

export function MPKbd({ children }: MPKbdProps) {
  return (
    <span
      className="mp-mono"
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        minWidth: 18,
        height: 18,
        padding: '0 5px',
        fontSize: 10.5,
        fontWeight: 500,
        background: 'var(--mp-surface2)',
        color: 'var(--mp-textMuted)',
        border: '1px solid var(--mp-border)',
        borderRadius: 4,
      }}
    >
      {children}
    </span>
  );
}
