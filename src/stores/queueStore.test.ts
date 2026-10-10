import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { toast } from 'sonner';

import type { Job, JobProgressEvent, QueueSnapshot } from '@/ipc/bindings';
import {
  fixtureAppSettings,
  fixtureQueueSnapshot,
  mkJob,
} from '@/test/fixtures';
import {
  resetPrinterNames,
  usePrinterNamesStore,
} from '@/stores/printerNamesStore';
import { useSettingsStore } from '@/stores/settingsStore';

// The tauri-specta `events` object drives the event plugin over IPC, which
// mockIPC would also intercept. Swapping just `events` for a controllable fake
// (while keeping the real `commands`, so mockIPC still serves command invokes)
// is the cleanest reliable way to emit events on demand. `vi.hoisted` makes the
// registry available inside the hoisted `vi.mock` factory.
type Listener = (e: { payload: unknown }) => void;
const eventMock = vi.hoisted(() => {
  const queueUpdated = new Set<(e: { payload: unknown }) => void>();
  const jobProgressEvent = new Set<(e: { payload: unknown }) => void>();
  /**
   * Event names whose `listen` must reject. The interesting failure is a
   * *partial* one — the first listener registers, the second rejects — because
   * that is what can leave a live listener behind with `subscribed: false`.
   */
  const failListen = new Set<string>();
  const listener = (
    name: string,
    set: Set<(e: { payload: unknown }) => void>,
  ) =>
    ({
      listen: (cb: (e: { payload: unknown }) => void) => {
        if (failListen.has(name))
          return Promise.reject(new Error(`cannot listen to ${name}`));
        set.add(cb);
        return Promise.resolve(() => set.delete(cb));
      },
    }) as const;
  return {
    queueUpdated,
    jobProgressEvent,
    failListen,
    events: {
      queueUpdated: listener('queueUpdated', queueUpdated),
      jobProgressEvent: listener('jobProgressEvent', jobProgressEvent),
    },
  };
});

vi.mock('@/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/ipc/bindings')>();
  return { ...actual, events: eventMock.events };
});

// Imported after the mock is registered (vi.mock is hoisted regardless).
import {
  CLEAR_UNDO_MS,
  setQueueNavigator,
  useQueueStore,
} from '@/stores/queueStore';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

function mockQueue(
  opts: { snapshot?: QueueSnapshot; fail?: string } = {},
): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    if (cmd === 'queue_status') return opts.snapshot ?? fixtureQueueSnapshot;
    if (cmd === 'clear_done') return 4;
    if (cmd === 'cancel_all') return 2;
    return null;
  });
  return calls;
}

function emitQueueUpdated(snapshot: QueueSnapshot) {
  for (const cb of eventMock.queueUpdated as Set<Listener>)
    cb({ payload: snapshot });
}

function emitProgress(payload: JobProgressEvent) {
  for (const cb of eventMock.jobProgressEvent as Set<Listener>) cb({ payload });
}

/** A snapshot of `jobs`, running queue. */
function snapshotOf(jobs: Job[]): QueueSnapshot {
  return { schema_version: 1, queue_paused: false, jobs };
}

function resetStore() {
  eventMock.queueUpdated.clear();
  eventMock.jobProgressEvent.clear();
  eventMock.failListen.clear();
  useQueueStore.setState({
    status: 'idle',
    snapshot: null,
    subscribed: false,
    pendingClear: [],
    pendingIds: [],
    notifiedBatches: [],
    notifiedPaused: [],
    uiJobs: [],
    counts: { active: 0, pending: 0, done: 0 },
    queuePaused: false,
    recentDone: [],
  });
}

