/**
 * Frontend → log file bridge (NFR-033, FR-113).
 *
 * The error screens tell the user "the logs folder has the details", so the
 * details have to get there: every error the UI catches but cannot show — a
 * render crash, an unhandled rejection, a listener that failed to register —
 * goes through {@link logError}, which keeps the devtools `console.error` and
 * additionally forwards it to the Rust `log_frontend_error` command.
 *
 * Fire-and-forget by design: the caller is already on a failure path, so
 * logging must never throw, never await, and never produce a second error.
 */

import { commands } from '@/ipc/bindings';
import { isTauri } from '@/lib/ipc';

/** Message and stack of anything that was thrown. */
function describe(error: unknown): { message: string; stack: string | null } {
  if (error instanceof Error) {
    return { message: error.message || error.name, stack: error.stack ?? null };
  }
  if (typeof error === 'string') return { message: error, stack: null };
  try {
    return { message: JSON.stringify(error) ?? String(error), stack: null };
  } catch {
    return { message: String(error), stack: null };
  }
}

/**
 * Record an error the UI caught. `source` names where it was caught
 * (`'error-boundary'`, `'app.drag-drop-listener'`, …); `extraStack` carries a
 * second trace such as React's component stack.
 */
export function logError(
  source: string,
  error: unknown,
  extraStack?: string | null,
): void {
  console.error(source, error);
  if (!isTauri()) return;
  const { message, stack } = describe(error);
  const combined = [stack, extraStack].filter(Boolean).join('\n') || null;
  try {
    void commands
      .logFrontendError({ source, message, stack: combined })
      .catch(() => {});
  } catch {
    /* the bridge itself is gone; the console line above is all we can do */
  }
}

/**
 * Forward uncaught errors and unhandled promise rejections. Returns the
 * function that removes the listeners (tests; HMR).
 */
export function installGlobalErrorLogging(target: Window = window): () => void {
  const onError = (event: ErrorEvent) => {
    logError('window.error', event.error ?? event.message);
  };
  const onRejection = (event: PromiseRejectionEvent) => {
    logError('unhandledrejection', event.reason);
  };
  target.addEventListener('error', onError);
  target.addEventListener('unhandledrejection', onRejection);
  return () => {
    target.removeEventListener('error', onError);
    target.removeEventListener('unhandledrejection', onRejection);
  };
}
