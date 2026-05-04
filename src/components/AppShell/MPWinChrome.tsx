import type { ReactNode } from 'react';
import { MPGlyph } from '@/components/Brand';

interface MPWinChromeProps {
  children: ReactNode;
  os?: 'windows' | 'macos';
  title?: string;
}

export function MPWinChrome({
  children,
  os = 'windows',
  title = 'MaeclyPrint',
}: MPWinChromeProps) {
  return (
    <div
      className="mp-app"
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--mp-bg)',
        color: 'var(--mp-text)',
        borderRadius: os === 'macos' ? 10 : 0,
        overflow: 'hidden',
      }}
    >
      {/* Title bar */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          height: os === 'macos' ? 36 : 32,
          background: 'var(--mp-surface)',
          borderBottom: '1px solid var(--mp-border)',
          padding: os === 'macos' ? '0 12px' : '0 0 0 12px',
          flexShrink: 0,
          userSelect: 'none',
        }}
      >
        {os === 'macos' ? (
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <span
              style={{
                width: 12,
                height: 12,
                borderRadius: '50%',
                background: '#ED6A5E',
                border: '1px solid rgba(0,0,0,0.08)',
              }}
            />
            <span
              style={{
                width: 12,
                height: 12,
                borderRadius: '50%',
                background: '#F5BF4F',
                border: '1px solid rgba(0,0,0,0.08)',
              }}
            />
            <span
              style={{
                width: 12,
                height: 12,
                borderRadius: '50%',
                background: '#62C554',
                border: '1px solid rgba(0,0,0,0.08)',
              }}
            />
          </div>
        ) : (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <MPGlyph size={14} />
            <span
              style={{
                fontSize: 12,
                color: 'var(--mp-textMuted)',
                fontWeight: 500,
              }}
            >
              {title}
            </span>
          </div>
        )}
        <div
          style={{
            flex: 1,
            textAlign: 'center',
            fontSize: 12,
            color: 'var(--mp-textMuted)',
            fontWeight: 500,
          }}
        >
          {os === 'macos' ? title : ''}
        </div>
        {os === 'windows' ? (
          <div style={{ display: 'flex', height: '100%' }}>
            <span
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                width: 46,
                height: '100%',
                color: 'var(--mp-textMuted)',
              }}
            >
              <svg width="10" height="1" viewBox="0 0 10 1">
                <line x1="0" y1="0.5" x2="10" y2="0.5" stroke="currentColor" />
              </svg>
            </span>
            <span
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                width: 46,
                height: '100%',
                color: 'var(--mp-textMuted)',
              }}
            >
              <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
                <rect
                  x="0.5"
                  y="0.5"
                  width="9"
                  height="9"
                  stroke="currentColor"
                />
              </svg>
            </span>
            <span
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                width: 46,
                height: '100%',
                color: 'var(--mp-textMuted)',
              }}
            >
              <svg width="10" height="10" viewBox="0 0 10 10">
                <line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" />
                <line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" />
              </svg>
            </span>
          </div>
        ) : (
          <div style={{ width: 50 }} />
        )}
      </div>
      {children}
    </div>
  );
}
