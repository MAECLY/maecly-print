import type { ReactNode } from 'react';

interface MPKbdProps {
  children: ReactNode;
  /**
   * Spoken form of the key when the glyph alone does not read well (arrows,
   * symbols). Purely a text alternative — the badge is never interactive.
   */
  'aria-label'?: string;
}

/**
 * Key cap. A real `<kbd>` so screen readers announce it as keyboard input
 * rather than plain text, and never as a control the user could activate:
 * the shortcut itself is owned by whoever handles the key event.
 */
export function MPKbd({ children, 'aria-label': ariaLabel }: MPKbdProps) {
  return (
    <kbd
      className="mp-mono"
      {...(ariaLabel !== undefined ? { 'aria-label': ariaLabel } : {})}
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
    </kbd>
  );
}
