import type { ReactNode } from 'react';

interface MPTopBarProps {
  title: string;
  subtitle?: ReactNode;
  right?: ReactNode;
}

/**
 * Screen header: title + one-line subtitle on the left, actions on the right.
 *
 * At the narrowest window (880 px, and longer Spanish labels) the actions do
 * not fit beside the title: they wrap onto a second line, aligned right,
 * instead of running past the window edge, and the subtitle stays on one line
 * with an ellipsis instead of stacking word by word.
 */
export function MPTopBar({ title, subtitle, right }: MPTopBarProps) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        flexWrap: 'wrap',
        columnGap: 12,
        rowGap: 8,
        padding: '14px 20px',
        borderBottom: '1px solid var(--mp-border)',
        flexShrink: 0,
        background: 'var(--mp-bg)',
      }}
    >
      <div style={{ flex: '1 1 180px', minWidth: 0 }}>
        <div
          style={{
            fontSize: 15,
            fontWeight: 600,
            letterSpacing: '-0.01em',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
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
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {subtitle}
          </div>
        )}
      </div>
      {right && (
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            justifyContent: 'flex-end',
            gap: 6,
            alignItems: 'center',
            marginLeft: 'auto',
            maxWidth: '100%',
          }}
        >
          {right}
        </div>
      )}
    </div>
  );
}
