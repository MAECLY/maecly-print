import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import type { Job, QueueSnapshot } from '@/ipc/bindings';
import { fixtureQueueSnapshot, mkJob } from '@/test/fixtures';
import { Toaster } from '@/components/AppShell/Toaster';

// Swap the tauri-specta `events` object for a no-op (the queue store subscribes
// on init); real `commands` stay so mockIPC serves the queue commands.
const eventMock = vi.hoisted(() => {
  const noop = { listen: () => Promise.resolve(() => {}) };
  return { events: { queueUpdated: noop, jobProgressEvent: noop } };
});

vi.mock('@/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/ipc/bindings')>();
  return { ...actual, events: eventMock.events };
});

import { QueueScreen, type QueueView } from './QueueScreen';
import { useQueueStore } from '@/stores/queueStore';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

function mockQueue(snapshot: QueueSnapshot = fixtureQueueSnapshot): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === 'queue_status') return snapshot;
    if (cmd === 'clear_done') return 4;
    if (cmd === 'cancel_all') return 6;
    return null;
  });
  return calls;
}

/** A snapshot of `jobs`, running queue. */
function snapshotOf(jobs: Job[]): QueueSnapshot {
  return { schema_version: 1, queue_paused: false, jobs };
}

function resetStore() {
  // Cancels any undo window a previous test left open (and its timer).
  useQueueStore.getState().undoClearDone();
  useQueueStore.setState({
    status: 'idle',
    snapshot: null,
    subscribed: false,
    pendingClear: [],
    pendingIds: [],
    notifiedBatches: [],
    uiJobs: [],
    counts: { active: 0, pending: 0, done: 0 },
    queuePaused: false,
    recentDone: [],
  });
}

interface RenderOpts {
  snapshot?: QueueSnapshot;
  view?: QueueView;
  setView?: (v: QueueView) => void;
}

/** Init the store (as App would) then render the screen in its ready state. */
async function renderReady(opts: RenderOpts = {}): Promise<Call[]> {
  const calls = mockQueue(opts.snapshot);
  await useQueueStore.getState().init();
  render(
    <>
      <QueueScreen
        view={opts.view ?? 'list'}
        setView={opts.setView ?? (() => {})}
      />
      <Toaster theme="light" />
    </>,
  );
  return calls;
}

describe('QueueScreen', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('FR-031: renders the job groups and count subtitle from the snapshot', async () => {
    await renderReady();

    expect(
      screen.getByText('3 active · 3 pending · 3 done'),
    ).toBeInTheDocument();
    expect(screen.getByText('Active · 3')).toBeInTheDocument();
    expect(screen.getByText('Pending · 3')).toBeInTheDocument();
    expect(screen.getByText('Recent · 3')).toBeInTheDocument();
    expect(screen.getByText('Q3-Financial-Report.pdf')).toBeInTheDocument();
    // The obscure `converted` state is spelled out for the user.
    expect(screen.getByText(/Ready to print/)).toBeInTheDocument();
    expect(
      screen.getByText(/Interrupted by shutdown · resume to continue/),
    ).toBeInTheDocument();
  });

  it('FR-033: toggles the whole queue: pause invokes pause_queue', async () => {
    const calls = await renderReady();

    fireEvent.click(screen.getByRole('button', { name: 'Pause queue' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'pause_queue')).toBe(true),
    );
  });

  it('FR-033: shows Resume queue and invokes resume_queue when paused', async () => {
    const calls = await renderReady({
      snapshot: { ...fixtureQueueSnapshot, queue_paused: true },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Resume queue' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'resume_queue')).toBe(true),
    );
  });

  it('FR-036: clears finished jobs through an in-app confirm dialog', async () => {
    await renderReady();

    // Top-bar action opens the confirm modal (window.confirm is a WKWebview no-op).
    fireEvent.click(screen.getByRole('button', { name: 'Clear done' }));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(/Clear 3 finished jobs\?/),
    ).toBeInTheDocument();
    expect(within(dialog).getByText(/5 seconds to undo/)).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Clear done' }));

    // The jobs leave the screen immediately; the backend call is deferred.
    await waitFor(() =>
      expect(screen.queryByText('Recent · 3')).not.toBeInTheDocument(),
    );
    expect(
      await screen.findByText('Cleared 3 finished jobs'),
    ).toBeInTheDocument();
  });

  it('NFR-032: the clear toast offers a 5 s Undo that puts the jobs back', async () => {
    const calls = await renderReady();

    fireEvent.click(screen.getByRole('button', { name: 'Clear done' }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Clear done' }));

    fireEvent.click(await screen.findByRole('button', { name: 'Undo' }));

    expect(await screen.findByText('Recent · 3')).toBeInTheDocument();
    expect(await screen.findByText('3 finished jobs kept')).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'clear_done')).toBe(false);
  });

  it('FR-032: Cancel all confirms, then cancels every unfinished job', async () => {
    const calls = await renderReady();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel all' }));
    const dialog = await screen.findByRole('dialog');
    // 3 active + 3 pending in the fixture; finished ones are not counted.
    expect(
      within(dialog).getByText(/6 jobs that have not finished/),
    ).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'cancel_all')).toBe(false);

    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel all' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'cancel_all')).toBe(true),
    );
    expect(await screen.findByText('Cancelled 6 jobs')).toBeInTheDocument();
  });

  it('FR-032: Cancel all is disabled when nothing is left to cancel', async () => {
    await renderReady({
      snapshot: snapshotOf([
        mkJob({ id: 'a', state: 'done', file_name: 'a.pdf' }),
      ]),
    });
    expect(screen.getByRole('button', { name: 'Cancel all' })).toBeDisabled();
  });

  it('FR-036: Clear done is disabled when there is no finished job', async () => {
    await renderReady({
      snapshot: snapshotOf([
        mkJob({ id: 'a', state: 'printing', file_name: 'a.pdf' }),
      ]),
    });
    expect(screen.getByRole('button', { name: 'Clear done' })).toBeDisabled();
  });

  it('FR-030: shows an unavailable state outside the Tauri host', () => {
    useQueueStore.setState({ status: 'unavailable' });
    render(<QueueScreen view="list" setView={() => {}} />);
    expect(screen.getByText('Queue unavailable')).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Retry' }),
    ).not.toBeInTheDocument();
  });

  it('NFR-033: the error state explains itself and retries the read', async () => {
    const calls = mockQueue();
    useQueueStore.setState({ status: 'error', error: 'queue.json is locked' });
    render(<QueueScreen view="list" setView={() => {}} />);

    expect(screen.getByText(/queue.json is locked/)).toBeInTheDocument();
    expect(screen.getByText(/Nothing was lost/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'queue_status')).toBe(true),
    );
    await waitFor(() => expect(useQueueStore.getState().status).toBe('ready'));
  });

  it('FR-030: shows the loading state while the first snapshot is in flight', () => {
    useQueueStore.setState({ status: 'loading' });
    render(<QueueScreen view="list" setView={() => {}} />);
    expect(screen.getByText('Loading the queue…')).toBeInTheDocument();
  });

  it('FR-031: shows an empty state when the queue has no job', async () => {
    await renderReady({ snapshot: snapshotOf([]) });
    expect(screen.getByText('No jobs in the queue')).toBeInTheDocument();
  });
});

