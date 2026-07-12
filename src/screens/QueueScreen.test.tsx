import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import type { QueueSnapshot } from '@/ipc/bindings';
import { fixtureQueueSnapshot } from '@/test/fixtures';
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

import { QueueScreen } from './QueueScreen';
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
    return null;
  });
  return calls;
}

function resetStore() {
  useQueueStore.setState({
    status: 'idle',
    snapshot: null,
    uiJobs: [],
    counts: { active: 0, pending: 0, done: 0 },
    queuePaused: false,
    recentDone: [],
  });
}

/** Init the store (as App would) then render the screen in its ready state. */
async function renderReady(snapshot?: QueueSnapshot): Promise<Call[]> {
  const calls = mockQueue(snapshot);
  await useQueueStore.getState().init();
  render(
    <>
      <QueueScreen view="list" setView={() => {}} />
      <Toaster theme="light" />
    </>,
  );
  return calls;
}

describe('QueueScreen', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('renders the job groups and count subtitle from the snapshot', async () => {
    await renderReady();

    expect(
      screen.getByText('3 active · 3 pending · 3 done'),
    ).toBeInTheDocument();
    expect(screen.getByText('Active · 3')).toBeInTheDocument();
    expect(screen.getByText('Pending · 3')).toBeInTheDocument();
    expect(screen.getByText('Recent · 3')).toBeInTheDocument();
    expect(screen.getByText('Q3-Financial-Report.pdf')).toBeInTheDocument();
  });

  it('toggles the whole queue: pause invokes pause_queue', async () => {
    const calls = await renderReady();

    fireEvent.click(screen.getByRole('button', { name: 'Pause queue' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'pause_queue')).toBe(true),
    );
  });

  it('shows Resume queue and invokes resume_queue when paused', async () => {
    const calls = await renderReady({
      ...fixtureQueueSnapshot,
      queue_paused: true,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Resume queue' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'resume_queue')).toBe(true),
    );
  });

  it('clears finished jobs through an in-app confirm dialog', async () => {
    const calls = await renderReady();

    // Top-bar action opens the confirm modal (window.confirm is a WKWebview no-op).
    fireEvent.click(screen.getByRole('button', { name: 'Clear done' }));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(/Clear 3 finished jobs\?/),
    ).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole('button', { name: 'Clear done' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'clear_done')).toBe(true),
    );
    expect(
      await screen.findByText(/Cleared 4 finished jobs/),
    ).toBeInTheDocument();
  });

  it('shows an unavailable state outside the Tauri host', () => {
    useQueueStore.setState({ status: 'unavailable' });
    render(<QueueScreen view="list" setView={() => {}} />);
    expect(screen.getByText('Queue unavailable')).toBeInTheDocument();
  });
});