describe('queueStore', () => {
  beforeEach(() => resetStore());
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  it('FR-030: loads the initial snapshot and derives counts on init', async () => {
    mockQueue();
    await useQueueStore.getState().init();

    const s = useQueueStore.getState();
    expect(s.status).toBe('ready');
    expect(s.snapshot).toEqual(fixtureQueueSnapshot);
    expect(s.uiJobs).toHaveLength(9);
    expect(s.counts).toEqual({ active: 3, pending: 3, done: 3 });
    expect(s.queuePaused).toBe(false);
    // recentDone is the terminal jobs, newest finished_at first.
    expect(s.recentDone.map((j) => j.id)).toEqual([
      'j-cancelled',
      'j-done',
      'j-failed',
    ]);
  });

  it('FR-030: is idempotent — a second init does not re-fetch', async () => {
    const calls = mockQueue();
    await useQueueStore.getState().init();
    await useQueueStore.getState().init();
    expect(calls.filter((c) => c.cmd === 'queue_status')).toHaveLength(1);
  });

  it('FR-030: replaces the snapshot wholesale on queueUpdated', async () => {
    mockQueue();
    await useQueueStore.getState().init();

    const next = snapshotOf([
      mkJob({
        id: 'x',
        state: 'queued',
        file_name: 'only.pdf',
        started_at: null,
      }),
    ]);
    emitQueueUpdated({ ...next, queue_paused: true });

    const s = useQueueStore.getState();
    expect(s.queuePaused).toBe(true);
    expect(s.uiJobs).toHaveLength(1);
    expect(s.counts).toEqual({ active: 0, pending: 1, done: 0 });
  });

  it('FR-031: merges job progress by id and ignores unknown ids', async () => {
    mockQueue();
    await useQueueStore.getState().init();

    emitProgress({ id: 'j-converting', phase: 'converting', percent: 0.9 });
    const job = useQueueStore
      .getState()
      .snapshot!.jobs.find((j) => j.id === 'j-converting');
    expect(job?.progress).toEqual({ phase: 'converting', percent: 0.9 });

    // An unknown id is a no-op: the snapshot reference is untouched.
    const before = useQueueStore.getState().snapshot;
    emitProgress({ id: 'nope', phase: 'x', percent: 0.5 });
    expect(useQueueStore.getState().snapshot).toBe(before);
  });

  it('FR-031: an indeterminate printing phase is not flattened to 0 %', async () => {
    mockQueue({
      snapshot: snapshotOf([
        mkJob({
          id: 'p',
          state: 'printing',
          file_name: 'spooling.pdf',
          progress: { phase: 'printing', percent: null },
        }),
      ]),
    });
    await useQueueStore.getState().init();

    const [job] = useQueueStore.getState().uiJobs;
    expect(job?.indeterminate).toBe(true);
  });

  it('FR-031: a print the OS queue is holding, or a stopped printer, gets a note', async () => {
    mockQueue({
      snapshot: snapshotOf([
        mkJob({
          id: 'held',
          state: 'printing',
          file_name: 'held.pdf',
          progress: { phase: 'held', percent: null },
        }),
        mkJob({
          id: 'stopped',
          state: 'printing',
          file_name: 'stopped.pdf',
          progress: { phase: 'stopped', percent: null },
        }),
      ]),
    });
    await useQueueStore.getState().init();

    const byId = new Map(useQueueStore.getState().uiJobs.map((j) => [j.id, j]));
    expect(byId.get('held')?.noteKey).toBe('heldByPrinter');
    expect(byId.get('held')?.indeterminate).toBe(true);
    expect(byId.get('stopped')?.noteKey).toBe('printerNeedsAttention');
  });

  it('FR-032: invokes the matching command for each job action', async () => {
    const calls = mockQueue();
    await useQueueStore.getState().init();

    await useQueueStore.getState().pauseJob('j1');
    await useQueueStore.getState().resumeJob('j1');
    await useQueueStore.getState().cancelJob('j1');
    await useQueueStore.getState().retryJob('j1');
    await useQueueStore.getState().pauseQueue();
    await useQueueStore.getState().resumeQueue();

    const cmds = calls.map((c) => c.cmd);
    expect(cmds).toEqual(
      expect.arrayContaining([
        'pause_job',
        'resume_job',
        'cancel_job',
        'retry_job',
        'pause_queue',
        'resume_queue',
      ]),
    );
    expect(calls.find((c) => c.cmd === 'pause_job')?.args).toEqual({
      id: 'j1',
    });
  });

  it('FR-032: resolves cancelAll with its count', async () => {
    mockQueue();
    await useQueueStore.getState().init();
    await expect(useQueueStore.getState().cancelAll()).resolves.toBe(2);
  });

  it('FR-030: marks status error when the initial snapshot fails', async () => {
    mockQueue({ fail: 'queue_status' });
    await useQueueStore.getState().init();
    const s = useQueueStore.getState();
    expect(s.status).toBe('error');
    expect(s.error).toBe('boom');
  });

  it('NFR-033: init is re-runnable after an error so the queue can recover', async () => {
    mockQueue({ fail: 'queue_status' });
    await useQueueStore.getState().init();
    expect(useQueueStore.getState().status).toBe('error');

    mockQueue();
    await useQueueStore.getState().init();
    expect(useQueueStore.getState().status).toBe('ready');
    expect(useQueueStore.getState().uiJobs).toHaveLength(9);
  });

  it('NFR-033: surfaces an error toast when an action fails', async () => {
    mockQueue({ fail: 'pause_job' });
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();
    await useQueueStore.getState().pauseJob('j1');
    expect(spy).toHaveBeenCalledWith('boom');
  });

  it('NFR-033: a half-failed subscription takes its live listener down with it', async () => {
    // `queueUpdated` registers, `jobProgressEvent` rejects. Leaving the first
    // one alive while reporting `subscribed: false` is what lets a later
    // `init()` register a SECOND one and handle every event twice.
    mockQueue();
    eventMock.failListen.add('jobProgressEvent');
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');

    await useQueueStore.getState().init();

    expect(useQueueStore.getState().subscribed).toBe(false);
    expect(eventMock.queueUpdated.size).toBe(0);
    expect(eventMock.jobProgressEvent.size).toBe(0);
    // The user is told the view is no longer live, not left guessing.
    expect(error).toHaveBeenCalledTimes(1);
    // The one-shot snapshot still loaded.
    expect(useQueueStore.getState().status).toBe('ready');
  });

  it('NFR-033: a retry after a half-failed subscription registers exactly one of each listener', async () => {
    mockQueue({ fail: 'queue_status' });
    eventMock.failListen.add('jobProgressEvent');
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();
    expect(useQueueStore.getState().status).toBe('error');

    eventMock.failListen.clear();
    mockQueue();
    await useQueueStore.getState().init();

    expect(useQueueStore.getState().status).toBe('ready');
    expect(eventMock.queueUpdated.size).toBe(1);
    expect(eventMock.jobProgressEvent.size).toBe(1);
  });

  it('FR-030: reports unavailable outside the Tauri host', async () => {
    // clearMocks() only strips sub-properties of __TAURI_INTERNALS__, so a prior
    // mockIPC leaves the object (and isTauri() true). Drop it for a true no-host.
    const w = window as unknown as {
      __TAURI_INTERNALS__?: unknown;
      __TAURI__?: unknown;
    };
    delete w.__TAURI_INTERNALS__;
    delete w.__TAURI__;

    await useQueueStore.getState().init();
    expect(useQueueStore.getState().status).toBe('unavailable');
  });
});

