/**
 * Queue store — the single source of truth for the print queue in the UI.
 *
 * `init()` fetches the initial snapshot and subscribes to the two backend
 * events: `queueUpdated` (authoritative full snapshot, replaces state
 * wholesale — FR-030) and `jobProgressEvent` (advisory per-job progress,
 * merged into the matching job by id). Derived views (`uiJobs`, `counts`,
 * `queuePaused`, `recentDone`) are recomputed once per snapshot change so
 * component selectors return stable references between updates.
 *
 * Two behaviours live here rather than in the screen because they must survive
 * navigation away from the Queue:
 *
 * - **Batch completion notifications (FR-070/FR-071).** Every snapshot is
 *   scanned for batches whose jobs have all reached a terminal state, and the
 *   ones that were not complete a moment ago produce a summary toast ("12 of
 *   12 files printed successfully"), or an actionable failure toast with a way
 *   into the job log. The FIRST snapshot of a session only SEEDS the
 *   already-complete set — whether it came from `init()`'s `queue_status` or
 *   from an event that beat it — so yesterday's history never re-announces
 *   itself.
 * - **Jobs that stop and need the user (FR-071).** A job the dispatcher pauses
 *   on an error the user has to fix (printer gone, SumatraPDF missing, a
 *   setting the driver refuses) keeps its batch from ever completing, so it is
 *   announced on its own, with an "Open queue" action. A manual pause (no
 *   error) and a shutdown interruption are not alarms.
 * - **Native notifications (FR-072).** Each of those announcements is also
 *   offered to the OS notification centre; the backend shows it only when the
 *   window is in the background and the user has not turned it off.
 * - **The "Clear done" undo window (NFR-032).** `requestClearDone()` hides the
 *   finished jobs immediately and only calls `clear_done` five seconds later,
 *   so `undoClearDone()` can put them back without needing the backend to
 *   resurrect anything.
 *
 * Toast copy is translated at call time (a store is not a component and cannot
 * use hooks) with the locale read from the settings store — the same pattern as
 * `src/lib/jobFiles.ts`.
 */

import { create } from 'zustand';
import { toast } from 'sonner';

import { events, type Job, type QueueSnapshot } from '@/ipc/bindings';
import {
  CATALOGS,
  matchLocale,
  translate,
  type TranslateParams,
  type TranslationKey,
} from '@/i18n';
import { detectSystemLocale } from '@/i18n/I18nProvider';
import { logError } from '@/lib/errorLog';
import { IpcFailure } from '@/lib/ipcError';
import {
  cancelAll as cancelAllCmd,
  cancelJob as cancelJobCmd,
  clearDone as clearDoneCmd,
  isTauri,
  notifyNative,
  openLogsFolder,
  pauseJob as pauseJobCmd,
  pauseQueue as pauseQueueCmd,
  queueStatus as queueStatusCmd,
  resumeJob as resumeJobCmd,
  resumeQueue as resumeQueueCmd,
  retryJob as retryJobCmd,
  toUiQueueJob,
  type UiQueueJobView,
} from '@/lib/ipc';
import { localizeJobError } from '@/lib/jobError';
import { usePrinterNamesStore } from '@/stores/printerNamesStore';
import { useSettingsStore } from '@/stores/settingsStore';

/** Lifecycle of the store's connection to the backend queue. */
export type QueueLoadStatus =
  | 'idle'
  | 'loading'
  | 'ready'
  | 'unavailable'
  | 'error';

export interface QueueCounts {
  active: number;
  pending: number;
  done: number;
}

/**
 * How long "Clear done" stays undoable before the backend call goes out
 * (NFR-032 asks for a 5 s Undo affordance). Exported so the screen can put the
 * number in the confirmation copy and tests don't have to guess it.
 */
export const CLEAR_UNDO_MS = 5000;

