import { useState } from 'react';
import { toast } from 'sonner';

import { ConfirmDialog, type ConfirmState } from '@/components/ConfirmDialog';
import { Icon } from '@/components/Icon';
import { MPButton, MPSegmented, MPTopBar } from '@/components/ui';
import { useQueueStore } from '@/stores/queueStore';
import { QueueListView } from './QueueViews/QueueListView';
import { QueueBoardView } from './QueueViews/QueueBoardView';
import { QueueTimelineView } from './QueueViews/QueueTimelineView';

export type QueueView = 'list' | 'board' | 'timeline';

interface QueueScreenProps {
  view: QueueView;
  setView: (v: QueueView) => void;
}

/** Centered loading / unavailable / empty state for the queue body. */
function QueueNotice({
  spinner,
  title,
  body,
}: {
  spinner: boolean;
  title: string;
  body: string;
}) {
  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 10,
        padding: 24,
        textAlign: 'center',
      }}
    >
      {spinner ? (
        <div
          style={{
            width: 22,
            height: 22,
            border: '2px solid var(--mp-surface3)',
            borderTopColor: 'var(--mp-accent)',
            borderRadius: '50%',
            animation: 'mp-spin 0.9s linear infinite',
          }}
        />
      ) : (
        <Icon name="queue" size={26} color="var(--mp-textDim)" />
      )}
      <div style={{ fontSize: 13.5, fontWeight: 500 }}>{title}</div>
      <div
        style={{
          fontSize: 12,
          color: 'var(--mp-textMuted)',
          maxWidth: 380,
          lineHeight: 1.5,
        }}
      >
        {body}
      </div>
    </div>
  );
}

export function QueueScreen({ view, setView }: QueueScreenProps) {
  const status = useQueueStore((s) => s.status);
  const error = useQueueStore((s) => s.error);
  const uiJobs = useQueueStore((s) => s.uiJobs);
  const counts = useQueueStore((s) => s.counts);
  const queuePaused = useQueueStore((s) => s.queuePaused);
  const pauseQueue = useQueueStore((s) => s.pauseQueue);
  const resumeQueue = useQueueStore((s) => s.resumeQueue);
  const clearDone = useQueueStore((s) => s.clearDone);
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);

  const subtitle =
    view === 'board'
      ? 'Board view — kanban across job phases'
      : view === 'timeline'
        ? 'Timeline — chronological, with durations'
        : `${counts.active} active · ${counts.pending} pending · ${counts.done} done`;

  const viewToggle = (
    <MPSegmented<QueueView>
      value={view}
      onChange={setView}
      options={[
        { value: 'list', icon: 'list', label: 'List' },
        { value: 'board', icon: 'grid', label: 'Board' },
        { value: 'timeline', icon: 'queue', label: 'Timeline' },
      ]}
      size="sm"
    />
  );

  const requestClearDone = () => {
    setConfirm({
      title: 'Clear finished jobs',
      message: `Clear ${counts.done} finished ${
        counts.done === 1 ? 'job' : 'jobs'
      }? This removes them from history.`,
      confirmLabel: 'Clear done',
      run: async () => {
        const removed = await clearDone();
        // `null` = the backend call failed; the store already showed the
        // error toast, so a success toast here would be a lie.
        if (removed !== null) {
          toast.success(
            `Cleared ${removed} finished ${removed === 1 ? 'job' : 'jobs'}`,
          );
        }
      },
    });
  };

  const topBar = (
    <MPTopBar
      title="Queue"
      subtitle={subtitle}
      right={
        view === 'list' ? (
          <>
            {viewToggle}
            <MPButton
              variant="ghost"
              icon={queuePaused ? 'play' : 'pause'}
              size="sm"
              onClick={() => void (queuePaused ? resumeQueue() : pauseQueue())}
            >
              {queuePaused ? 'Resume queue' : 'Pause queue'}
            </MPButton>
            <MPButton
              variant="ghost"
              icon="trash"
              size="sm"
              disabled={counts.done === 0}
              onClick={requestClearDone}
            >
              Clear done
            </MPButton>
          </>
        ) : (
          viewToggle
        )
      }
    />
  );

  let body: React.ReactNode;
  if (status === 'unavailable') {
    body = (
      <QueueNotice
        spinner={false}
        title="Queue unavailable"
        body="The print queue is managed by the desktop app — run MaeclyPrint to see and control jobs."
      />
    );
  } else if (status === 'error') {
    body = (
      <QueueNotice
        spinner={false}
        title="Queue unavailable"
        body={error ?? 'The queue could not be read.'}
      />
    );
  } else if (status === 'idle' || status === 'loading') {
    body = (
      <QueueNotice
        spinner
        title="Loading the queue…"
        body="Reading the current print jobs."
      />
    );
  } else if (uiJobs.length === 0) {
    body = (
      <QueueNotice
        spinner={false}
        title="No jobs in the queue"
        body="Print something and it will show up here — active, pending, and recently finished jobs."
      />
    );
  } else if (view === 'list') {
    body = <QueueListView jobs={uiJobs} onConfirm={setConfirm} />;
  } else if (view === 'board') {
    body = <QueueBoardView jobs={uiJobs} />;
  } else {
    body = <QueueTimelineView jobs={uiJobs} />;
  }

  return (
    <>
      {topBar}
      <div
        style={{
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          minHeight: 0,
          position: 'relative',
        }}
      >
        {body}
        {confirm && (
          <ConfirmDialog state={confirm} onClose={() => setConfirm(null)} />
        )}
      </div>
    </>
  );
}