describe('QueueScreen — row actions', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('FR-032: Pause, Resume and Retry invoke their per-job command', async () => {
    const calls = await renderReady();

    fireEvent.click(
      screen.getAllByRole('button', { name: /^Pause (?!queue)/ })[0]!,
    );
    fireEvent.click(
      screen.getAllByRole('button', { name: /^Resume (?!queue)/ })[0]!,
    );
    fireEvent.click(screen.getByRole('button', { name: /^Retry / }));

    await waitFor(() =>
      expect(calls.filter((c) => c.cmd.endsWith('_job')).length).toBe(3),
    );
    // The first Pause button belongs to the first pausable row (`converting`).
    expect(calls.find((c) => c.cmd === 'pause_job')?.args).toEqual({
      id: 'j-converting',
    });
    expect(calls.find((c) => c.cmd === 'resume_job')?.args).toEqual({
      id: 'j-paused',
    });
    expect(calls.find((c) => c.cmd === 'retry_job')?.args).toEqual({
      id: 'j-failed',
    });
  });

  it('FR-032: only the actions legal for a state are offered', async () => {
    await renderReady();

    // `printing` cannot be paused (the OS owns it) and terminal rows offer
    // nothing but Retry on a failure.
    expect(
      screen.getAllByRole('button', { name: /^Pause (?!queue)/ }),
    ).toHaveLength(3);
    expect(
      screen.getAllByRole('button', { name: /^Resume (?!queue)/ }),
    ).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: /^Retry / })).toHaveLength(1);
    expect(screen.getAllByRole('button', { name: /^Cancel / })).toHaveLength(6);
  });

  it('NFR-044: each row action names the job it acts on', async () => {
    await renderReady({
      snapshot: snapshotOf([
        mkJob({ id: 'a', state: 'queued', file_name: 'a.pdf' }),
        mkJob({ id: 'b', state: 'queued', file_name: 'b.pdf' }),
      ]),
    });
    expect(
      screen.getByRole('button', { name: 'Cancel a.pdf' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Cancel b.pdf' }),
    ).toBeInTheDocument();
  });

  it('FR-032: cancelling a non-printing job goes straight to cancel_job', async () => {
    const calls = await renderReady({
      snapshot: snapshotOf([
        mkJob({ id: 'q', state: 'queued', file_name: 'q.pdf' }),
      ]),
    });

    fireEvent.click(screen.getByRole('button', { name: 'Cancel q.pdf' }));

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === 'cancel_job')?.args).toEqual({
        id: 'q',
      }),
    );
  });

  it('FR-032: cancelling a printing job confirms first', async () => {
    const calls = await renderReady({
      snapshot: snapshotOf([
        mkJob({ id: 'p', state: 'printing', file_name: 'invoice.pdf' }),
      ]),
    });

    fireEvent.click(screen.getByRole('button', { name: 'Cancel invoice.pdf' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/invoice.pdf/)).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'cancel_job')).toBe(false);

    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel job' }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === 'cancel_job')?.args).toEqual({
        id: 'p',
      }),
    );
  });

  it('FR-032: dismissing that confirmation leaves the job alone', async () => {
    const calls = await renderReady({
      snapshot: snapshotOf([
        mkJob({ id: 'p', state: 'printing', file_name: 'invoice.pdf' }),
      ]),
    });

    fireEvent.click(screen.getByRole('button', { name: 'Cancel invoice.pdf' }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));

    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(calls.some((c) => c.cmd === 'cancel_job')).toBe(false);
  });
});

