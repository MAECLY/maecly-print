import { MPButton } from '@/components/ui';

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
 */
export function ConfirmDialog({
  state,
  onClose,
}: {
  state: ConfirmState;
  onClose: () => void;
}) {
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
        role="dialog"
        aria-modal="true"
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
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 6 }}>
          {state.title}
        </div>
        <div
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
