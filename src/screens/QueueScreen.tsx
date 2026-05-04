import { MPButton, MPSegmented, MPTopBar } from '@/components/ui';
import type { UiQueueJob } from '@/types/domain';
import { QueueListView } from './QueueViews/QueueListView';
import { QueueBoardView } from './QueueViews/QueueBoardView';
import { QueueTimelineView } from './QueueViews/QueueTimelineView';

export type QueueView = 'list' | 'board' | 'timeline';

interface QueueScreenProps {
  jobs: UiQueueJob[];
  setJobs: (jobs: UiQueueJob[]) => void;
  view: QueueView;
  setView: (v: QueueView) => void;
}

export function QueueScreen({
  jobs,
  setJobs,
  view,
  setView,
}: QueueScreenProps) {
  const groups = {
    active: jobs.filter((j) =>
      ['printing', 'converting', 'converted'].includes(j.status),
    ),
    pending: jobs.filter((j) => ['queued', 'paused'].includes(j.status)),
    done: jobs.filter((j) =>
      ['done', 'failed', 'cancelled'].includes(j.status),
    ),
  };

  const subtitle =
    view === 'board'
      ? 'Board view — kanban across job phases'
      : view === 'timeline'
        ? 'Timeline — chronological, with durations'
        : `${groups.active.length} active · ${groups.pending.length} pending · ${groups.done.length} done`;

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

  return (
    <>
      <MPTopBar
        title="Queue"
        subtitle={subtitle}
        right={
          view === 'list' ? (
            <>
              {viewToggle}
              <MPButton variant="ghost" icon="pause" size="sm">
                Pause queue
              </MPButton>
              <MPButton variant="ghost" icon="trash" size="sm">
                Clear done
              </MPButton>
            </>
          ) : (
            viewToggle
          )
        }
      />
      {view === 'list' && <QueueListView jobs={jobs} setJobs={setJobs} />}
      {view === 'board' && <QueueBoardView jobs={jobs} />}
      {view === 'timeline' && <QueueTimelineView jobs={jobs} />}
    </>
  );
}
