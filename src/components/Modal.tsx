/**
 * Full-window modal backdrop. The focus contract and the open-modal stack it
 * relies on live in `useModal.ts`.
 */

import type { CSSProperties, ReactNode } from 'react';
import { createPortal } from 'react-dom';

// ---------------------------------------------------------------------------
// Overlay
// ---------------------------------------------------------------------------

interface ModalOverlayProps {
  children: ReactNode;
  /** Click on the backdrop (outside the dialog). */
  onBackdropClick?: () => void;
  /** Backdrop look; the default dims and blurs the app behind. */
  style?: CSSProperties;
}

/**
 * Full-window backdrop, portalled to `document.body` so it sits above the
 * whole app (and outside the `inert` root) whichever screen rendered it.
 */
export function ModalOverlay({
  children,
  onBackdropClick,
  style,
}: ModalOverlayProps) {
  if (typeof document === 'undefined') return null;
  return createPortal(
    // Backdrop click is a pointer convenience; Escape is its keyboard
    // equivalent (useModalFocus), so the div needs no key handler of its own.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
    <div
      className="mp-modal-root"
      onClick={(e) => {
        if (e.target === e.currentTarget) onBackdropClick?.();
      }}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(20,18,14,0.42)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
        backdropFilter: 'blur(2px)',
        ...style,
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