describe('queueStore — recentDone ordering', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('FR-030: orders sub-second timestamps by instant, not by string compare', async () => {
    // `time`'s RFC 3339 formatter omits zero fractions, so a plain string sort
    // puts "…:00Z" AFTER "…:00.5Z" ('.' < 'Z') and "…:00.55Z" before
    // "…:00.5Z". Both traps in one fixture, newest last on purpose.
    mockQueue({
      snapshot: snapshotOf([
        mkJob({
          id: 'oldest',
          state: 'done',
          file_name: 'a.pdf',
          finished_at: '2026-07-11T10:00:00Z',
        }),
        mkJob({
          id: 'middle',
          state: 'done',
          file_name: 'b.pdf',
          finished_at: '2026-07-11T10:00:00.5Z',
        }),
        mkJob({
          id: 'newest',
          state: 'done',
          file_name: 'c.pdf',
          finished_at: '2026-07-11T10:00:00.55Z',
        }),
      ]),
    });
    await useQueueStore.getState().init();

    expect(useQueueStore.getState().recentDone.map((j) => j.id)).toEqual([
      'newest',
      'middle',
      'oldest',
    ]);
  });

  it('FR-030: falls back to submitted_at when finished_at is missing', async () => {
    mockQueue({
      snapshot: snapshotOf([
        {
          ...mkJob({ id: 'no-stamp', state: 'cancelled', file_name: 'a.pdf' }),
          submitted_at: '2026-07-11T09:00:00Z',
          finished_at: null,
        },
        mkJob({
          id: 'stamped',
          state: 'done',
          file_name: 'b.pdf',
          finished_at: '2026-07-11T10:00:00Z',
        }),
      ]),
    });
    await useQueueStore.getState().init();

    expect(useQueueStore.getState().recentDone.map((j) => j.id)).toEqual([
      'stamped',
      'no-stamp',
    ]);
  });
});

