/**
 * Shared modal plumbing (NFR-040 / NFR-041 / NFR-044).
 *
 * Every modal surface in the app — the confirmation dialog, the add-printer
 * dialog, the first-run tour — goes through these three pieces so they behave
 * identically:
 *
 * - {@link ModalOverlay} portals the backdrop to `document.body` with
 *   `position: fixed`, so it covers the whole window (top bar and sidebar
 *   included) no matter which screen opened it, and marks the app root `inert`
 *   while it is open so nothing behind it can be clicked, focused or read.
 * - {@link useModalFocus} moves focus in on open, keeps Tab / Shift+Tab inside,
 *   maps Escape to the caller's close action and hands focus back to the opener
 *   on close. Only the topmost modal reacts, so one Escape closes one dialog.
 * - {@link useAnyModalOpen} tells the app-wide shortcut layer to stand down
 *   while any modal owns the keyboard (NFR-034): a Ctrl/⌘+P printing behind a
 *   "Discard changes?" prompt, or a Ctrl/⌘+N navigating away from it, would be
 *   a genuine surprise.
 */

import {
  useEffect,
  useState,
  useSyncExternalStore,
  type RefObject,
} from 'react';
import { useLatest } from '@/lib/useLatest';

// ---------------------------------------------------------------------------
// Open-modal stack
// ---------------------------------------------------------------------------

const stack: symbol[] = [];
const listeners = new Set<() => void>();

/** The element that holds the app (everything a modal must make inert). */
function appRoot(): HTMLElement | null {
  return typeof document === 'undefined'
    ? null
    : document.getElementById('root');
}

function notify() {
  const root = appRoot();
  if (root) {
    if (stack.length > 0) root.setAttribute('inert', '');
    else root.removeAttribute('inert');
  }
  for (const l of listeners) l();
}

function push(id: symbol) {
  stack.push(id);
  notify();
}

function remove(id: symbol) {
  const i = stack.indexOf(id);
  if (i >= 0) stack.splice(i, 1);
  notify();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

const anyOpen = () => stack.length > 0;

/** True while at least one modal is mounted anywhere in the app. */
export function useAnyModalOpen(): boolean {
  return useSyncExternalStore(subscribe, anyOpen, anyOpen);
}

// ---------------------------------------------------------------------------
// Focus management
// ---------------------------------------------------------------------------

const TABBABLE =
  'button, [href], input, select, textarea, [contenteditable="true"], [tabindex]:not([tabindex="-1"])';

/** Everything inside `root` that can hold focus, in document order. */
export function tabbables(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(TABBABLE)).filter(
    (el) =>
      !el.hasAttribute('disabled') &&
      el.getAttribute('aria-hidden') !== 'true' &&
      !el.closest('[inert]'),
  );
}

export interface ModalFocusOptions {
  /** Escape closes the modal (or skips, for the tour). */
  onEscape?: () => void;
  /** What gets focus on open; defaults to the first tabbable element. */
  initialFocus?: RefObject<HTMLElement | null>;
  /** Alternatively, a selector resolved inside the dialog on open. */
  initialFocusSelector?: string;
}

/**
 * Register a modal for as long as the calling component is mounted and give it
 * the keyboard contract described in the module comment.
 */
export function useModalFocus(
  dialogRef: RefObject<HTMLElement | null>,
  { onEscape, initialFocus, initialFocusSelector }: ModalFocusOptions = {},
): void {
  const [id] = useState(() => Symbol('modal'));
  const escape = useLatest(onEscape);

  useEffect(() => {
    const opener = document.activeElement;
    push(id);
    const dialog = dialogRef.current;
    const target =
      initialFocus?.current ??
      (initialFocusSelector
        ? dialog?.querySelector<HTMLElement>(initialFocusSelector)
        : null) ??
      (dialog ? tabbables(dialog)[0] : undefined);
    if (target) target.focus();
    else if (dialog) {
      // Nothing tabbable yet: focus the dialog itself so the screen reader
      // still lands inside it.
      if (!dialog.hasAttribute('tabindex'))
        dialog.setAttribute('tabindex', '-1');
      dialog.focus();
    }
    return () => {
      remove(id);
      if (opener instanceof HTMLElement && document.contains(opener)) {
        opener.focus();
      }
    };
    // Mount/unmount only: the opener is whatever had focus when we appeared.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Escape and the Tab cycle live on the document so they work no matter where
  // focus sits; only the topmost modal answers.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (stack[stack.length - 1] !== id) return;
      if (e.key === 'Escape') {
        if (!escape.current) return;
        e.preventDefault();
        e.stopPropagation();
        escape.current();
        return;
      }
      const dialog = dialogRef.current;
      if (e.key !== 'Tab' || !dialog) return;
      const items = tabbables(dialog);
      const first = items[0];
      const last = items[items.length - 1];
      if (!first || !last) {
        e.preventDefault();
        return;
      }
      const active = document.activeElement;
      const inside = dialog.contains(active);
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
  }, [id, dialogRef, escape]);
}
