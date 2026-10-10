import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';

import type { Job, JobError, QueueSnapshot } from '@/ipc/bindings';
import { mkJob } from '@/test/fixtures';
import { needsPassword } from '@/lib/pdfPassword';

// Capture the queue's event listener so a test can push snapshots the way
// the backend does after `job_supply_password`.
const bus = vi.hoisted(() => {
  const state: { push: ((s: unknown) => void) | null } = { push: null };
  return {
    state,
    events: {
      queueUpdated: {
        listen: (cb: (ev: { payload: unknown }) => void) => {
          state.push = (payload) => cb({ payload });
          return Promise.resolve(() => {});
        },
      },
      jobProgressEvent: { listen: () => Promise.resolve(() => {}) },
    },
  };
});

vi.mock('@/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/ipc/bindings')>();
  return { ...actual, events: bus.events };
});

import { QueueScreen } from '@/screens/QueueScreen';
import { useQueueStore } from '@/stores/queueStore';

const required: JobError = {
  code: 'pdf_protected',
  message: 'the PDF is password-protected',
  reason: 'password_required',
  params: null,
};
const incorrect: JobError = { ...required, reason: 'password_incorrect' };

function snapshotOf(jobs: Job[], queuePaused = false): QueueSnapshot {
  return { schema_version: 1, queue_paused: queuePaused, jobs };
}

function locked(id: string, error: JobError = required): Job {
  return mkJob({ id, state: 'paused', file_name: `${id}.pdf`, error });
}

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

async function renderQueue(
  jobs: Job[],
  view: 'list' | 'board' | 'timeline' = 'list',
  queuePaused = false,
): Promise<Call[]> {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === 'queue_status') return snapshotOf(jobs, queuePaused);
    return null;
  });
  await useQueueStore.getState().init();
  render(<QueueScreen view={view} setView={() => {}} />);
  return calls;
}

function push(jobs: Job[], queuePaused = false) {
  act(() => bus.state.push?.(snapshotOf(jobs, queuePaused)));
}