describe('QueueScreen — progress honesty', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('FR-031: a printing job with no reported fraction renders indeterminate', async () => {
    await renderReady({
      snapshot: snapshotOf([
        mkJob({
          id: 'p',
          state: 'printing',
          file_name: 'spooling.pdf',
          progress: { phase: 'printing', percent: null },
        }),
      ]),
    });

    const bar = screen.getByRole('progressbar', {
      name: 'Progress of spooling.pdf',
    });
    // ARIA spells "in progress, amount unknown" as a missing aria-valuenow.
    expect(bar).not.toHaveAttribute('aria-valuenow');
    expect(bar).toHaveAttribute(
      'aria-valuetext',
      'In progress — the print spooler reports no percentage',
    );
  });

  it('FR-031: a measured conversion still renders its percentage', async () => {
    await renderReady({
      snapshot: snapshotOf([
        mkJob({
          id: 'c',
          state: 'converting',
          file_name: 'deck.pptx',
          progress: { phase: 'converting', percent: 0.34 },
        }),
      ]),
    });

    const bar = screen.getByRole('progressbar', {
      name: 'Progress of deck.pptx',
    });
    expect(bar).toHaveAttribute('aria-valuenow', '34');
  });
});

describe('QueueScreen — alternate views', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('FR-031: the view switcher reports the chosen view', async () => {
    const setView = vi.fn();
    await renderReady({ setView });

    fireEvent.click(screen.getByRole('radio', { name: 'Board' }));
    expect(setView).toHaveBeenCalledWith('board');
    fireEvent.click(screen.getByRole('radio', { name: 'Timeline' }));
    expect(setView).toHaveBeenCalledWith('timeline');
  });

  it('FR-031: the Board view lays the jobs out by phase', async () => {
    await renderReady({ view: 'board' });

    expect(
      screen.getByText('Board view — kanban across job phases'),
    ).toBeInTheDocument();
    for (const col of ['Queued', 'Converting', 'Printing', 'Done']) {
      expect(screen.getAllByText(col).length).toBeGreaterThan(0);
    }
    expect(screen.getByText('Q3-Financial-Report.pdf')).toBeInTheDocument();
    // FR-031 / WCAG 1.4.1: mixed columns say each card's state in words, so a
    // failed job is told apart from a printed one without relying on colour.
    expect(screen.getAllByText('Failed').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Paused').length).toBeGreaterThan(0);
    // Board and Timeline are read-only: no per-row controls, and the whole-queue
    // controls belong to the List view.
    expect(
      screen.queryByRole('button', { name: 'Pause queue' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /^Cancel / }),
    ).not.toBeInTheDocument();
  });

  it('FR-031: the Timeline view shows each job in order with its duration', async () => {
    await renderReady({
      view: 'timeline',
      snapshot: snapshotOf([
        mkJob({
          id: 'fast',
          state: 'done',
          file_name: 'memo.pdf',
          started_at: '2026-07-11T10:00:00Z',
          finished_at: '2026-07-11T10:00:07Z',
        }),
        mkJob({
          id: 'slow',
          state: 'done',
          file_name: 'atlas.pdf',
          started_at: '2026-07-11T10:00:00Z',
          finished_at: '2026-07-11T10:03:12Z',
        }),
        mkJob({
          id: 'running',
          state: 'printing',
          file_name: 'now.pdf',
          started_at: '2026-07-11T10:04:00Z',
        }),
      ]),
    });

    expect(
      screen.getByText('Timeline — chronological, with durations'),
    ).toBeInTheDocument();
    expect(screen.getByText(/took 7s/)).toBeInTheDocument();
    expect(screen.getByText(/took 3m 12s/)).toBeInTheDocument();
    // An unfinished job has no duration to show — and none is invented.
    expect(screen.queryByText(/took 0s/)).not.toBeInTheDocument();
    expect(screen.getByText('now.pdf')).toBeInTheDocument();
  });
});