interface QueueState {
  snapshot: QueueSnapshot | null;
  status: QueueLoadStatus;
  /** Explicit `| undefined` so `set({ error: undefined })` can clear it. */
  error?: string | undefined;
  /**
   * Whether the tauri event listeners are registered, so an `init()` retry
   * after a snapshot failure doesn't double-register them.
   */
  subscribed: boolean;
  /**
   * Ids hidden by an in-flight "Clear done" undo window. They are still in the
   * backend snapshot; the derived views pretend they are gone.
   */
  pendingClear: string[];
  /**
   * Job ids with a row action (pause/resume/retry/cancel) still waiting for
   * the backend, so the row can disable its buttons instead of letting a
   * double click send the command twice (NFR-033).
   */
  pendingIds: string[];
  /**
   * Batch ids already announced as finished, so one completion produces one
   * toast. A batch that leaves the set (a job was retried) can announce again.
   */
  notifiedBatches: string[];
  /**
   * Ids of jobs currently stopped on a user-fixable error that were already
   * announced. A job that resumes leaves the set, so stopping again
   * announces again.
   */
  notifiedPaused: string[];
  // Derived — recomputed on every snapshot change (memoized at the write).
  uiJobs: UiQueueJobView[];
  counts: QueueCounts;
  queuePaused: boolean;
  /** Last five terminal jobs, newest first — for the Home "Recent activity". */
  recentDone: UiQueueJobView[];

  init: () => Promise<void>;
  pauseJob: (id: string) => Promise<void>;
  resumeJob: (id: string) => Promise<void>;
  cancelJob: (id: string) => Promise<void>;
  retryJob: (id: string) => Promise<void>;
  pauseQueue: () => Promise<void>;
  resumeQueue: () => Promise<void>;
  /**
   * Hide every finished job and schedule the real `clear_done` for
   * {@link CLEAR_UNDO_MS} from now. Returns how many jobs were hidden (0 when
   * there was nothing to clear, in which case nothing is scheduled).
   *
   * `onCommit` runs the moment the window closes and the backend call goes
   * out — the screen dismisses its Undo toast there, because a toast can
   * outlive its nominal duration (it pauses while hovered).
   */
  requestClearDone: (onCommit?: () => void) => number;
  /**
   * Cancel a pending clear and put the hidden jobs back on screen. Returns
   * `false` when there was nothing left to cancel — the window had closed and
   * `clear_done` already went out — so the caller never claims the jobs were
   * kept when they are gone.
   */
  undoClearDone: () => boolean;
  /** Resolves to the cancelled count, or `null` when the backend call failed. */
  cancelAll: () => Promise<number | null>;
}

const TERMINAL = new Set(['done', 'failed', 'cancelled']);

/**
 * A paused job with one of these error codes did not stop because of the user:
 * shutdown recovery parks in-flight jobs this way, and resuming is all it
 * takes. Every other error on a paused job means something must be fixed.
 */
const BENIGN_PAUSE_CODES = new Set(['interrupted_by_shutdown']);

/** Where "Open queue" goes; registered by the app shell (`App.tsx`). */
let navigateToQueue: (() => void) | null = null;

/**
 * Let the app shell tell the store how to show the Queue screen. A store
 * cannot navigate by itself, and the toasts it raises must offer a way there.
 */
export function setQueueNavigator(fn: (() => void) | null): void {
  navigateToQueue = fn;
}

/** The locale to translate a toast into, read at call time (never a hook). */
function activeLocale() {
  const configured = useSettingsStore.getState().settings?.app.locale;
  return matchLocale(configured) ?? detectSystemLocale();
}

/** Translate a `queue` key using whatever locale is active right now. */
function t(key: TranslationKey<'queue'>, params?: TranslateParams): string {
  return translate(CATALOGS[activeLocale()], 'queue', key, params);
}

/** Same, for the shared `common` namespace (action labels). */
function tCommon(
  key: TranslationKey<'common'>,
  params?: TranslateParams,
): string {
  return translate(CATALOGS[activeLocale()], 'common', key, params);
}

/** Milliseconds since the epoch for an RFC 3339 stamp; `null` when unusable. */
function epoch(ts: string | null): number | null {
  if (!ts) return null;
  const ms = new Date(ts).getTime();
  return Number.isNaN(ms) ? null : ms;
}