async function submitPassword(calls: Call[], pw: string) {
  fireEvent.change(screen.getByLabelText('PDF password'), {
    target: { value: pw },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Unlock and print' }));
  await waitFor(() =>
    expect(calls.some((c) => c.cmd === 'job_supply_password')).toBe(true),
  );
}

function resetStore() {
  useQueueStore.setState({
    status: 'idle',
    snapshot: null,
    subscribed: false,
    pendingClear: [],
    pendingIds: [],
    notifiedBatches: [],
    notifiedPaused: [],
    passwordPrompt: null,
    uiJobs: [],
    counts: { active: 0, pending: 0, done: 0 },
    queuePaused: false,
    recentDone: [],
  });
}

describe('PdfPasswordDialog (FR-059)', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('mirrors Job::needs_password', () => {
    expect(needsPassword('paused', required)).toBe(true);
    expect(needsPassword('paused', incorrect)).toBe(true);
    expect(needsPassword('failed', required)).toBe(false);
    expect(
      needsPassword('paused', { ...required, reason: 'printing_not_allowed' }),
    ).toBe(false);
    expect(needsPassword('paused', null)).toBe(false);
  });

  it('offers "Enter password" instead of Resume on a locked job only', async () => {
    await renderQueue([
      locked('secret'),
      mkJob({ id: 'plain', state: 'paused', file_name: 'plain.pdf' }),
    ]);
    expect(
      screen.getByRole('button', { name: 'Enter the password of secret.pdf' }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Resume secret.pdf' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Resume plain.pdf' }),
    ).toBeInTheDocument();
  });

  it('sends the password, clears the field, asks again on a wrong one, closes once it converts', async () => {
    const calls = await renderQueue([locked('secret')]);
    fireEvent.click(
      screen.getByRole('button', { name: 'Enter the password of secret.pdf' }),
    );
    const dialog = screen.getByRole('dialog', {
      name: 'This PDF needs a password',
    });
    const field = screen.getByLabelText('PDF password');
    expect(field).toHaveAttribute('type', 'password');
    expect(field).toHaveAttribute('autocomplete', 'off');
    expect(dialog).toHaveTextContent('“secret.pdf” is protected');

    // Nothing typed: refused locally, nothing sent.
    fireEvent.click(screen.getByRole('button', { name: 'Unlock and print' }));
    expect(screen.getByRole('alert')).toHaveTextContent(
      'Enter the password first.',
    );
    expect(calls.some((c) => c.cmd === 'job_supply_password')).toBe(false);

    // A wrong password: the backend resumes the job, then pauses it again.
    fireEvent.change(field, { target: { value: 'wrong' } });
    fireEvent.click(screen.getByRole('button', { name: 'Unlock and print' }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === 'job_supply_password')?.args).toEqual({
        id: 'secret',
        password: 'wrong',
        applyToBatch: false,
      }),
    );
    expect(field).toHaveValue('');
    push([mkJob({ id: 'secret', state: 'queued', file_name: 'secret.pdf' })]);
    push([locked('secret', incorrect)]);
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent(
        'That password does not open this PDF',
      ),
    );
    expect(screen.getByRole('dialog')).toBeInTheDocument();

    // The right one: the job converts and the dialog goes away.
    fireEvent.change(field, { target: { value: 's3cret' } });
    fireEvent.click(screen.getByRole('button', { name: 'Unlock and print' }));
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === 'job_supply_password')).toHaveLength(
        2,
      ),
    );
    push([
      mkJob({ id: 'secret', state: 'converting', file_name: 'secret.pdf' }),
    ]);
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    push([
      mkJob({ id: 'secret', state: 'converted', file_name: 'secret.pdf' }),
    ]);
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
  });

  it('can apply the password to the batch, or cancel the job', async () => {
    const calls = await renderQueue([locked('a'), locked('b'), locked('c')]);
    fireEvent.click(
      screen.getByRole('button', { name: 'Enter the password of a.pdf' }),
    );
    fireEvent.click(
      screen.getByRole('checkbox', {
        name: 'Also try it on the other 2 protected PDFs of this batch',
      }),
    );
    fireEvent.change(screen.getByLabelText('PDF password'), {
      target: { value: 'pw' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Unlock and print' }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === 'job_supply_password')?.args).toEqual({
        id: 'a',
        password: 'pw',
        applyToBatch: true,
      }),
    );

    // `a` got through; the prompt for it closes by itself.
    push([
      mkJob({ id: 'a', state: 'converted', file_name: 'a.pdf' }),
      locked('b'),
      locked('c'),
    ]);
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    fireEvent.click(
      screen.getByRole('button', { name: 'Enter the password of b.pdf' }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cancel this job' }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === 'cancel_job')?.args).toEqual({
        id: 'b',
      }),
    );
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('"Not now" closes the prompt and leaves the job waiting', async () => {
    const calls = await renderQueue([locked('secret')]);
    fireEvent.click(
      screen.getByRole('button', { name: 'Enter the password of secret.pdf' }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Not now' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(
      calls.some((c) =>
        ['job_supply_password', 'cancel_job', 'resume_job'].includes(c.cmd),
      ),
    ).toBe(false);
  });

  it('FR-059: a password accepted while the queue is paused says so and offers to resume, instead of "Checking…" forever', async () => {
    const calls = await renderQueue([locked('secret')], 'list', true);
    fireEvent.click(
      screen.getByRole('button', { name: 'Enter the password of secret.pdf' }),
    );
    await submitPassword(calls, 'pw');
    push(
      [mkJob({ id: 'secret', state: 'queued', file_name: 'secret.pdf' })],
      true,
    );
    const dialog = await screen.findByRole('dialog');
    await waitFor(() =>
      expect(dialog).toHaveTextContent('the queue is paused'),
    );
    expect(dialog).not.toHaveTextContent('Checking the password');
    // Nothing is in flight: the user can leave, cancel, or resume the queue.
    expect(
      screen.getByRole('button', { name: 'Cancel this job' }),
    ).toBeEnabled();
    const resume = within(dialog).getByRole('button', {
      name: 'Resume queue',
    });
    fireEvent.click(resume);
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'resume_queue')).toBe(true),
    );
    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('FR-059: a password waiting for a free slot is not stuck on "Checking…", and a wrong one is still reported', async () => {
    const calls = await renderQueue([locked('secret')]);
    fireEvent.click(
      screen.getByRole('button', { name: 'Enter the password of secret.pdf' }),
    );
    await submitPassword(calls, 'wrong');
    push([mkJob({ id: 'secret', state: 'queued', file_name: 'secret.pdf' })]);
    const dialog = screen.getByRole('dialog');
    await waitFor(() =>
      expect(dialog).toHaveTextContent('waiting its turn in the queue'),
    );
    expect(
      within(dialog).queryByRole('button', { name: 'Resume queue' }),
    ).not.toBeInTheDocument();
    // Its turn comes: it is tried, and the password was wrong.
    push([
      mkJob({ id: 'secret', state: 'converting', file_name: 'secret.pdf' }),
    ]);
    await waitFor(() =>
      expect(dialog).toHaveTextContent('Checking the password'),
    );
    push([locked('secret', incorrect)]);
    await waitFor(() =>
      expect(screen.getByRole('alert')).toHaveTextContent(
        'That password does not open this PDF',
      ),
    );
  });

  it.each(['board', 'timeline'] as const)(
    'FR-059: the password prompt is reachable from the %s view too',
    async (view) => {
      await renderQueue([locked('secret')], view);
      fireEvent.click(
        screen.getByRole('button', {
          name: 'Enter the password of secret.pdf',
        }),
      );
      expect(
        screen.getByRole('dialog', { name: 'This PDF needs a password' }),
      ).toBeInTheDocument();
    },
  );
});
