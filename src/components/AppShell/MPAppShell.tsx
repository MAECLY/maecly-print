import type { ReactNode } from 'react';

interface MPAppShellProps {
  children: ReactNode;
}

/**
 * Root application shell.
 *
 * The window uses native OS decorations (`decorations: true` in
 * tauri.conf.json), so the title bar and min/maximize/close controls are drawn
 * by macOS and Windows themselves — correct per platform, no custom chrome.
 * This component is just the full-height content surface below that native bar.
 * App branding lives in the sidebar (see `MPSidebar`).
 */
export function MPAppShell({ children }: MPAppShellProps) {
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
        overflow: 'hidden',
      }}
    >
      {children}
    </div>
  );
}
