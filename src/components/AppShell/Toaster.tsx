import { Toaster as SonnerToaster } from 'sonner';

import { useT } from '@/i18n/I18nProvider';

/**
 * Themed wrapper around `sonner`.
 * Inherits the active theme via CSS variables; sonner accepts a theme prop
 * for its own internals.
 *
 * FR-092: sonner's own strings are English. The region name goes through
 * `containerAriaLabel` and the per-toast close button's label through
 * `toastOptions.closeButtonAriaLabel`.
 */
export function Toaster({ theme }: { theme: 'light' | 'dark' }) {
  const t = useT('common');
  return (
    <SonnerToaster
      theme={theme}
      position="bottom-right"
      // sonner 2 moved the default inset from 32px to 24px; keep the stack
      // where the design (and sonner 1) put it.
      offset={32}
      richColors={false}
      closeButton
      containerAriaLabel={t('toaster.region')}
      toastOptions={{
        closeButtonAriaLabel: t('toaster.close'),
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
