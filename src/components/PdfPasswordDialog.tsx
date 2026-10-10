import { useEffect, useId, useRef, useState, type FormEvent } from 'react';

import { ModalOverlay } from '@/components/Modal';
import { useModalFocus } from '@/components/useModal';
import { MPButton, MPCheckbox, MPInput } from '@/components/ui';
import { useT } from '@/i18n/I18nProvider';
import type { Job } from '@/ipc/bindings';
import { supplyPdfPassword } from '@/lib/ipc';
import { errorMessage } from '@/lib/ipcError';
import { needsPassword } from '@/lib/pdfPassword';
import { useQueueStore } from '@/stores/queueStore';

/** States in which the job has gone past the password (or past caring). */
const DONE_WITH_PASSWORD = new Set([
  'converted',
  'printing',
  'done',
  'failed',
  'cancelled',
]);

type Phase = 'idle' | 'sending' | 'checking';

interface PdfPasswordDialogProps {
  /** The paused job asking for a password. */
  jobId: string;
  /** Its file name, for the prompt. */
  file: string;
  onClose: () => void;
}

/**
 * The per-job password prompt for an encrypted PDF (FR-059, NFR-082).
 *
 * The password goes to `job_supply_password`, which keeps it in memory only,
 * for this job; the field is cleared as soon as it is sent and nothing is
 * stored on this side either. The dialog then follows the job: a wrong
 * password brings it back paused with `password_incorrect`, which re-arms the
 * field with an error so the user can try again; once the job converts (or
 * ends some other way) the dialog closes itself. "Not now" leaves the job
 * paused; "Cancel this job" cancels it.
 */
export function PdfPasswordDialog({
  jobId,
  file,
  onClose,
}: PdfPasswordDialogProps) {
  const t = useT('queue');
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputId = useId();
  const titleId = useId();
  const bodyId = useId();
  const errorId = useId();
  const cancelJob = useQueueStore((s) => s.cancelJob);

  const job = useQueueStore((s) =>
    s.snapshot?.jobs.find((j) => j.id === jobId),
  );
  const otherProtected = useQueueStore(
    (s) =>
      (s.snapshot?.jobs ?? []).filter(
        (j) =>
          j.id !== jobId &&
          j.batch_id === job?.batch_id &&
          needsPassword(j.state, j.error),
      ).length,
  );

  const [password, setPassword] = useState('');
  const [applyToBatch, setApplyToBatch] = useState(false);
  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState<string | null>(() =>
    job?.error?.reason === 'password_incorrect'
      ? t('password.incorrect')
      : null,
  );

  /**
   * What the dialog watches for after sending: `left` turns true once the job
   * is seen past the "needs a password" pause, so seeing that pause again
   * means the password was wrong.
   */
  const watch = useRef({ active: false, left: false });
  const close = useRef(onClose);
  close.current = onClose;

  useModalFocus(dialogRef, {
    onEscape: () => close.current(),
    initialFocusSelector: 'input',
  });

  // Follow the job through every snapshot the store adopts (a subscription,
  // not a render, so no intermediate state is missed).
  useEffect(() => {
    const onJob = (current: Job | undefined) => {
      const w = watch.current;
      if (!current || DONE_WITH_PASSWORD.has(current.state)) {
        w.active = false;
        close.current();
        return;
      }
      if (!w.active) return;
      if (needsPassword(current.state, current.error)) {
        if (!w.left) return;
        w.active = false;
        setPhase('idle');
        setError(t('password.incorrect'));
        dialogRef.current?.querySelector('input')?.focus();
        return;
      }
      w.left = true;
      if (current.state === 'paused') {
        // Paused for another reason (the queue's or the user's): the
        // password got through; the row says what is left to do.
        w.active = false;
        close.current();
      }
    };
    return useQueueStore.subscribe((s) =>
      onJob(s.snapshot?.jobs.find((j) => j.id === jobId)),
    );
  }, [jobId, t]);

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    if (phase !== 'idle') return;
    if (password.length === 0) {
      setError(t('password.empty'));
      return;
    }
    const secret = password;
    // Clear the field at once: the password lives on only in the call.
    setPassword('');
    setError(null);
    watch.current = { active: true, left: false };
    setPhase('sending');
    try {
      await supplyPdfPassword(
        jobId,
        secret,
        applyToBatch && otherProtected > 0,
      );
      setPhase((p) => (p === 'sending' ? 'checking' : p));
    } catch (err) {
      watch.current.active = false;
      setPhase('idle');
      setError(errorMessage(err));
    }
  };

  const busy = phase !== 'idle';

  return (
    <ModalOverlay
      onBackdropClick={() => {
        if (!busy) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        aria-busy={busy}
        style={{
          width: 400,
          maxWidth: 'calc(100vw - 32px)',
          background: 'var(--mp-surface)',
          border: '1px solid var(--mp-border)',
          borderRadius: 12,
          boxShadow: '0 24px 60px rgba(0,0,0,0.28)',
          padding: 18,
        }}
      >
        <form onSubmit={(e) => void submit(e)}>
          <div
            id={titleId}
            style={{ fontSize: 14, fontWeight: 600, marginBottom: 6 }}
          >
            {t('password.title')}
          </div>
          <div
            id={bodyId}
            style={{
              fontSize: 12.5,
              color: 'var(--mp-textMuted)',
              lineHeight: 1.5,
              marginBottom: 14,
              overflowWrap: 'anywhere',
            }}
          >
            {t('password.body', { file })}
          </div>
          <label
            htmlFor={inputId}
            style={{
              display: 'block',
              fontSize: 12,
              fontWeight: 500,
              marginBottom: 6,
            }}
          >
            {t('password.label')}
          </label>
          <MPInput
            id={inputId}
            type="password"
            autoComplete="off"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
              if (error) setError(null);
            }}
            disabled={busy}
            fullWidth
            error={error !== null}
            aria-describedby={error ? errorId : undefined}
          />
          <div
            id={errorId}
            role={error ? 'alert' : 'status'}
            aria-live="polite"
            style={{
              minHeight: 18,
              marginTop: 6,
              fontSize: 12,
              color: error ? 'var(--mp-error)' : 'var(--mp-textMuted)',
            }}
          >
            {error ?? (busy ? t('password.checking') : '')}
          </div>
          {otherProtected > 0 && (
            <div style={{ marginTop: 6 }}>
              <MPCheckbox
                checked={applyToBatch}
                onChange={(e) => setApplyToBatch(e.target.checked)}
                label={t('password.applyToBatch', { count: otherProtected })}
              />
            </div>
          )}
          <div
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              justifyContent: 'space-between',
              gap: 8,
              marginTop: 16,
            }}
          >
            <MPButton
              variant="ghost"
              size="sm"
              disabled={busy}
              onClick={() => {
                onClose();
                void cancelJob(jobId);
              }}
            >
              {t('password.cancelJob')}
            </MPButton>
            <div style={{ display: 'flex', gap: 8, marginLeft: 'auto' }}>
              <MPButton variant="ghost" size="sm" onClick={onClose}>
                {t('password.later')}
              </MPButton>
              <MPButton
                variant="primary"
                size="sm"
                type="submit"
                disabled={busy}
              >
                {t('password.submit')}
              </MPButton>
            </div>
          </div>
        </form>
      </div>
    </ModalOverlay>
  );
}
