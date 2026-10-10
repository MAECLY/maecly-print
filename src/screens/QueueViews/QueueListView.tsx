import { FileGlyph, MPIconButton, StatusDot } from '@/components/ui';
import { useT, type TFunction } from '@/i18n/I18nProvider';
import { useQueueStore } from '@/stores/queueStore';
import type { UiQueueJobView } from '@/lib/ipc';
import type { ConfirmState } from '@/components/ConfirmDialog';
import { needsPassword } from '@/lib/pdfPassword';

import { useJobLabels } from './useJobLabels';

interface QueueListViewProps {
  jobs: UiQueueJobView[];
  /** Request an in-app confirmation (used for cancelling an in-flight job). */
  onConfirm: (state: ConfirmState) => void;
  /** Ask for the password of a job's encrypted PDF (FR-059). */
  onEnterPassword: (job: UiQueueJobView) => void;
}

const TERMINAL = new Set(['done', 'failed', 'cancelled']);

/**
 * A job's progress bar, honest about what the backend actually knows.
 *
 * `indeterminate` jobs (the printing phase: no spooler reports a completion
 * fraction) get a sliding pulse and NO `aria-valuenow`, which is exactly how
 * ARIA spells "in progress, amount unknown". Painting them at 0 % — or making
 * a percentage up — would be a lie. Shared with the Board view, which renders
 * the same bar at a smaller size.
 */
export function JobProgressBar({
  job,
  height,
  t,
}: {
  job: UiQueueJobView;
  height: number;
  t: TFunction<'queue'>;
}) {
  const fill =
    job.status === 'failed'
      ? 'var(--mp-error)'
      : job.status === 'done'
        ? 'var(--mp-success)'
        : 'var(--mp-accent)';
  const label = t('progress.label', { file: job.file });
  return (
    <div
      role="progressbar"
      aria-label={label}
      {...(job.indeterminate
        ? { 'aria-valuetext': t('progress.indeterminate') }
        : {
            'aria-valuemin': 0,
            'aria-valuemax': 100,
            'aria-valuenow': Math.round(job.progress * 100),
          })}
      title={job.indeterminate ? t('progress.indeterminate') : undefined}
      style={{
        height,
        background: 'var(--mp-surface3)',
        borderRadius: 3,
        overflow: 'hidden',
        position: 'relative',
      }}
    >
      {job.indeterminate ? (
        <div
          style={{
            position: 'absolute',
            top: 0,
            bottom: 0,
            left: 0,
            width: '25%',
            background: fill,
            animation: 'mp-progress 1.4s ease-in-out infinite',
          }}
        />
      ) : (
        <div
          style={{
            position: 'absolute',
            inset: 0,
            width: `${job.progress * 100}%`,
            background: fill,
            transition: 'width .3s',
          }}
        />
      )}
    </div>
  );
}

interface RowProps {
  j: UiQueueJobView;
  t: TFunction<'queue'>;
  labels: ReturnType<typeof useJobLabels>;
  onPause: () => void;
  onResume: () => void;
  onRetry: () => void;
  onCancel: () => void;
  onEnterPassword: () => void;
  /** A command for this job is still in flight: no second click (NFR-033). */
  busy: boolean;
}

