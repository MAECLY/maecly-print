import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { toast } from 'sonner';

import type { JobProgressEvent, QueueSnapshot } from '@/ipc/bindings';
import { fixtureQueueSnapshot, mkJob } from '@/test/fixtures';

// The tauri-specta `events` object drives the event plugin over IPC, which
// mockIPC would also intercept. Swapping just `events` for a controllable fake
// (while keeping the real `commands`, so mockIPC still serves command invokes)
// is the cleanest reliable way to emit events on demand. `vi.hoisted` makes the
// registry available inside the hoisted `vi.mock` factory.
type Listener = (e: { payload: unknown }) => void;
const eventMock = vi.hoisted(() => {
  const queueUpdated = new Set<(e: { payload: unknown }) => void>();
  const jobProgressEvent = new Set<(e: { payload: unknown }) => void>();
  return {
    queueUpdated,
    jobProgressEvent,
    events: {
      queueUpdated: {
        listen: (cb: (e: { payload: unknown }) => void) => {
          queueUpdated.add(cb);
          return Promise.resolve(() => queueUpdated.delete(cb));
        },
      },
      jobProgressEvent: {
        listen: (cb: (e: { payload: unknown }) => void) => {
          jobProgressEvent.add(cb);
          return Promise.resolve(() => jobProgressEvent.delete(cb));
        },
      },
    },
  };
});

vi.mock('@/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/ipc/bindings')>();
  return { ...actual, events: eventMock.events };
});

// Imported after the mock is registered (vi.mock is hoisted regardless).
import { useQueueStore } from '@/stores/queueStore';

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

function resetStore() {
  eventMock.queueUpdated.clear();
  eventMock.jobProgressEvent.clear();
  useQueueStore.setState({
    status: 'idle',
    snapshot: null,
    subscribed: false,
    uiJobs: [],
    counts: { active: 0, pending: 0, done: 0 },
    queuePaused: false,
    recentDone: [],
  });
}

describe('queueStore', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('loads the initial snapshot and derives counts on init', async () => {
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

  it('is idempotent — a second init does not re-fetch', async () => {
    const calls = mockQueue();
    await useQueueStore.getState().init();
    await useQueueStore.getState().init();
    expect(calls.filter((c) => c.cmd === 'queue_status')).toHaveLength(1);
  });

  it('replaces the snapshot wholesale on queueUpdated', async () => {
    mockQueue();
    await useQueueStore.getState().init();

    const next: QueueSnapshot = {
      schema_version: 1,
      queue_paused: true,
      jobs: [
        mkJob({
          id: 'x',
          state: 'queued',
          file_name: 'only.pdf',
          started_at: null,
        }),
      ],
    };
    emitQueueUpdated(next);

    const s = useQueueStore.getState();
    expect(s.snapshot).toEqual(next);
    expect(s.queuePaused).toBe(true);
    expect(s.uiJobs).toHaveLength(1);
    expect(s.counts).toEqual({ active: 0, pending: 1, done: 0 });
  });

  it('merges job progress by id and ignores unknown ids', async () => {
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

  it('invokes the matching command for each job action', async () => {
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

  it('resolves clearDone and cancelAll with their counts', async () => {
    mockQueue();
    await useQueueStore.getState().init();
    await expect(useQueueStore.getState().clearDone()).resolves.toBe(4);
    await expect(useQueueStore.getState().cancelAll()).resolves.toBe(2);
  });

  it('marks status error when the initial snapshot fails', async () => {
    mockQueue({ fail: 'queue_status' });
    await useQueueStore.getState().init();
    const s = useQueueStore.getState();
    expect(s.status).toBe('error');
    expect(s.error).toBe('boom');
  });

  it('surfaces an error toast when an action fails', async () => {
    mockQueue({ fail: 'pause_job' });
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useQueueStore.getState().init();
    await useQueueStore.getState().pauseJob('j1');
    expect(spy).toHaveBeenCalledWith('boom');
    spy.mockRestore();
  });

  it('reports unavailable outside the Tauri host', async () => {
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