describe('queueStore — batch completion notifications', () => {
  beforeEach(() => resetStore());
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  /** Two jobs of one batch, still running. */
  const running = snapshotOf([
    mkJob({ id: 'a', state: 'printing', file_name: 'a.pdf' }),
    mkJob({ id: 'b', state: 'queued', file_name: 'b.pdf' }),
  ]);

  it('FR-070: announces a batch once every job of it is terminal', async () => {
    mockQueue({ snapshot: running });
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();
    expect(success).not.toHaveBeenCalled();

    emitQueueUpdated(
      snapshotOf([
        mkJob({
          id: 'a',
          state: 'done',
          file_name: 'a.pdf',
          finished_at: '2026-07-11T10:00:00Z',
        }),
        mkJob({
          id: 'b',
          state: 'done',
          file_name: 'b.pdf',
          finished_at: '2026-07-11T10:00:01Z',
        }),
      ]),
    );

    expect(success).toHaveBeenCalledTimes(1);
    expect(success.mock.calls[0]?.[0]).toBe(
      '2 of 2 files printed successfully',
    );
  });

  it('FR-070: a batch is announced once, not on every later snapshot', async () => {
    mockQueue({ snapshot: running });
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();

    const finished = snapshotOf([
      mkJob({ id: 'a', state: 'done', file_name: 'a.pdf' }),
      mkJob({ id: 'b', state: 'cancelled', file_name: 'b.pdf' }),
    ]);
    emitQueueUpdated(finished);
    emitQueueUpdated(finished);

    expect(success).toHaveBeenCalledTimes(1);
    expect(success.mock.calls[0]?.[0]).toBe(
      '1 of 2 files printed successfully',
    );
  });

  it('FR-070: the first snapshot only seeds — old history never re-announces', async () => {
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    mockQueue();
    await useQueueStore.getState().init();
    expect(success).not.toHaveBeenCalled();
  });

  it('FR-070: an event that beats the initial snapshot seeds too, it never replays history', async () => {
    // The listeners are registered before `queue_status` is awaited, so the
    // backend can push a snapshot first. That snapshot is still the first this
    // session sees — everything terminal in it belongs to a past session.
    const finished = snapshotOf([
      mkJob({ id: 'a', state: 'done', file_name: 'a.pdf' }),
      mkJob({ id: 'b', state: 'done', file_name: 'b.pdf' }),
    ]);
    let release!: () => void;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    mockIPC(async (cmd) => {
      if (cmd === 'queue_status') {
        await held;
        return finished;
      }
      return null;
    });
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');

    const initDone = useQueueStore.getState().init();
    await vi.waitFor(() => expect(eventMock.queueUpdated.size).toBe(1));
    emitQueueUpdated(finished);
    release();
    await initDone;

    expect(success).not.toHaveBeenCalled();
    expect(useQueueStore.getState().status).toBe('ready');

    // Seeding is not muting: a batch that goes back to work and finishes
    // *during* this session is still announced.
    emitQueueUpdated(running);
    emitQueueUpdated(finished);
    expect(success).toHaveBeenCalledTimes(1);
    expect(success.mock.calls[0]?.[0]).toBe(
      '2 of 2 files printed successfully',
    );
  });

  it('FR-071: a failed batch reports the error and offers the job log', async () => {
    mockQueue({ snapshot: running });
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();

    emitQueueUpdated(
      snapshotOf([
        mkJob({
          id: 'a',
          state: 'failed',
          file_name: 'a.pdf',
          error: { code: 'print_failed', message: 'Paper jam in Tray 2' },
        }),
        mkJob({ id: 'b', state: 'done', file_name: 'b.pdf' }),
      ]),
    );

    expect(error).toHaveBeenCalledTimes(1);
    const [text, opts] = error.mock.calls[0] ?? [];
    expect(text).toBe('1 of 2 files failed to print');
    const options = opts as { description: string; action: { label: string } };
    // NFR-033: what failed, why, and where to read more.
    expect(options.description).toContain('Paper jam in Tray 2');
    expect(options.description).toContain('a.pdf');
    expect(options.action.label).toBe('Open the logs folder');
  });

  it('FR-070: an all-cancelled batch is reported as cancelled, not as printed', async () => {
    mockQueue({ snapshot: running });
    const plain = vi.spyOn(toast, 'message').mockReturnValue('id');
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();

    emitQueueUpdated(
      snapshotOf([
        mkJob({ id: 'a', state: 'cancelled', file_name: 'a.pdf' }),
        mkJob({ id: 'b', state: 'cancelled', file_name: 'b.pdf' }),
      ]),
    );

    expect(success).not.toHaveBeenCalled();
    expect(plain).toHaveBeenCalledWith('2 jobs cancelled');
  });

  it('FR-070: a batch that goes back to work can announce again', async () => {
    mockQueue({ snapshot: running });
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();

    const finished = snapshotOf([
      mkJob({ id: 'a', state: 'done', file_name: 'a.pdf' }),
      mkJob({ id: 'b', state: 'failed', file_name: 'b.pdf' }),
    ]);
    // failed → the failure toast, not the success one.
    vi.spyOn(toast, 'error').mockReturnValue('id');
    emitQueueUpdated(finished);
    // Retry puts the batch back in flight …
    emitQueueUpdated(running);
    // … and finishing it again is a new completion.
    emitQueueUpdated(
      snapshotOf([
        mkJob({ id: 'a', state: 'done', file_name: 'a.pdf' }),
        mkJob({ id: 'b', state: 'done', file_name: 'b.pdf' }),
      ]),
    );

    expect(success).toHaveBeenCalledTimes(1);
    expect(success.mock.calls[0]?.[0]).toBe(
      '2 of 2 files printed successfully',
    );
  });

  it('FR-070: batches are announced independently', async () => {
    const batch2 = (state: Job['state']) => ({
      ...mkJob({ id: 'c', state, file_name: 'c.pdf' }),
      batch_id: 'batch-2',
    });
    mockQueue({
      snapshot: snapshotOf([...running.jobs, batch2('queued')]),
    });
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();

    // Only the second batch finishes.
    emitQueueUpdated(snapshotOf([...running.jobs, batch2('done')]));

    expect(success).toHaveBeenCalledTimes(1);
    expect(success.mock.calls[0]?.[0]).toBe('1 of 1 file printed successfully');
  });
});

