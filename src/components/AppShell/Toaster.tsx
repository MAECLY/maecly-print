import { Toaster as SonnerToaster } from 'sonner';

/**
 * Themed wrapper around `sonner`.
 * Inherits the active theme via CSS variables; sonner accepts a theme prop
 * for its own internals.
 */
export function Toaster({ theme }: { theme: 'light' | 'dark' }) {
  return (
    <SonnerToaster
      theme={theme}
      position="bottom-right"
      richColors={false}
      closeButton
      toastOptions={{
        style: {
          fontFamily: 'var(--mp-font-ui)',
          fontSize: 13,
          background: 'var(--mp-surface)',
          color: 'var(--mp-text)',
          border: '1px solid var(--mp-border)',
          boxShadow: '0 8px 24px rgba(0,0,0,0.12), 0 0 0 1px var(--mp-border)',
        },
      }}
    />
  );
}
