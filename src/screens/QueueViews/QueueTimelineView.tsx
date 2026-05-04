import { FileGlyph, MPBadge, StatusDot, type BadgeTone } from '@/components/ui';
import type { UiQueueJob } from '@/types/domain';

interface QueueTimelineViewProps {
  jobs: UiQueueJob[];
}

function tone(status: UiQueueJob['status']): BadgeTone {
  if (status === 'done') return 'success';
  if (status === 'failed') return 'error';
  if (status === 'printing') return 'accent';
  return 'neutral';
}

export function QueueTimelineView({ jobs }: QueueTimelineViewProps) {
  return (
    <div
      className="mp-scroll"
      style={{ flex: 1, overflow: 'auto', padding: '14px 20px' }}
    >
      {jobs.map((j, i) => (
        <div
          key={j.id}
          style={{
            display: 'flex',
            gap: 12,
            paddingBottom: 14,
            position: 'relative',
          }}
        >
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              flexShrink: 0,
              paddingTop: 4,
            }}
          >
            <StatusDot status={j.status} />
            {i < jobs.length - 1 && (
              <div
                style={{
                  flex: 1,
                  width: 1,
                  background: 'var(--mp-border)',
                  marginTop: 4,
                }}
              />
            )}
          </div>
          <div style={{ flex: 1, minWidth: 0, paddingBottom: 6 }}>
            <div
              className="mp-mono"
              style={{
                fontSize: 10.5,
                color: 'var(--mp-textDim)',
                marginBottom: 2,
              }}
            >
              {j.startedAt}
              {j.finishedAt ? ` → ${j.finishedAt}` : ''}
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <FileGlyph ext={j.ext} size={18} />
              <span style={{ fontSize: 12.5, fontWeight: 500 }}>{j.file}</span>
              <MPBadge tone={tone(j.status)}>{j.status}</MPBadge>
            </div>
            <div
              className="mp-mono"
              style={{
                fontSize: 10.5,
                color: 'var(--mp-textMuted)',
                marginTop: 4,
              }}
            >
              {j.printer} · {j.profile} · {j.pages}p
              {j.error ? ` · ${j.error}` : ''}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}