/**
 * Recency key of a terminal job. PARSE the timestamps rather than comparing the
 * strings: `time`'s RFC 3339 formatter emits fractional seconds only when they
 * are non-zero, so `"…:00Z"` sorts lexicographically AFTER `"…:00.5Z"` (`.` <
 * `Z`) — the exact trap documented in `src-tauri/src/queue/persist.rs`. Jobs
 * with no usable `finished_at` fall back to `submitted_at`, matching what
 * `prune_history` does on the Rust side.
 */
function finishedRank(job: Job): number {
  return epoch(job.finished_at) ?? epoch(job.submitted_at) ?? 0;
}

/** Everything derived from a snapshot, computed in one place. */
function derive(
  snapshot: QueueSnapshot | null,
  pendingClear: readonly string[],
): {
  snapshot: QueueSnapshot | null;
  pendingClear: string[];
  uiJobs: UiQueueJobView[];
  counts: QueueCounts;
  queuePaused: boolean;
  recentDone: UiQueueJobView[];
} {
  const hidden = new Set(pendingClear);
  const jobs = (snapshot?.jobs ?? []).filter((j) => !hidden.has(j.id));
  const uiJobs = jobs.map(toUiQueueJob);
  const counts: QueueCounts = {
    active: uiJobs.filter(
      (j) =>
        j.status === 'printing' ||
        j.status === 'converting' ||
        j.status === 'converted',
    ).length,
    pending: uiJobs.filter(
      (j) => j.status === 'queued' || j.status === 'paused',
    ).length,
    done: uiJobs.filter((j) => TERMINAL.has(j.status)).length,
  };
  const recentDone = jobs
    .filter((j) => TERMINAL.has(j.state))
    // Newest first, with the id as a tie-break so the order is total and does
    // not depend on the incoming job order.
    .sort(
      (a, b) => finishedRank(b) - finishedRank(a) || a.id.localeCompare(b.id),
    )
    .slice(0, 5)
    .map(toUiQueueJob);
  return {
    snapshot,
    pendingClear: [...pendingClear],
    uiJobs,
    counts,
    queuePaused: snapshot?.queue_paused ?? false,
    recentDone,
  };
}

/** Ids of the batches in `snapshot` whose every job has reached a terminal state. */
function completeBatchIds(snapshot: QueueSnapshot | null): string[] {
  const byBatch = new Map<string, Job[]>();
  for (const job of snapshot?.jobs ?? []) {
    const bucket = byBatch.get(job.batch_id);
    if (bucket) bucket.push(job);
    else byBatch.set(job.batch_id, [job]);
  }
  return [...byBatch.entries()]
    .filter(([, jobs]) => jobs.every((j) => TERMINAL.has(j.state)))
    .map(([id]) => id);
}

/** Ids of the jobs in `snapshot` stopped on an error the user has to fix. */
function needsActionIds(snapshot: QueueSnapshot | null): string[] {
  return (snapshot?.jobs ?? [])
    .filter(
      (j) =>
        j.state === 'paused' &&
        j.error !== null &&
        !BENIGN_PAUSE_CODES.has(j.error.code),
    )
    .map((j) => j.id);
}

/**
 * FR-072: offer an announcement to the OS notification centre as well. The
 * backend decides whether to show it (window in the background, switch on);
 * any failure is irrelevant here because the toast has already been shown.
 */
function notifyOs(title: string, body: string): void {
  if (!isTauri()) return;
  notifyNative(title, body).catch(() => {});
}

/**
 * A job's error in the UI language (FR-092), naming printers as the Printers
 * screen does when their display name is known. The closing full stop is
 * dropped: the toast templates continue the sentence (`{{error}} — fix it…`).
 */
function jobErrorText(job: Job | undefined): string {
  if (!job?.error) return '';
  const names = usePrinterNamesStore.getState().names;
  return localizeJobError(job.error, {
    locale: activeLocale(),
    printerName: (id) => names[id],
  }).replace(/\.$/, '');
}

