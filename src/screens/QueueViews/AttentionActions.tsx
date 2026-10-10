import { useT } from '@/i18n/I18nProvider';
import type { UiQueueJobView } from '@/lib/ipc';
import { missingDependency } from '@/lib/deps';
import { needsPassword } from '@/lib/pdfPassword';

/** What the views call to act on a job that needs the user. */
export interface AttentionHandlers {
  /** Ask for the password of a job's encrypted PDF (FR-059). */
  onEnterPassword: (job: UiQueueJobView) => void;
  /** Offer to install what a paused job is missing, then resume it. */
  onInstallDependency: (job: UiQueueJobView) => void;
}

/**
 * The way out of a pause that needs the user, as labelled buttons — for the
 * Board and Timeline views, which have no row of icon actions. The List view
 * offers the same two actions as icon buttons. Renders nothing for any other
 * job.
 */
export function AttentionActions({
  job,
  onEnterPassword,
  onInstallDependency,
}: { job: UiQueueJobView } & AttentionHandlers) {
  const t = useT('queue');
  const locked = needsPassword(job.status, job.errorInfo);
  const dependency = missingDependency(job);
  if (!locked && !dependency) return null;
  const style = {
    fontSize: 11,
    padding: '2px 8px',
    borderRadius: 5,
    border: '1px solid var(--mp-border)',
    background: 'var(--mp-surface)',
    color: 'var(--mp-text)',
    cursor: 'pointer',
  } as const;
  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, marginTop: 6 }}>
      {locked && (
        <button
          type="button"
          className="mp-focus"
          style={style}
          aria-label={t('password.rowAction', { file: job.file })}
          onClick={() => onEnterPassword(job)}
        >
          {t('password.enter')}
        </button>
      )}
      {dependency && (
        <button
          type="button"
          className="mp-focus"
          style={style}
          aria-label={t('fix.installLabel', { file: job.file })}
          onClick={() => onInstallDependency(job)}
        >
          {t('fix.install')}
        </button>
      )}
    </div>
  );
}
