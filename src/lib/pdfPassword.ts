import type { JobError } from '@/ipc/bindings';

/**
 * FR-059: whether a job in `state` with `error` is paused waiting for the
 * password of its encrypted PDF (none given yet, or a wrong one): the state
 * the queue offers "Enter password" for. Mirrors `Job::needs_password` in
 * Rust.
 */
export function needsPassword(
  state: string,
  error: JobError | null | undefined,
): boolean {
  return (
    state === 'paused' &&
    error?.code === 'pdf_protected' &&
    (error.reason === 'password_required' ||
      error.reason === 'password_incorrect')
  );
}
