import { FileGlyph, MPIconButton, StatusDot } from '@/components/ui';
import type { JobState, UiQueueJob } from '@/types/domain';

interface QueueListViewProps {
  jobs: UiQueueJob[];
  setJobs: (jobs: UiQueueJob[]) => void;
}

function Row({
  j,
  onToggle,
}: {
  j: UiQueueJob;
  onToggle: (id: string, st: JobState) => void;
}) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '24px 1fr 110px 90px 110px 32px',
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
      <div style={{ display: 'flex' }}>
        {j.status === 'printing' || j.status === 'converting' ? (
          <MPIconButton
            icon="pause"
            size={24}
            label="Pause"
            onClick={() => onToggle(j.id, 'paused')}
          />
        ) : j.status === 'paused' || j.status === 'queued' ? (
          <MPIconButton
            icon="play"
            size={24}
            label="Resume"
            onClick={() => onToggle(j.id, 'printing')}
          />
        ) : j.status === 'failed' ? (
          <MPIconButton
            icon="rotate"
            size={24}
            label="Retry"
            onClick={() => onToggle(j.id, 'queued')}
          />
        ) : (
          <MPIconButton icon="more" size={24} label="More" />
        )}
      </div>
    </div>
  );
}

export function QueueListView({ jobs, setJobs }: QueueListViewProps) {
  const groups = {
    active: jobs.filter((j) =>
      ['printing', 'converting', 'converted'].includes(j.status),
    ),
    pending: jobs.filter((j) => ['queued', 'paused'].includes(j.status)),
    done: jobs.filter((j) =>
      ['done', 'failed', 'cancelled'].includes(j.status),
    ),
  };

  const toggleJob = (id: string, st: JobState) =>
    setJobs(jobs.map((j) => (j.id === id ? { ...j, status: st } : j)));

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
                <Row key={j.id} j={j} onToggle={toggleJob} />
              ))}
            </div>
          ),
      )}
    </div>
  );
}
