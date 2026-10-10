import { useEffect, useRef, useState, type ReactNode } from 'react';
import { toast } from 'sonner';

import { ConfirmDialog, type ConfirmState } from '@/components/ConfirmDialog';
import { DependencyInstallDialog } from '@/components/DependencyInstallDialog';
import { PdfPasswordDialog } from '@/components/PdfPasswordDialog';
import { Icon } from '@/components/Icon';
import { MPButton, MPSegmented, MPTopBar } from '@/components/ui';
import { useT } from '@/i18n/I18nProvider';
import type { DependencyReport } from '@/ipc/bindings';
import { remediationKey } from '@/lib/deps';
import { fetchAppInfo, type UiQueueJobView } from '@/lib/ipc';
import { errorMessage } from '@/lib/ipcError';
import { CLEAR_UNDO_MS, useQueueStore } from '@/stores/queueStore';
import { missingDependency } from '@/lib/deps';
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
  const resumeJob = useQueueStore((s) => s.resumeJob);
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);
  /**
   * The job whose PDF password is being asked for (FR-059). In the store, so
   * a toast can open it and the store can hold back a second alarm for it.
   */
  const passwordFor = useQueueStore((s) => s.passwordPrompt);
  const openPasswordPrompt = useQueueStore((s) => s.openPasswordPrompt);
  const closePasswordPrompt = useQueueStore((s) => s.closePasswordPrompt);
  /** FR-058: the dependency a paused job is missing, offered for install. */
  const [installFor, setInstallFor] = useState<DependencyReport | null>(null);

  /**
   * Resume every job paused because `report`'s dependency was missing, once
   * it is found. Returns how many were resumed.
   */
  const resumeWaitingOn = (report: DependencyReport): number => {
    const waiting = useQueueStore
      .getState()
      .uiJobs.filter((j) => missingDependency(j) === report.id);
    for (const j of waiting) void resumeJob(j.id);
    return waiting.length;
  };

  /**
   * FR-058: a job paused on `dependency_missing` offers the same consent
   * dialog as Print Job. The dependency is probed afresh first: if it is
   * there by now, the job just resumes; if nothing can install it, the
   * remediation says what to do instead.
   */
  const installDependencyFor = async (job: UiQueueJobView) => {
    const id = missingDependency(job);
    if (!id) return;
    try {
      const info = await fetchAppInfo();
      const report = (info.dependencies ?? []).find((d) => d.id === id);
      if (!report) {
        toast.info(tCommon(remediationKey(id)));
      } else if (report.found) {
        resumeWaitingOn(report);
        toast.success(t('fix.alreadyInstalled', { name: report.name }));
      } else if (report.install.length === 0) {
        toast.info(tCommon(remediationKey(id)));
      } else {
        setInstallFor(report);
      }
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  /** After an install (or a background one ending): resume what it unblocks. */
  const onDependencyResolved = (report: DependencyReport) => {
    if (!report.found) return;
    const count = resumeWaitingOn(report);
    if (count > 0) {
      toast.success(t('fix.resumed', { count, name: report.name }));
    }
  };

  // FR-058: a Store or web install finishes outside the app, so while a job
  // waits on a missing dependency, coming back to the window (focus, or the
  // page becoming visible again) probes the dependencies afresh and resumes
  // what is now unblocked — the promise the "opened" install result makes.
  const waitingOnDependency = uiJobs.some((j) => missingDependency(j) !== null);
  const resolvedRef = useRef(onDependencyResolved);
  resolvedRef.current = onDependencyResolved;
  useEffect(() => {
    if (!waitingOnDependency) return;
    let probing = false;
    const recheck = async () => {
      if (probing) return;
      probing = true;
      try {
        const info = await fetchAppInfo();
        for (const report of info.dependencies ?? []) {
          resolvedRef.current(report);
        }
      } catch (e) {
        console.error('dependency re-probe failed', e);
      } finally {
        probing = false;
      }
    };
    const onFocus = () => void recheck();
    const onVisible = () => {
      if (document.visibilityState === 'visible') void recheck();
    };
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [waitingOnDependency]);

  const attention = {
    onEnterPassword: (j: UiQueueJobView) => openPasswordPrompt(j.id, j.file),
    onInstallDependency: (j: UiQueueJobView) => void installDependencyFor(j),
  };

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
    body = (
      <QueueListView jobs={uiJobs} onConfirm={setConfirm} {...attention} />
    );
  } else if (view === 'board') {
    body = <QueueBoardView jobs={uiJobs} {...attention} />;
  } else {
    body = <QueueTimelineView jobs={uiJobs} {...attention} />;
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
        {passwordFor && (
          <PdfPasswordDialog
            key={passwordFor.id}
            jobId={passwordFor.id}
            file={passwordFor.file}
            onClose={closePasswordPrompt}
          />
        )}
        {installFor && (
          <DependencyInstallDialog
            report={installFor}
            onClose={() => setInstallFor(null)}
            onResolved={onDependencyResolved}
            resumesJobs
          />
        )}
      </div>
    </>
  );
}