/** Announce a job that stopped and is waiting for the user (FR-071). */
function notifyJobNeedsAction(snapshot: QueueSnapshot, jobId: string): void {
  const job = snapshot.jobs.find((j) => j.id === jobId);
  if (!job) return;
  const title = t('notify.jobNeedsAction', { file: job.file_name });
  const description = t('notify.jobNeedsActionHint', {
    error: jobErrorText(job),
  });
  toast.error(title, {
    description,
    duration: 10000,
    action: navigateToQueue
      ? { label: t('notify.openQueue'), onClick: () => navigateToQueue?.() }
      : {
          label: tCommon('actions.openLogsFolder'),
          onClick: () => {
            openLogsFolder().catch((e: unknown) => toast.error(message(e)));
          },
        },
  });
  notifyOs(title, description);
}

/**
 * Announce one finished batch (FR-070 / FR-071).
 *
 * A batch with failures wins over the success summary: the user needs the
 * error, what to do about it, and a way into the job log — which is the logs
 * folder, the finest-grained entry point the backend exposes today.
 *
 * `jobs` is every job the batch has had this session, including ones a
 * "Clear done" already removed from the snapshot: counting only what is left
 * would announce "1 of 1" for a batch of three.
 */
function notifyBatchComplete(jobs: readonly Job[]): void {
  const total = jobs.length;
  if (total === 0) return;
  const done = jobs.filter((j) => j.state === 'done').length;
  const failed = jobs.filter((j) => j.state === 'failed');
  const cancelled = jobs.filter((j) => j.state === 'cancelled').length;

  if (failed.length > 0) {
    const first = failed[0];
    // `count` is the batch size: it decides "file" vs "files", exactly as in
    // the success copy ("1 of 2 files failed", not "1 of 2 file failed").
    const title = t('notify.batchFailed', {
      count: total,
      failed: failed.length,
      total,
    });
    const description = t('notify.batchFailedHint', {
      file: first?.file_name ?? '',
      error: jobErrorText(first),
    });
    notifyOs(title, description);
    toast.error(title, {
      description,
      duration: 10000,
      action: {
        label: tCommon('actions.openLogsFolder'),
        onClick: () => {
          openLogsFolder().catch((e: unknown) => toast.error(message(e)));
        },
      },
    });
    return;
  }
  if (done === 0) {
    // Nothing printed and nothing failed: the user cancelled it themselves, so
    // a neutral acknowledgement, not a success or an error.
    toast.message(t('notify.batchCancelled', { count: cancelled }));
    return;
  }
  const summary = t('notify.batchDone', { count: total, done, total });
  toast.success(summary);
  notifyOs('Pluryprint', summary);
}

