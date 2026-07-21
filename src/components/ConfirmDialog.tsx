import { useEffect, useId, useRef } from 'react';
import { MPButton } from '@/components/ui';

/** A pending confirmation surfaced by the in-app modal (never window.confirm). */
export interface ConfirmState {
  title: string;
  message: string;
  confirmLabel: string;
  run: () => void | Promise<void>;
}

/** Everything inside the dialog that can hold focus, in document order. */
function tabbables(root: HTMLElement): HTMLElement[] {
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
    ),
  ).filter((el) => !el.hasAttribute('disabled'));
}

/**
 * In-app confirmation dialog. `window.confirm` is a no-op in WKWebView (the
 * macOS Tauri webview), so we render a real modal. Shared across screens that
 * need a "are you sure?" step before a side-effecting action.
 *
 * Keyboard contract (NFR-040 / NFR-041): focus lands on Cancel — the least
 * destructive action —, Tab cycles inside the dialog, Escape closes it, and
 * the element that opened it gets the focus back on unmount.
 */
export function ConfirmDialog({
  state,
  onClose,
}: {
  state: ConfirmState;
  onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const messageId = useId();

  useEffect(() => {
    const opener = document.activeElement;
    tabbables(dialogRef.current ?? document.body)[0]?.focus();
    return () => {
      if (opener instanceof HTMLElement && document.contains(opener)) {
        opener.focus();
      }
    };
  }, []);

  // Escape and the Tab cycle live on the document so they work no matter
  // where focus sits while the modal is open.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
        return;
      }
      if (e.key !== 'Tab' || !dialogRef.current) return;
      const items = tabbables(dialogRef.current);
      const first = items[0];
      const last = items[items.length - 1];
      if (!first || !last) return;
      const active = document.activeElement;
      const inside = dialogRef.current.contains(active);
      if (e.shiftKey && (active === first || !inside)) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !inside)) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  return (
    <div
      onClick={onClose}
      style={{
        position: 'absolute',
        inset: 0,
        background: 'rgba(20,18,14,0.42)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 50,
        backdropFilter: 'blur(2px)',
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={messageId}
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 360,
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
            Cancel
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
    </div>
  );
}
