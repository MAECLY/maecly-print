import { useEffect, useRef } from 'react';
import { Toaster as SonnerToaster } from 'sonner';

import { useT } from '@/i18n/I18nProvider';

/**
 * Themed wrapper around `sonner`.
 * Inherits the active theme via CSS variables; sonner accepts a theme prop
 * for its own internals.
 *
 * FR-092: sonner's own strings are English. The region name goes through
 * `containerAriaLabel`; the per-toast close button has no prop that reaches it
 * in this sonner version, so its label is set on the rendered buttons.
 */
export function Toaster({ theme }: { theme: 'light' | 'dark' }) {
  const t = useT('common');
  const closeLabel = t('toaster.close');
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    const region = ref.current;
    if (!region) return;
    const relabel = () => {
      for (const b of region.querySelectorAll('button[data-close-button]')) {
        if (b.getAttribute('aria-label') !== closeLabel) {
          b.setAttribute('aria-label', closeLabel);
        }
      }
    };
    relabel();
    const observer = new MutationObserver(relabel);
    observer.observe(region, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['aria-label'],
    });
    return () => observer.disconnect();
  }, [closeLabel]);

  return (
    <SonnerToaster
      ref={ref}
      theme={theme}
      position="bottom-right"
      richColors={false}
      closeButton
      containerAriaLabel={t('toaster.region')}
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
