import type { ReactNode } from 'react';

interface MPSectionLabelProps {
  children: ReactNode;
  mono?: boolean;
}

export function MPSectionLabel({ children, mono = true }: MPSectionLabelProps) {
  return (
    <div
      className={mono ? 'mp-mono' : ''}
      style={{
        fontSize: 10.5,
        fontWeight: 500,
        letterSpacing: '0.08em',
        color: 'var(--mp-textDim)',
        textTransform: 'uppercase',
        marginBottom: 8,
      }}
    >
      {children}
    </div>
  );
}