describe('queueStore — clear done with undo (NFR-032)', () => {
  beforeEach(() => {
    resetStore();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    clearMocks();
    vi.restoreAllMocks();
  });

  it('NFR-032: hides the finished jobs at once and clears them 5 s later', async () => {
    const calls = mockQueue();
    await useQueueStore.getState().init();

    expect(useQueueStore.getState().requestClearDone()).toBe(3);
    const hidden = useQueueStore.getState();
    expect(hidden.counts.done).toBe(0);
    expect(hidden.uiJobs).toHaveLength(6);
    expect(hidden.recentDone).toEqual([]);
    expect(calls.some((c) => c.cmd === 'clear_done')).toBe(false);

    await vi.advanceTimersByTimeAsync(CLEAR_UNDO_MS);
    expect(calls.some((c) => c.cmd === 'clear_done')).toBe(true);
    expect(useQueueStore.getState().pendingClear).toEqual([]);
  });

  it('NFR-032: undo restores the jobs and never reaches the backend', async () => {
    const calls = mockQueue();
    await useQueueStore.getState().init();

    useQueueStore.getState().requestClearDone();
    await vi.advanceTimersByTimeAsync(CLEAR_UNDO_MS - 1);
    useQueueStore.getState().undoClearDone();

    expect(useQueueStore.getState().counts.done).toBe(3);
    expect(useQueueStore.getState().uiJobs).toHaveLength(9);

    await vi.advanceTimersByTimeAsync(CLEAR_UNDO_MS);
    expect(calls.some((c) => c.cmd === 'clear_done')).toBe(false);
  });

  it('NFR-032: nothing to clear schedules nothing', async () => {
    const calls = mockQueue({
      snapshot: snapshotOf([
        mkJob({ id: 'a', state: 'printing', file_name: 'a.pdf' }),
      ]),
    });
    await useQueueStore.getState().init();

    expect(useQueueStore.getState().requestClearDone()).toBe(0);
    await vi.advanceTimersByTimeAsync(CLEAR_UNDO_MS);
    expect(calls.some((c) => c.cmd === 'clear_done')).toBe(false);
  });

  it('NFR-033: a failed clear puts the jobs back instead of hiding them forever', async () => {
    mockQueue({ fail: 'clear_done' });
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();

    useQueueStore.getState().requestClearDone();
    await vi.advanceTimersByTimeAsync(CLEAR_UNDO_MS);

    expect(error).toHaveBeenCalledWith('boom');
    expect(useQueueStore.getState().counts.done).toBe(3);
    expect(useQueueStore.getState().pendingClear).toEqual([]);
  });

  it('NFR-032: undo reports whether it really cancelled the pending clear', async () => {
    mockQueue();
    await useQueueStore.getState().init();
    const onCommit = vi.fn();

    useQueueStore.getState().requestClearDone(onCommit);
    expect(useQueueStore.getState().undoClearDone()).toBe(true);
    expect(onCommit).not.toHaveBeenCalled();

    // Past the window: the backend call already went out, so an Undo (from a
    // toast kept open by hovering it) must not claim the jobs were kept.
    useQueueStore.getState().requestClearDone(onCommit);
    await vi.advanceTimersByTimeAsync(CLEAR_UNDO_MS);
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(useQueueStore.getState().undoClearDone()).toBe(false);
  });
});