function Row({
  j,
  t,
  labels,
  onPause,
  onResume,
  onRetry,
  onCancel,
  onEnterPassword,
  busy,
}: RowProps) {
  const terminal = TERMINAL.has(j.status);
  const error = labels.error(j);
  const meta = [
    t('job.meta', { profile: j.profile, printer: labels.printer(j) }),
    j.noteKey ? t(`note.${j.noteKey}`) : null,
    error ?? null,
  ]
    .filter(Boolean)
    .join(' · ');
  const canPause =
    j.status === 'queued' ||
    j.status === 'converting' ||
    j.status === 'converted';
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '24px 1fr 110px 90px 110px 56px',
        gap: 10,
        alignItems: 'center',
        padding: '8px 14px',
        borderBottom: '1px solid var(--mp-border)',
      }}
    >
      <FileGlyph ext={j.ext} size={20} />
      <div style={{ minWidth: 0 }}>
        <div
          style={{
            fontSize: 12.5,
            fontWeight: 500,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {j.file}
        </div>
        <div
          className="mp-mono"
          title={meta}
          style={{
            fontSize: 10.5,
            color: 'var(--mp-textDim)',
            marginTop: 1,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {meta}
        </div>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <StatusDot status={j.status} />
        <span
          className="mp-mono"
          style={{
            fontSize: 10.5,
            color: 'var(--mp-textMuted)',
            textTransform: 'uppercase',
          }}
        >
          {/* Every `JobState` has a `status.<state>` entry, so the template
              literal resolves to a known key of the namespace. */}
          {t(`status.${j.status}`)}
        </span>
      </div>
      <JobProgressBar job={j} height={5} t={t} />
      <div
        className="mp-mono"
        style={{
          fontSize: 10.5,
          color: 'var(--mp-textMuted)',
          textAlign: 'right',
        }}
      >
        {j.finishedAt
          ? t('job.times', { from: j.startedAt, to: j.finishedAt })
          : j.startedAt}
      </div>
      <div style={{ display: 'flex', gap: 2, justifyContent: 'flex-end' }}>
        {canPause && (
          <MPIconButton
            icon="pause"
            size={24}
            label={t('rowActions.pause', { file: j.file })}
            disabled={busy}
            onClick={onPause}
          />
        )}
        {needsPassword(j.status, j.errorInfo) && (
          <MPIconButton
            icon="lock"
            size={24}
            label={t('password.rowAction', { file: j.file })}
            disabled={busy}
            onClick={onEnterPassword}
          />
        )}
        {j.status === 'paused' && !needsPassword(j.status, j.errorInfo) && (
          <MPIconButton
            icon="play"
            size={24}
            label={t('rowActions.resume', { file: j.file })}
            disabled={busy}
            onClick={onResume}
          />
        )}
        {j.status === 'failed' && (
          <MPIconButton
            icon="rotate"
            size={24}
            label={t('rowActions.retry', { file: j.file })}
            disabled={busy}
            onClick={onRetry}
          />
        )}
        {!terminal && (
          <MPIconButton
            icon="x"
            size={24}
            label={t('rowActions.cancel', { file: j.file })}
            disabled={busy}
            onClick={onCancel}
          />
        )}
      </div>
    </div>
  );
}

export function QueueListView({
  jobs,
  onConfirm,
  onEnterPassword,
}: QueueListViewProps) {
  const t = useT('queue');
  const pauseJob = useQueueStore((s) => s.pauseJob);
  const resumeJob = useQueueStore((s) => s.resumeJob);
  const retryJob = useQueueStore((s) => s.retryJob);
  const cancelJob = useQueueStore((s) => s.cancelJob);
  const pendingIds = useQueueStore((s) => s.pendingIds);
  const labels = useJobLabels(jobs);

  const groups = {
    active: jobs.filter((j) =>
      ['printing', 'converting', 'converted'].includes(j.status),
    ),
    pending: jobs.filter((j) => ['queued', 'paused'].includes(j.status)),
    done: jobs.filter((j) =>
      ['done', 'failed', 'cancelled'].includes(j.status),
    ),
  };

  // Cancelling a printing job interrupts a job the spooler already started, so
  // it gets a confirm; every other non-terminal state cancels immediately.
  const requestCancel = (j: UiQueueJobView) => {
    if (j.status === 'printing') {
      onConfirm({
        title: t('confirm.cancelTitle'),
        message: t('confirm.cancelMessage', { file: j.file }),
        confirmLabel: t('confirm.cancelConfirm'),
        run: () => cancelJob(j.id),
      });
    } else {
      void cancelJob(j.id);
    }
  };

  return (
    <div className="mp-scroll" style={{ flex: 1, overflow: 'auto' }}>
      {(
        [
          { key: 'active', label: t('groups.active'), items: groups.active },
          { key: 'pending', label: t('groups.pending'), items: groups.pending },
          { key: 'done', label: t('groups.done'), items: groups.done },
        ] as const
      ).map(
        (g) =>
          g.items.length > 0 && (
            <div key={g.key}>
              <div
                style={{
                  padding: '10px 14px',
                  background: 'var(--mp-surface2)',
                  borderBottom: '1px solid var(--mp-border)',
                  borderTop:
                    g.key !== 'active' ? '1px solid var(--mp-border)' : 'none',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                }}
              >
                <span
                  className="mp-mono"
                  style={{
                    fontSize: 10.5,
                    color: 'var(--mp-textMuted)',
                    textTransform: 'uppercase',
                    letterSpacing: '0.08em',
                    fontWeight: 500,
                  }}
                >
                  {t('groups.header', {
                    label: g.label,
                    count: g.items.length,
                  })}
                </span>
              </div>
              {g.items.map((j) => (
                <Row
                  key={j.id}
                  j={j}
                  t={t}
                  labels={labels}
                  onPause={() => void pauseJob(j.id)}
                  onResume={() => void resumeJob(j.id)}
                  onRetry={() => void retryJob(j.id)}
                  onCancel={() => requestCancel(j)}
                  onEnterPassword={() => onEnterPassword(j)}
                  busy={pendingIds.includes(j.id)}
                />
              ))}
            </div>
          ),
      )}
    </div>
  );
}
