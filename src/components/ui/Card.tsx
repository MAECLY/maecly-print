import { useState, type CSSProperties, type ReactNode } from 'react';

interface MPCardProps {
  children: ReactNode;
  padding?: number | string;
  style?: CSSProperties;
  onClick?: (e: React.MouseEvent<HTMLDivElement>) => void;
  hoverable?: boolean;
}

export function MPCard({
  children,
  padding = 16,
  style,
  onClick,
  hoverable,
}: MPCardProps) {
  const [hover, setHover] = useState(false);
  return (
    <div
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        background: 'var(--mp-surface)',
        border: '1px solid var(--mp-border)',
        borderRadius: 8,
        padding,
        boxShadow: hover && hoverable ? '0 2px 6px rgba(0,0,0,0.04)' : 'none',
        cursor: onClick ? 'pointer' : 'default',
        transition: 'box-shadow .15s, border-color .12s',
        borderColor:
          hover && hoverable ? 'var(--mp-borderStrong)' : 'var(--mp-border)',
        ...style,
      }}
    >
      {children}
    </div>
  );
}
