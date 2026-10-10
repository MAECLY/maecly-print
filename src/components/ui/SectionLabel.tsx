import type { ReactNode } from 'react';

interface MPSectionLabelProps {
  children: ReactNode;
  mono?: boolean;
  /** Lets a control name itself with `aria-labelledby` (NFR-043). */
  id?: string | undefined;
}

export function MPSectionLabel({
  children,
  mono = true,
  id,
}: MPSectionLabelProps) {
  return (
    <div
      {...(id !== undefined ? { id } : {})}
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
