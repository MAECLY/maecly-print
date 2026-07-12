import { Icon } from '@/components/Icon';
import {
  MPButton,
  MPCard,
  MPKbd,
  MPSectionLabel,
  FileGlyph,
  StatusDot,
  MPTopBar,
} from '@/components/ui';
import { useQueueStore } from '@/stores/queueStore';

interface HomeScreenProps {
  onAction?: (action: 'newjob' | 'queue') => void;
  onSelectJob?: (id: string) => void;
}

export function HomeScreen({ onAction, onSelectJob }: HomeScreenProps) {
  const recent = useQueueStore((s) => s.recentDone);
  return (
    <>
      <MPTopBar
        title="Home"
        subtitle="Right-click files in Explorer or Finder, drag them here, or press Ctrl+N to start a job."
        right={
          <MPButton
            variant="primary"
            icon="plus"
            onClick={() => onAction?.('newjob')}
          >
            New print job
          </MPButton>
        }
      />
      <div
        className="mp-scroll"
        style={{ flex: 1, overflow: 'auto', padding: 20 }}
      >
        <div
          style={{
            border: '1.5px dashed var(--mp-borderStrong)',
            borderRadius: 10,
            padding: '28px 20px',
            textAlign: 'center',
            background: 'var(--mp-surface)',
            marginBottom: 20,
          }}
        >
          <div
            style={{
              display: 'inline-flex',
              padding: 10,
              background: 'var(--mp-surface2)',
              borderRadius: 10,
              marginBottom: 10,
            }}
          >
            <Icon name="upload" size={20} color="var(--mp-textMuted)" />
          </div>
          <div
            style={{
              fontSize: 14,
              fontWeight: 500,
              marginBottom: 4,
            }}
          >
            Drop files or folders to print
          </div>
          <div style={{ fontSize: 12, color: 'var(--mp-textMuted)' }}>
            PDF, Office, ODF, images, HTML, Markdown · no count limit
          </div>
        </div>

        <MPSectionLabel>Quick actions</MPSectionLabel>
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: '1fr 1fr 1fr',
            gap: 10,
            marginBottom: 24,
          }}
        >
          {[
            {
              ico: 'plus' as const,
              t: 'New print job',
              d: 'Pick files and a profile',
              k: 'Ctrl+N',
            },
            {
              ico: 'folder' as const,
              t: 'Print a folder',
              d: 'Recurse or top-level only',
              k: 'Ctrl+O',
            },
            {
              ico: 'cmd' as const,
              t: 'Run from CLI',
              d: 'maeclyprint print --files',
              k: 'Ctrl+K',
            },
          ].map((q, i) => (
            <MPCard
              key={i}
              hoverable
              onClick={() => onAction?.('newjob')}
              padding={14}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  marginBottom: 8,
                }}
              >
                <Icon name={q.ico} size={16} color="var(--mp-accent)" />
                <MPKbd>{q.k}</MPKbd>
              </div>
              <div style={{ fontSize: 13, fontWeight: 500 }}>{q.t}</div>
              <div
                style={{
                  fontSize: 11.5,
                  color: 'var(--mp-textMuted)',
                  marginTop: 2,
                }}
              >
                {q.d}
              </div>
            </MPCard>
          ))}
        </div>

        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            marginBottom: 8,
          }}
        >
          <MPSectionLabel>Recent activity</MPSectionLabel>
          <button
            onClick={() => onAction?.('queue')}
            className="mp-focus"
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: 11.5,
              color: 'var(--mp-textMuted)',
              display: 'inline-flex',
              alignItems: 'center',
              gap: 3,
            }}
          >
            View all queue
            <Icon name="chevr" size={11} />
          </button>
        </div>
        <MPCard padding={0}>
          {recent.length === 0 ? (
            <div
              style={{
                padding: '18px 14px',
                textAlign: 'center',
                fontSize: 12,
                color: 'var(--mp-textMuted)',
              }}
            >
              No recent activity yet — finished jobs will appear here.
            </div>
          ) : (
            recent.map((j, i, arr) => (
              <div
                key={j.id}
                onClick={() => onSelectJob?.(j.id)}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: '10px 14px',
                  borderBottom:
                    i < arr.length - 1 ? '1px solid var(--mp-border)' : 'none',
                  cursor: 'pointer',
                }}
              >
                <FileGlyph ext={j.ext} size={22} />
                <div style={{ flex: 1, minWidth: 0 }}>
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
                    }}
                  >
                    {j.printer} · {j.profile}
                  </div>
                </div>
                <StatusDot status={j.status} />
                <span
                  className="mp-mono"
                  style={{
                    fontSize: 10.5,
                    color: 'var(--mp-textMuted)',
                    textTransform: 'uppercase',
                    minWidth: 60,
                    textAlign: 'right',
                  }}
                >
                  {j.status}
                </span>
              </div>
            ))
          )}
        </MPCard>

        <div
          style={{
            marginTop: 20,
            padding: '10px 12px',
            background: 'var(--mp-infoSoft)',
            borderRadius: 6,
            display: 'flex',
            gap: 8,
            alignItems: 'flex-start',
          }}
        >
          <Icon name="info" size={14} color="var(--mp-info)" />
          <div
            style={{
              fontSize: 12,
              color: 'var(--mp-info)',
              lineHeight: 1.5,
            }}
          >
            <strong>Tip · </strong>
            MaeclyPrint adds itself to the right-click menu on Windows and Mac.
            Select any number of files, right-click, and choose{' '}
            <span className="mp-mono">Print with MaeclyPrint</span> — no 15-file
            limit.
          </div>
        </div>
      </div>
    </>
  );
}
