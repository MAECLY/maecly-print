import { FileGlyph, MPIconButton, StatusDot } from '@/components/ui';
import { useQueueStore } from '@/stores/queueStore';
import type { UiQueueJob } from '@/types/domain';
import type { ConfirmState } from '@/components/ConfirmDialog';

interface QueueListViewProps {
  jobs: UiQueueJob[];
  /** Request an in-app confirmation (used for cancelling an in-flight job). */
  onConfirm: (state: ConfirmState) => void;
}

const TERMINAL = new Set(['done', 'failed', 'cancelled']);

interface RowProps {
  j: UiQueueJob;
  onPause: () => void;
  onResume: () => void;
  onRetry: () => void;
  onCancel: () => void;
}

function Row({ j, onPause, onResume, onRetry, onCancel }: RowProps) {
  const terminal = TERMINAL.has(j.status);
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
          style={{
            fontSize: 10.5,
            color: 'var(--mp-textDim)',
            marginTop: 1,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {j.profile} · {j.printer}
          {j.note ? ` · ${j.note}` : ''}
          {j.error ? ` · ${j.error}` : ''}
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
          {j.status}
        </span>
      </div>
      <div
        style={{
          height: 5,
          background: 'var(--mp-surface3)',
          borderRadius: 3,
          overflow: 'hidden',
          position: 'relative',
        }}
      >
        <div
          style={{
            position: 'absolute',
            inset: 0,
            width: `${(j.progress || 0) * 100}%`,
            background:
              j.status === 'failed'
                ? 'var(--mp-error)'
                : j.status === 'done'
                  ? 'var(--mp-success)'
                  : 'var(--mp-accent)',
            transition: 'width .3s',
          }}
        />
      </div>
      <div
        className="mp-mono"
        style={{
          fontSize: 10.5,
          color: 'var(--mp-textMuted)',
          textAlign: 'right',
        }}
      >
        {j.startedAt}
        {j.finishedAt ? ` → ${j.finishedAt}` : ''}
      </div>
      <div style={{ display: 'flex', gap: 2, justifyContent: 'flex-end' }}>
        {canPause && (
          <MPIconButton
            icon="pause"
            size={24}
            label="Pause"
            onClick={onPause}
          />
        )}
        {j.status === 'paused' && (
          <MPIconButton
            icon="play"
            size={24}
            label="Resume"
            onClick={onResume}
          />
        )}
        {j.status === 'failed' && (
          <MPIconButton
            icon="rotate"
            size={24}
            label="Retry"
            onClick={onRetry}
          />
        )}
        {!terminal && (
          <MPIconButton icon="x" size={24} label="Cancel" onClick={onCancel} />
        )}
      </div>
    </div>
  );
}

export function QueueListView({ jobs, onConfirm }: QueueListViewProps) {
  const pauseJob = useQueueStore((s) => s.pauseJob);
  const resumeJob = useQueueStore((s) => s.resumeJob);
  const retryJob = useQueueStore((s) => s.retryJob);
  const cancelJob = useQueueStore((s) => s.cancelJob);

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
  const requestCancel = (j: UiQueueJob) => {
    if (j.status === 'printing') {
      onConfirm({
        title: 'Cancel this job?',
        message: `“${j.file}” is printing. Cancel it? Pages already sent to the printer may still come out.`,
        confirmLabel: 'Cancel job',
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
          { key: 'active', label: 'Active', items: groups.active },
          { key: 'pending', label: 'Pending', items: groups.pending },
          { key: 'done', label: 'Recent', items: groups.done },
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
                  {g.label} · {g.items.length}
                </span>
              </div>
              {g.items.map((j) => (
                <Row
                  key={j.id}
                  j={j}
                  onPause={() => void pauseJob(j.id)}
                  onResume={() => void resumeJob(j.id)}
                  onRetry={() => void retryJob(j.id)}
                  onCancel={() => requestCancel(j)}
                />
              ))}
            </div>
          ),
      )}
    </div>
  );
}