describe('queueStore — batch totals and row actions', () => {
  beforeEach(() => resetStore());
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  it('FR-070: a batch partly cleared from history is still announced in full', async () => {
    const batch = (a: Job['state'], b: Job['state'], c: Job['state']) =>
      snapshotOf([
        mkJob({ id: 'a', state: a, file_name: 'a.pdf' }),
        mkJob({ id: 'b', state: b, file_name: 'b.pdf' }),
        mkJob({ id: 'c', state: c, file_name: 'c.pdf' }),
      ]);
    mockQueue({ snapshot: batch('queued', 'queued', 'queued') });
    const success = vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();

    emitQueueUpdated(batch('done', 'done', 'printing'));
    // "Clear done" removed a and b from the backend snapshot…
    emitQueueUpdated(
      snapshotOf([mkJob({ id: 'c', state: 'printing', file_name: 'c.pdf' })]),
    );
    // …and then the last one finished.
    emitQueueUpdated(
      snapshotOf([mkJob({ id: 'c', state: 'done', file_name: 'c.pdf' })]),
    );

    expect(success).toHaveBeenCalledTimes(1);
    expect(success.mock.calls[0]?.[0]).toBe(
      '3 of 3 files printed successfully',
    );
  });

  it('NFR-033: a row action is marked in flight until the backend answers', async () => {
    let release!: () => void;
    const held = new Promise<void>((r) => (release = r));
    mockIPC(async (cmd) => {
      if (cmd === 'queue_status') return fixtureQueueSnapshot;
      if (cmd === 'cancel_job') await held;
      return null;
    });
    await useQueueStore.getState().init();

    const first = useQueueStore.getState().cancelJob('j-queued');
    expect(useQueueStore.getState().pendingIds).toContain('j-queued');
    release();
    await first;
    expect(useQueueStore.getState().pendingIds).not.toContain('j-queued');
  });

  it('NFR-033: cancelling a job that already finished is a translated note, not a raw error', async () => {
    const snap = snapshotOf([
      mkJob({ id: 'j-1', state: 'printing', file_name: 'report.pdf' }),
    ]);
    mockIPC((cmd) => {
      if (cmd === 'queue_status') return snap;
      if (cmd === 'cancel_job') {
        // The job reached Done while the confirm was open.
        emitQueueUpdated(
          snapshotOf([
            mkJob({ id: 'j-1', state: 'done', file_name: 'report.pdf' }),
          ]),
        );
        throw {
          code: 'InvalidState',
          message:
            'job 0192f3aa-0000-7000-8000-000000000000 has already finished',
        };
      }
      return null;
    });
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    const info = vi.spyOn(toast, 'info').mockReturnValue('id');
    vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();

    await useQueueStore.getState().cancelJob('j-1');

    expect(error).not.toHaveBeenCalled();
    expect(info).toHaveBeenCalledWith(
      '“report.pdf” had already finished — there was nothing to cancel.',
    );
  });
});

