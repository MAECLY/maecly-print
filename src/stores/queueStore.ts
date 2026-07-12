/**
 * Queue store — the single source of truth for the print queue in the UI.
 *
 * `init()` fetches the initial snapshot and subscribes to the two backend
 * events: `queueUpdated` (authoritative full snapshot, replaces state
 * wholesale — FR-030) and `jobProgressEvent` (advisory per-job progress,
 * merged into the matching job by id). Derived views (`uiJobs`, `counts`,
 * `queuePaused`, `recentDone`) are recomputed once per snapshot change so
 * component selectors return stable references between updates.
 */

import { create } from 'zustand';
import { toast } from 'sonner';

import { events, type QueueSnapshot } from '@/ipc/bindings';
import {
  cancelAll as cancelAllCmd,
  cancelJob as cancelJobCmd,
  clearDone as clearDoneCmd,
  isTauri,
  pauseJob as pauseJobCmd,
  pauseQueue as pauseQueueCmd,
  queueStatus as queueStatusCmd,
  resumeJob as resumeJobCmd,
  resumeQueue as resumeQueueCmd,
  retryJob as retryJobCmd,
  toUiQueueJob,
} from '@/lib/ipc';
import type { UiQueueJob } from '@/types/domain';

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

interface QueueState {
  snapshot: QueueSnapshot | null;
  status: QueueLoadStatus;
  error?: string;
  /**
   * Whether the tauri event listeners are registered, so an `init()` retry
   * after a snapshot failure doesn't double-register them.
   */
  subscribed: boolean;
  // Derived — recomputed on every snapshot change (memoized at the write).
  uiJobs: UiQueueJob[];
  counts: QueueCounts;
  queuePaused: boolean;
  /** Last five terminal jobs, newest first — for the Home "Recent activity". */
  recentDone: UiQueueJob[];

  init: () => Promise<void>;
  pauseJob: (id: string) => Promise<void>;
  resumeJob: (id: string) => Promise<void>;
  cancelJob: (id: string) => Promise<void>;
  retryJob: (id: string) => Promise<void>;
  pauseQueue: () => Promise<void>;
  resumeQueue: () => Promise<void>;
  /** Resolves to the pruned count, or `null` when the backend call failed. */
  clearDone: () => Promise<number | null>;
  /** Resolves to the cancelled count, or `null` when the backend call failed. */
  cancelAll: () => Promise<number | null>;
}

const TERMINAL = new Set(['done', 'failed', 'cancelled']);

/** Everything derived from a snapshot, computed in one place. */
function derive(snapshot: QueueSnapshot | null): {
  snapshot: QueueSnapshot | null;
  uiJobs: UiQueueJob[];
  counts: QueueCounts;
  queuePaused: boolean;
  recentDone: UiQueueJob[];
} {
  const jobs = snapshot?.jobs ?? [];
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
  // RFC 3339 UTC timestamps sort lexicographically, so string compare on
  // `finished_at` gives newest-first without parsing dates.
  const recentDone = jobs
    .filter((j) => TERMINAL.has(j.state))
    .sort((a, b) => (b.finished_at ?? '').localeCompare(a.finished_at ?? ''))
    .slice(0, 5)
    .map(toUiQueueJob);
  return {
    snapshot,
    uiJobs,
    counts,
    queuePaused: snapshot?.queue_paused ?? false,
    recentDone,
  };
}

/** Turn an unknown thrown value into a user-facing message. */
function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useQueueStore = create<QueueState>()((set, get) => {
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
    if (changed) set(derive({ ...cur, jobs }));
  }

  /** Run a fallible action wrapper, surfacing failures as an error toast. */
  async function guard(run: () => Promise<void>): Promise<void> {
    try {
      await run();
    } catch (e) {
      toast.error(message(e));
    }
  }

  return {
    snapshot: null,
    status: 'idle',
    subscribed: false,
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
      // (older) fetch result redundant.
      let eventSeen = false;
      if (!get().subscribed) {
        try {
          await events.queueUpdated.listen((ev) => {
            eventSeen = true;
            set({ ...derive(ev.payload), status: 'ready' });
          });
          await events.jobProgressEvent.listen((ev) => {
            applyProgress(ev.payload.id, ev.payload.phase, ev.payload.percent);
          });
          set({ subscribed: true });
        } catch (e) {
          // The snapshot below still works; without events the view is a
          // one-shot, so surface it rather than pretending we're live.
          console.error('queue event subscription failed', e);
          toast.error(
            'Live queue updates unavailable — reopen the app to retry.',
          );
        }
      }

      try {
        const snap = await queueStatusCmd();
        if (eventSeen) {
          // A fresher full snapshot already arrived via the event stream;
          // applying the older fetch result would rewind the UI.
          set({ status: 'ready' });
        } else {
          set({ ...derive(snap), status: 'ready' });
        }
      } catch (e) {
        if (!eventSeen) set({ status: 'error', error: message(e) });
      }
    },

    pauseJob: (id) => guard(() => pauseJobCmd(id)),
    resumeJob: (id) => guard(() => resumeJobCmd(id)),
    cancelJob: (id) => guard(() => cancelJobCmd(id)),
    retryJob: (id) => guard(() => retryJobCmd(id)),
    pauseQueue: () => guard(() => pauseQueueCmd()),
    resumeQueue: () => guard(() => resumeQueueCmd()),

    clearDone: async () => {
      try {
        return await clearDoneCmd();
      } catch (e) {
        toast.error(message(e));
        return null;
      }
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
