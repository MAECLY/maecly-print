import { useId, useRef } from 'react';

import { ModalOverlay } from '@/components/Modal';
import { useModalFocus } from '@/components/useModal';
import { MPButton } from '@/components/ui';
import { useT } from '@/i18n/I18nProvider';

/** A pending confirmation surfaced by the in-app modal (never window.confirm). */
export interface ConfirmState {
  title: string;
  message: string;
  confirmLabel: string;
  run: () => void | Promise<void>;
}

/**
 * In-app confirmation dialog. `window.confirm` is a no-op in WKWebView (the
 * macOS Tauri webview), so we render a real modal. Shared across screens that
 * need a "are you sure?" step before a side-effecting action.
 *
 * Keyboard contract (NFR-040 / NFR-041): focus lands on Cancel — the least
 * destructive action —, Tab cycles inside the dialog, Escape closes it, and
 * the element that opened it gets the focus back on unmount. The backdrop is
 * portalled over the whole window and the app behind it is inert, whichever
 * screen opened it, and the app shortcuts stand down while it is open (see
 * `Modal.tsx`).
 */
export function ConfirmDialog({
  state,
  onClose,
}: {
  state: ConfirmState;
  onClose: () => void;
}) {
  const t = useT('common');
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const messageId = useId();

  useModalFocus(dialogRef, { onEscape: onClose });

  return (
    <ModalOverlay onBackdropClick={onClose}>
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={messageId}
        style={{
          width: 360,
          maxWidth: 'calc(100vw - 32px)',
          background: 'var(--mp-surface)',
          border: '1px solid var(--mp-border)',
          borderRadius: 12,
          boxShadow: '0 24px 60px rgba(0,0,0,0.28)',
          padding: 18,
        }}
      >
        <div
          id={titleId}
          style={{ fontSize: 14, fontWeight: 600, marginBottom: 6 }}
        >
          {state.title}
        </div>
        <div
          id={messageId}
          style={{
            fontSize: 12.5,
            color: 'var(--mp-textMuted)',
            lineHeight: 1.5,
            marginBottom: 16,
          }}
        >
          {state.message}
        </div>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <MPButton variant="ghost" size="sm" onClick={onClose}>
            {t('actions.cancel')}
          </MPButton>
          <MPButton
            variant="primary"
            size="sm"
            onClick={() => {
              onClose();
              void state.run();
            }}
          >
            {state.confirmLabel}
          </MPButton>
        </div>
      </div>
    </ModalOverlay>
  );
}