/** Turn an unknown thrown value into a user-facing message. */
function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useQueueStore = create<QueueState>()((set, get) => {
  /** Timer of the open "Clear done" undo window, if any. */
  let clearTimer: ReturnType<typeof setTimeout> | null = null;
  /** Called when that window closes and `clear_done` goes out. */
  let onClearCommit: (() => void) | null = null;

  /**
   * Every job each batch has had this session, by batch id then job id, last
   * known state. Survives a "Clear done" that removes a batch's finished jobs
   * from the snapshot while the rest is still printing, so the completion
   * toast counts the whole batch (FR-070). Pruned once a batch is entirely
   * gone from the snapshot; reset when a session's first snapshot is adopted.
   */
  const batchMembers = new Map<string, Map<string, Job>>();

  function trackBatches(next: QueueSnapshot, fresh: boolean): void {
    if (fresh) batchMembers.clear();
    const live = new Set<string>();
    for (const job of next.jobs) {
      live.add(job.batch_id);
      let members = batchMembers.get(job.batch_id);
      if (!members) {
        members = new Map();
        batchMembers.set(job.batch_id, members);
      }
      members.set(job.id, job);
    }
    for (const id of [...batchMembers.keys()]) {
      if (!live.has(id)) batchMembers.delete(id);
    }
  }

  /**
   * Whether some snapshot — from `queue_status` OR from an event that beat it
   * — has already been adopted. The FIRST one only SEEDS `notifiedBatches`:
   * every batch it contains finished before this session was watching, so
   * announcing it would replay history. Read off the state rather than kept in
   * a flag so it cannot drift from what the store actually holds.
   */
  function seeded(): boolean {
    return get().snapshot !== null;
  }

  /**
   * Merge advisory progress into the matching job. Unknown ids are ignored,
   * and so are jobs that are no longer running: a throttled progress event can
   * arrive after the job was paused/cancelled, and re-attaching an active
   * progress bar to it would contradict the authoritative snapshot.
   */
  function applyProgress(id: string, phase: string, percent: number): void {
    const cur = get().snapshot;
    if (!cur) return;
    let changed = false;
    const jobs = cur.jobs.map((j) => {
      if (j.id !== id) return j;
      if (j.state !== 'converting' && j.state !== 'printing') return j;
      changed = true;
      return { ...j, progress: { phase, percent } };
    });
    if (changed) set(derive({ ...cur, jobs }, get().pendingClear));
  }

  /** Adopt an authoritative snapshot and announce any batch that just ended. */
  function adopt(next: QueueSnapshot, announce: boolean): void {
    trackBatches(next, !seeded());
    const complete = completeBatchIds(next);
    const known = get().notifiedBatches;
    const fresh = announce ? complete.filter((id) => !known.includes(id)) : [];
    const stuck = needsActionIds(next);
    const knownStuck = get().notifiedPaused;
    const freshStuck = announce
      ? stuck.filter((id) => !knownStuck.includes(id))
      : [];
    set({
      ...derive(next, get().pendingClear),
      status: 'ready',
      notifiedBatches: complete,
      notifiedPaused: stuck,
    });
    for (const id of freshStuck) notifyJobNeedsAction(next, id);
    for (const id of fresh) {
      notifyBatchComplete([...(batchMembers.get(id)?.values() ?? [])]);
    }
  }

  /** Send the deferred `clear_done` once the undo window has elapsed. */
  async function commitClearDone(): Promise<void> {
    clearTimer = null;
    const notify = onClearCommit;
    onClearCommit = null;
    notify?.();
    if (get().pendingClear.length === 0) return;
    try {
      await clearDoneCmd();
    } catch (e) {
      toast.error(message(e));
    } finally {
      // Drop the local mask either way: on success the backend's own
      // `queue-updated` already removed the jobs, and on failure they must come
      // back on screen instead of staying invisible for the session.
      set(derive(get().snapshot, []));
    }
  }

  /** Run a fallible action wrapper, surfacing failures as an error toast. */
  async function guard(run: () => Promise<void>): Promise<void> {
    try {
      await run();
    } catch (e) {
      toast.error(message(e));
    }
  }

  /**
   * Run one row action for job `id`: at most one in flight per job (a second
   * click while the first is pending is dropped, and the row disables its
   * buttons meanwhile), and a refusal that only means "the job moved on
   * before the command arrived" — it finished, or was removed — becomes a
   * translated note instead of the backend's English text with a raw UUID.
   */
  async function jobAction(
    id: string,
    run: (id: string) => Promise<void>,
  ): Promise<void> {
    if (get().pendingIds.includes(id)) return;
    const file = get().snapshot?.jobs.find((j) => j.id === id)?.file_name;
    set({ pendingIds: [...get().pendingIds, id] });
    try {
      await run(id);
    } catch (e) {
      const now = get().snapshot?.jobs.find((j) => j.id === id);
      const movedOn =
        e instanceof IpcFailure &&
        (e.code === 'JobNotFound' ||
          (e.code === 'InvalidState' && (!now || TERMINAL.has(now.state))));
      if (movedOn) {
        toast.info(t('notify.alreadyFinished', { file: file ?? '' }));
      } else {
        toast.error(message(e));
      }
    } finally {
      set({ pendingIds: get().pendingIds.filter((p) => p !== id) });
    }
  }

  return {
    snapshot: null,
    status: 'idle',
    subscribed: false,
    pendingClear: [],
    pendingIds: [],
    notifiedBatches: [],
    notifiedPaused: [],
    uiJobs: [],
    counts: { active: 0, pending: 0, done: 0 },
    queuePaused: false,
    recentDone: [],

    init: async () => {
      // Idempotent while loading/ready; re-runnable after a failure so one
      // transient error doesn't leave the queue UI dead for the session.
      const status = get().status;
      if (status !== 'idle' && status !== 'error') return;
      if (!isTauri()) {
        set({ status: 'unavailable' });
        return;
      }
      set({ status: 'loading', error: undefined });

      // Subscribe BEFORE fetching the snapshot so a transition emitted while
      // the fetch is in flight is not lost. Events carry the full
      // authoritative snapshot, so an event that lands first simply makes the
      // (older) fetch result redundant — and, because it is then the first
      // snapshot this session sees, it seeds instead of announcing.
      if (!get().subscribed) {
        // Registered listeners, so a half-done subscription can be undone. A
        // listener left alive with `subscribed: false` would be re-registered
        // by the next `init()` and every event would be handled twice.
        const unlisten: (() => void)[] = [];
        try {
          unlisten.push(
            await events.queueUpdated.listen((ev) => {
              adopt(ev.payload, seeded());
            }),
          );
          unlisten.push(
            await events.jobProgressEvent.listen((ev) => {
              applyProgress(
                ev.payload.id,
                ev.payload.phase,
                ev.payload.percent,
              );
            }),
          );
          set({ subscribed: true });
        } catch (e) {
          for (const off of unlisten) {
            try {
              off();
            } catch (offError) {
              logError('queue.unsubscribe', offError);
            }
          }
          // The snapshot below still works; without events the view is a
          // one-shot, so surface it rather than pretending we're live.
          logError('queue.subscribe', e);
          toast.error(t('notify.liveUnavailable'));
        }
      }

      try {
        const snap = await queueStatusCmd();
        if (seeded()) {
          // A fresher full snapshot already arrived via the event stream;
          // applying the older fetch result would rewind the UI. It also
          // already did the seeding, so nothing here may announce.
          set({ status: 'ready' });
        } else {
          // `announce: false` — everything terminal in the first snapshot
          // finished before this session was watching.
          adopt(snap, false);
        }
      } catch (e) {
        if (!seeded()) set({ status: 'error', error: message(e) });
      }
    },

    pauseJob: (id) => jobAction(id, pauseJobCmd),
    resumeJob: (id) => jobAction(id, resumeJobCmd),
    cancelJob: (id) => jobAction(id, cancelJobCmd),
    retryJob: (id) => jobAction(id, retryJobCmd),
    pauseQueue: () => guard(() => pauseQueueCmd()),
    resumeQueue: () => guard(() => resumeQueueCmd()),

    requestClearDone: (onCommit) => {
      // Known limitation of doing this client-side: `clear_done` has no id
      // list, so a job that reaches a terminal state DURING the undo window is
      // swept too even though it was never hidden. Five seconds is short
      // enough that this is preferable to a backend-side deferred delete.
      const ids = (get().snapshot?.jobs ?? [])
        .filter((j) => TERMINAL.has(j.state))
        .map((j) => j.id);
      if (ids.length === 0) return 0;
      const pending = [...new Set([...get().pendingClear, ...ids])];
      set(derive(get().snapshot, pending));
      if (clearTimer) clearTimeout(clearTimer);
      // A superseded window's toast is going away too: let its owner know.
      onClearCommit?.();
      onClearCommit = onCommit ?? null;
      clearTimer = setTimeout(() => void commitClearDone(), CLEAR_UNDO_MS);
      return ids.length;
    },

    undoClearDone: () => {
      // No timer means no window: either nothing was pending, or the backend
      // call is already out (and its own `finally` restores the view). In both
      // cases there is nothing to undo, and saying otherwise would be a lie.
      if (!clearTimer) return false;
      clearTimeout(clearTimer);
      clearTimer = null;
      onClearCommit = null;
      set(derive(get().snapshot, []));
      return true;
    },

    cancelAll: async () => {
      try {
        return await cancelAllCmd();
      } catch (e) {
        toast.error(message(e));
        return null;
      }
    },
  };
});