describe('queueStore — jobs that stop and need the user (FR-071)', () => {
  beforeEach(() => resetStore());
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
    setQueueNavigator(null);
  });

  const running = snapshotOf([
    mkJob({ id: 'a', state: 'printing', file_name: 'a.pdf' }),
    mkJob({ id: 'b', state: 'queued', file_name: 'b.pdf' }),
  ]);
  const stuck = (code: string, message: string) =>
    snapshotOf([
      mkJob({
        id: 'a',
        state: 'paused',
        file_name: 'a.pdf',
        error: { code, message },
      }),
      mkJob({ id: 'b', state: 'queued', file_name: 'b.pdf' }),
    ]);

  it('FR-071: a job paused on a user-fixable error is announced with a way to the queue', async () => {
    mockQueue({ snapshot: running });
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    const openQueue = vi.fn();
    setQueueNavigator(openQueue);
    await useQueueStore.getState().init();

    emitQueueUpdated(
      stuck('printer_unavailable', "Printer 'Front Desk' is not available"),
    );

    expect(error).toHaveBeenCalledTimes(1);
    const [text, opts] = error.mock.calls[0] ?? [];
    expect(text).toBe('Printing stopped: a.pdf needs your attention');
    const options = opts as {
      description: string;
      action: { label: string; onClick: () => void };
    };
    expect(options.description).toContain("Printer 'Front Desk'");
    expect(options.action.label).toBe('Open queue');
    options.action.onClick();
    expect(openQueue).toHaveBeenCalledTimes(1);
  });

  it('FR-092: the stop is announced in the UI language, printer by display name', async () => {
    useSettingsStore.setState({
      status: 'ready',
      settings: {
        ...fixtureAppSettings,
        app: { ...fixtureAppSettings.app, locale: 'es-ES' },
      },
      error: undefined,
    });
    usePrinterNamesStore.setState({
      names: { EPSON_L15160: 'EPSON L15160 Series' },
    });
    try {
      mockQueue({ snapshot: running });
      const error = vi.spyOn(toast, 'error').mockReturnValue('id');
      await useQueueStore.getState().init();

      emitQueueUpdated(
        snapshotOf([
          mkJob({
            id: 'a',
            state: 'paused',
            file_name: 'a.pdf',
            error: {
              code: 'printer_disabled',
              message: "printer 'EPSON_L15160' is disabled in Pluryprint",
              params: { printer: 'EPSON_L15160' },
            },
          }),
        ]),
      );

      const [text, opts] = error.mock.calls[0] ?? [];
      expect(text).toBe('La impresión se detuvo: a.pdf necesita tu atención');
      expect((opts as { description: string }).description).toBe(
        'La impresora «EPSON L15160 Series» ya no se gestiona en Pluryprint. Restáurala en la pantalla Impresoras o elige otra impresora — corrígelo y reanuda el trabajo desde la cola.',
      );
    } finally {
      useSettingsStore.setState({ status: 'idle', settings: null });
      resetPrinterNames();
    }
  });

  it('FR-071: the same stop is announced once, and again only after a resume', async () => {
    mockQueue({ snapshot: running });
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();

    const paused = stuck('dependency_missing', 'SumatraPDF was not found');
    emitQueueUpdated(paused);
    emitQueueUpdated(paused);
    expect(error).toHaveBeenCalledTimes(1);

    emitQueueUpdated(running); // resumed
    emitQueueUpdated(paused); // and stopped again
    expect(error).toHaveBeenCalledTimes(2);
  });

  it('FR-071: a manual pause or a shutdown interruption is not an alarm', async () => {
    mockQueue({ snapshot: running });
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();

    emitQueueUpdated(
      snapshotOf([
        mkJob({ id: 'a', state: 'paused', file_name: 'a.pdf' }),
        mkJob({ id: 'b', state: 'queued', file_name: 'b.pdf' }),
      ]),
    );
    emitQueueUpdated(
      stuck('interrupted_by_shutdown', 'Pluryprint closed mid-job'),
    );
    expect(error).not.toHaveBeenCalled();
  });

  it('FR-071: a job already stuck when the session starts only seeds', async () => {
    mockQueue({ snapshot: stuck('printer_unavailable', 'gone') });
    const error = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();
    emitQueueUpdated(stuck('printer_unavailable', 'gone'));
    expect(error).not.toHaveBeenCalled();
  });
});

