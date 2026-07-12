import { FileGlyph, MPCard, StatusDot } from '@/components/ui';
import type { UiQueueJob } from '@/types/domain';

interface QueueBoardViewProps {
  jobs: UiQueueJob[];
}

export function QueueBoardView({ jobs }: QueueBoardViewProps) {
  const cols = [
    {
      key: 'queued',
      label: 'Queued',
      // `converted` (PDF ready, waiting for a printer) has no column of its own;
      // it belongs with the not-yet-printing work.
      items: jobs.filter((j) =>
        ['queued', 'paused', 'converted'].includes(j.status),
      ),
    },
    {
      key: 'converting',
      label: 'Converting',
      items: jobs.filter((j) => j.status === 'converting'),
    },
    {
      key: 'printing',
      label: 'Printing',
      items: jobs.filter((j) => j.status === 'printing'),
    },
    {
      key: 'done',
      label: 'Done',
      items: jobs.filter((j) =>
        ['done', 'failed', 'cancelled'].includes(j.status),
      ),
    },
  ];
  return (
    <div
      className="mp-scroll"
      style={{
        flex: 1,
        overflow: 'auto',
        padding: 14,
        display: 'grid',
        gridTemplateColumns: 'repeat(4,1fr)',
        gap: 12,
      }}
    >
      {cols.map((c) => (
        <div
          key={c.key}
          style={{ display: 'flex', flexDirection: 'column', gap: 8 }}
        >
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              padding: '2px 4px',
            }}
          >
            <span
              className="mp-mono"
              style={{
                fontSize: 10.5,
                color: 'var(--mp-textMuted)',
                textTransform: 'uppercase',
                letterSpacing: '0.08em',
              }}
            >
              {c.label}
            </span>
            <span
              className="mp-mono"
              style={{ fontSize: 10.5, color: 'var(--mp-textDim)' }}
            >
              {c.items.length}
            </span>
          </div>
          {c.items.map((j) => (
            <MPCard key={j.id} padding={10} hoverable>
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  marginBottom: 6,
                }}
              >
                <FileGlyph ext={j.ext} size={18} />
                <span
                  style={{
                    fontSize: 11.5,
                    fontWeight: 500,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                    flex: 1,
                    minWidth: 0,
                  }}
                >
                  {j.file}
                </span>
              </div>
              <div
                className="mp-mono"
                style={{
                  fontSize: 10,
                  color: 'var(--mp-textDim)',
                  marginBottom: 8,
                }}
              >
                {j.pages}p · {j.profile}
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <StatusDot status={j.status} />
                <div
                  style={{
                    flex: 1,
                    height: 3,
                    background: 'var(--mp-surface3)',
                    borderRadius: 2,
                    overflow: 'hidden',
                  }}
                >
                  <div
                    style={{
                      height: '100%',
                      width: `${(j.progress || 0) * 100}%`,
                      background: 'var(--mp-accent)',
                    }}
                  />
                </div>
              </div>
            </MPCard>
          ))}
        </div>
      ))}
    </div>
  );
}
