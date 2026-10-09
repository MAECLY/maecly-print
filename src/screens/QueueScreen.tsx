import { useState, type ReactNode } from 'react';
import { toast } from 'sonner';

import { ConfirmDialog, type ConfirmState } from '@/components/ConfirmDialog';
import { Icon } from '@/components/Icon';
import { MPButton, MPSegmented, MPTopBar } from '@/components/ui';
import { useT } from '@/i18n/I18nProvider';
import { CLEAR_UNDO_MS, useQueueStore } from '@/stores/queueStore';
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
  action,
}: {
  spinner: boolean;
  title: string;
  body: string;
  /** Optional recovery control, e.g. the Retry button of the error state. */
  action?: ReactNode;
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
      {action}
    </div>
  );
}

export function QueueScreen({ view, setView }: QueueScreenProps) {
  const t = useT('queue');
  const tCommon = useT('common');
  const status = useQueueStore((s) => s.status);
  const error = useQueueStore((s) => s.error);
  const uiJobs = useQueueStore((s) => s.uiJobs);
  const counts = useQueueStore((s) => s.counts);
  const queuePaused = useQueueStore((s) => s.queuePaused);
  const init = useQueueStore((s) => s.init);
  const pauseQueue = useQueueStore((s) => s.pauseQueue);
  const resumeQueue = useQueueStore((s) => s.resumeQueue);
  const requestClearDone = useQueueStore((s) => s.requestClearDone);
  const undoClearDone = useQueueStore((s) => s.undoClearDone);
  const cancelAll = useQueueStore((s) => s.cancelAll);
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);

  const subtitle =
    view === 'board'
      ? t('subtitle.board')
      : view === 'timeline'
        ? t('subtitle.timeline')
        : t('subtitle.counts', {
            active: counts.active,
            pending: counts.pending,
            done: counts.done,
          });

  const viewToggle = (
    <MPSegmented<QueueView>
      value={view}
      onChange={setView}
      aria-label={t('views.label')}
      options={[
        { value: 'list', icon: 'list', label: t('views.list') },
        { value: 'board', icon: 'grid', label: t('views.board') },
        { value: 'timeline', icon: 'queue', label: t('views.timeline') },
      ]}
      size="sm"
    />
  );

  /**
   * Clearing the history is destructive, so it confirms first and then stays
   * undoable for five seconds (NFR-032): the store hides the jobs immediately
   * and only calls `clear_done` when the window closes.
   */
  const askClearDone = () => {
    setConfirm({
      title: t('confirm.clearTitle'),
      message: t('confirm.clearMessage', {
        count: counts.done,
        seconds: CLEAR_UNDO_MS / 1000,
      }),
      confirmLabel: t('actions.clearDone'),
      run: () => {
        // The toast pauses its countdown while hovered, so it can outlive the
        // undo window: the store tells us when the window closes and the toast
        // goes with it, and an Undo that still lands too late says so instead
        // of claiming the (already deleted) jobs were kept.
        const toastId = `clear-done-${Date.now()}`;
        const hidden = requestClearDone(() => toast.dismiss(toastId));
        if (hidden === 0) return;
        toast.success(t('toast.cleared', { count: hidden }), {
          id: toastId,
          duration: CLEAR_UNDO_MS,
          action: {
            label: t('toast.undo'),
            onClick: () => {
              if (undoClearDone()) {
                toast.success(t('toast.restored', { count: hidden }));
              } else {
                toast.error(t('toast.undoTooLate'));
              }
            },
          },
        });
      },
    });
  };

  /**
   * Cancel every unfinished job (FR-032). Confirmed first, and with no undo:
   * the backend cannot un-cancel, and pages already on the printer are gone —
   * so the confirmation, not a 5 s window, is the safety net.
   */
  const unfinished = counts.active + counts.pending;
  const askCancelAll = () => {
    setConfirm({
      title: t('confirm.cancelAllTitle'),
      message: t('confirm.cancelAllMessage', { count: unfinished }),
      confirmLabel: t('confirm.cancelAllConfirm'),
      run: async () => {
        const cancelled = await cancelAll();
        if (cancelled !== null) {
          toast.success(t('toast.cancelledAll', { count: cancelled }));
        }
      },
    });
  };

  const topBar = (
    <MPTopBar
      title={t('title')}
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
              {queuePaused ? t('actions.resumeQueue') : t('actions.pauseQueue')}
            </MPButton>
            <MPButton
              variant="ghost"
              icon="x"
              size="sm"
              disabled={unfinished === 0}
              onClick={askCancelAll}
            >
              {t('actions.cancelAll')}
            </MPButton>
            <MPButton
              variant="ghost"
              icon="trash"
              size="sm"
              disabled={counts.done === 0}
              onClick={askClearDone}
            >
              {t('actions.clearDone')}
            </MPButton>
          </>
        ) : (
          viewToggle
        )
      }
    />
  );

  let body: ReactNode;
  if (status === 'unavailable') {
    body = (
      <QueueNotice
        spinner={false}
        title={t('notice.unavailableTitle')}
        body={t('notice.unavailableBody')}
      />
    );
  } else if (status === 'error') {
    // `init()` is re-runnable after a failure, so a transient read error must
    // not leave the queue dead until the app is restarted (NFR-033).
    body = (
      <QueueNotice
        spinner={false}
        title={t('notice.errorTitle')}
        body={`${error ?? t('notice.errorBody')} ${t('notice.errorHint')}`}
        action={
          <MPButton
            variant="ghost"
            icon="rotate"
            size="sm"
            onClick={() => void init()}
          >
            {tCommon('actions.retry')}
          </MPButton>
        }
      />
    );
  } else if (status === 'idle' || status === 'loading') {
    body = (
      <QueueNotice
        spinner
        title={t('notice.loadingTitle')}
        body={t('notice.loadingBody')}
      />
    );
  } else if (uiJobs.length === 0) {
    body = (
      <QueueNotice
        spinner={false}
        title={t('empty.title')}
        body={t('empty.body')}
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