describe('queueStore — native notifications (FR-072)', () => {
  beforeEach(() => resetStore());
  afterEach(() => {
    clearMocks();
    vi.restoreAllMocks();
  });

  const running = snapshotOf([
    mkJob({ id: 'a', state: 'printing', file_name: 'a.pdf' }),
  ]);

  function nativeCalls(calls: Call[]) {
    return calls
      .filter((c) => c.cmd === 'notify_native')
      .map((c) => c.args as { title: string; body: string });
  }

  it('FR-072: a finished batch also asks for a native notification', async () => {
    const calls = mockQueue({ snapshot: running });
    vi.spyOn(toast, 'success').mockReturnValue('id');
    await useQueueStore.getState().init();

    emitQueueUpdated(
      snapshotOf([mkJob({ id: 'a', state: 'done', file_name: 'a.pdf' })]),
    );

    await vi.waitFor(() => expect(nativeCalls(calls)).toHaveLength(1));
    expect(nativeCalls(calls)[0]?.body).toBe(
      '1 of 1 file printed successfully',
    );
  });

  it('FR-072: a failed batch and a stuck job are native notifications too', async () => {
    const calls = mockQueue({
      snapshot: snapshotOf([
        mkJob({ id: 'a', state: 'printing', file_name: 'a.pdf' }),
        {
          ...mkJob({ id: 'c', state: 'queued', file_name: 'c.pdf' }),
          batch_id: 'b2',
        },
      ]),
    });
    vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();

    emitQueueUpdated(
      snapshotOf([
        mkJob({
          id: 'a',
          state: 'failed',
          file_name: 'a.pdf',
          error: { code: 'print_failed', message: 'Paper jam' },
        }),
        {
          ...mkJob({
            id: 'c',
            state: 'paused',
            file_name: 'c.pdf',
            error: { code: 'printer_unavailable', message: 'Unplugged' },
          }),
          batch_id: 'b2',
        },
      ]),
    );

    await vi.waitFor(() => expect(nativeCalls(calls)).toHaveLength(2));
    const titles = nativeCalls(calls).map((c) => c.title);
    expect(titles).toContain('1 of 1 file failed to print');
    expect(titles).toContain('Printing stopped: c.pdf needs your attention');
  });

  it('FR-072: the first snapshot of a session never notifies', async () => {
    const calls = mockQueue();
    await useQueueStore.getState().init();
    expect(nativeCalls(calls)).toHaveLength(0);
  });
});
