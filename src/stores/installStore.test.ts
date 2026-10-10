import { afterEach, describe, expect, it, vi, type Mock } from 'vitest';
import { waitFor } from '@testing-library/react';

import type {
  DependencyInstallState,
  DependencyReport,
  InstallOutcome,
  RunningInstall,
} from '@/ipc/bindings';

import {
  resetInstallStore,
  useInstallStore,
  type InstallBackend,
  type InstallHandlers,
} from './installStore';

function office(found = false): DependencyReport {
  return {
    id: 'libre-office',
    name: 'LibreOffice',
    applicable: true,
    found,
    path: null,
    found_in: null,
    blocks: found ? [] : ['docx'],
    remediation: null,
    install: [],
  };
}

function outcome(
  status: InstallOutcome['status'],
  over: Partial<InstallOutcome> = {},
): InstallOutcome {
  return {
    status,
    exit_code: null,
    elevated: false,
    report: office(status === 'installed'),
    ...over,
  };
}

function running(over: Partial<RunningInstall> = {}): RunningInstall {
  return {
    seq: 7,
    id: 'libre-office',
    name: 'LibreOffice',
    method: 'winget',
    started_at: '2026-10-10T10:00:00Z',
    elevated: true,
    may_continue: false,
    ...over,
  };
}

function handlers(): InstallHandlers & {
  onResolved: Mock<InstallHandlers['onResolved']>;
  announce: Mock<InstallHandlers['announce']>;
} {
  return {
    onResolved: vi.fn<InstallHandlers['onResolved']>(),
    announce: vi.fn<InstallHandlers['announce']>(),
  };
}

/** A backend whose install and slot the test drives. */
function backend(state: { current: DependencyInstallState }) {
  let finish: (o: InstallOutcome) => void = () => {};
  let emit: (id: 'libre-office', line: string) => void = () => {};
  const b: InstallBackend = {
    run: vi.fn(
      () =>
        new Promise<InstallOutcome>((resolve) => {
          finish = resolve;
        }),
    ),
    progress: (onLine) => {
      emit = onLine;
      return Promise.resolve(() => {});
    },
    describe: (e) => String(e),
    status: vi.fn(() => Promise.resolve(state.current)),
    pollMs: 1,
  };
  return {
    b,
    finish: (o: InstallOutcome) => finish(o),
    emit: (line: string) => emit('libre-office', line),
  };
}

const idle: DependencyInstallState = { running: null, last: null };

afterEach(() => {
  resetInstallStore();
});

describe('installStore', () => {
  /**
   * Round-6 finding: the Queue started the install, Print Job's dialog was
   * opened on it and closed, and only Print Job heard the end — so the
   * Queue's paused jobs were never resumed.
   */
  it('FR-058: every screen that asked hears the end, not only the last dialog', async () => {
    const { b, finish } = backend({ current: idle });
    const queue = handlers();
    const printJob = handlers();
    const store = useInstallStore.getState();
    expect(store.start(office(), 'winget', b, queue)).toBe(true);

    // Print Job's dialog opens on the running install, then closes.
    const unwatch = useInstallStore.getState().watch('libre-office', printJob);
    unwatch();

    const fresh = office(true);
    finish(outcome('installed', { report: fresh }));
    await waitFor(() => expect(queue.onResolved).toHaveBeenCalledWith(fresh));
    expect(printJob.onResolved).toHaveBeenCalledWith(fresh);
    expect(queue.onResolved).toHaveBeenCalledTimes(1);
    // Announced once, nobody watching.
    expect(
      queue.announce.mock.calls.length + printJob.announce.mock.calls.length,
    ).toBe(1);
  });

  it('FR-058: a dialog open at the end shows it, so nothing is announced', async () => {
    const { b, finish } = backend({ current: idle });
    const queue = handlers();
    const dialog = handlers();
    useInstallStore.getState().start(office(), 'winget', b, queue);
    useInstallStore.getState().watch('libre-office', dialog);
    finish(outcome('installed'));
    await waitFor(() => expect(useInstallStore.getState().active).toBeNull());
    expect(queue.onResolved).toHaveBeenCalledTimes(1);
    expect(dialog.onResolved).toHaveBeenCalledTimes(1);
    expect(queue.announce).not.toHaveBeenCalled();
    expect(dialog.announce).not.toHaveBeenCalled();
  });

  it('FR-058: after an end the installer may outlive, no install starts until the backend frees the slot', async () => {
    const state = {
      current: {
        running: running({ may_continue: true }),
        last: null,
      } as DependencyInstallState,
    };
    const { b, finish } = backend(state);
    useInstallStore.getState().start(office(), 'winget', b, handlers());
    finish(
      outcome('cancel-requested-installer-may-continue', { elevated: true }),
    );
    await waitFor(() =>
      expect(useInstallStore.getState().finishing).toEqual({
        id: 'libre-office',
        name: 'LibreOffice',
      }),
    );
    expect(
      useInstallStore.getState().start(office(), 'winget', b, handlers()),
    ).toBe(false);

    state.current = idle;
    await waitFor(() =>
      expect(useInstallStore.getState().finishing).toBeNull(),
    );
    expect(
      useInstallStore.getState().start(office(), 'winget', b, handlers()),
    ).toBe(true);
  });

  it('FR-058: a timeout the installer may outlive holds the slot too', async () => {
    const state = {
      current: {
        running: running({ may_continue: true }),
        last: null,
      } as DependencyInstallState,
    };
    const { b, finish } = backend(state);
    useInstallStore.getState().start(office(), 'winget', b, handlers());
    finish(outcome('timed-out-installer-may-continue'));
    await waitFor(() =>
      expect(useInstallStore.getState().finishing).not.toBeNull(),
    );
    state.current = idle;
    await waitFor(() =>
      expect(useInstallStore.getState().finishing).toBeNull(),
    );
  });

  /**
   * Round-6 finding: after a reload the window forgot the running install
   * and offered an Install the backend refused.
   */
  it('FR-058: hydrate picks up an install that runs in the backend and follows it to its end', async () => {
    const state = {
      current: { running: running(), last: null } as DependencyInstallState,
    };
    const { b, emit } = backend(state);
    const announce = vi.fn();
    const dialog = handlers();
    useInstallStore.getState().watch('libre-office', dialog);

    await useInstallStore
      .getState()
      .hydrate(b, { announce, unknownEnd: (n) => `unknown ${n}` });
    expect(useInstallStore.getState().active).toEqual({
      id: 'libre-office',
      name: 'LibreOffice',
      method: 'winget',
    });
    emit('Downloading 40%');
    expect(useInstallStore.getState().lines).toEqual(['Downloading 40%']);
    // A second Install is not offered meanwhile.
    expect(
      useInstallStore.getState().start(office(), 'winget', b, handlers()),
    ).toBe(false);

    const fresh = office(true);
    state.current = {
      running: null,
      last: {
        seq: 7,
        id: 'libre-office',
        outcome: outcome('installed', { report: fresh }),
      },
    };
    await waitFor(() => expect(useInstallStore.getState().active).toBeNull());
    expect(useInstallStore.getState().last?.outcome?.status).toBe('installed');
    expect(dialog.onResolved).toHaveBeenCalledWith(fresh);
    // The dialog showed it.
    expect(announce).not.toHaveBeenCalled();
  });

  it('FR-058: an install picked up by hydrate is announced when no dialog is open, honestly when its end is unknown', async () => {
    const state = {
      current: { running: running(), last: null } as DependencyInstallState,
    };
    const { b } = backend(state);
    const announce = vi.fn();
    await useInstallStore
      .getState()
      .hydrate(b, { announce, unknownEnd: (n) => `unknown ${n}` });
    state.current = idle;
    await waitFor(() => expect(announce).toHaveBeenCalledTimes(1));
    expect(announce.mock.calls[0]![0]).toMatchObject({
      id: 'libre-office',
      failure: 'unknown LibreOffice',
    });
  });

  /**
   * Round-7 finding: after a reload no Queue handler is subscribed, so the
   * jobs paused on the dependency stayed paused while the toast said the
   * files could print now.
   */
  it('FR-058: an install picked up by hydrate resumes the waiting queue jobs itself, and the announcement says so', async () => {
    const state = {
      current: { running: running(), last: null } as DependencyInstallState,
    };
    const { b } = backend(state);
    const tally = { resumed: 2, failed: 0, checked: true };
    b.resumeWaiting = vi.fn(() => Promise.resolve(tally));
    const announce = vi.fn();
    await useInstallStore
      .getState()
      .hydrate(b, { announce, unknownEnd: (n) => `unknown ${n}` });
    state.current = {
      running: null,
      last: {
        seq: 7,
        id: 'libre-office',
        outcome: outcome('installed', { report: office(true) }),
      },
    };
    await waitFor(() => expect(announce).toHaveBeenCalledTimes(1));
    expect(b.resumeWaiting).toHaveBeenCalledWith('libre-office');
    expect(announce.mock.calls[0]![0]).toMatchObject({
      id: 'libre-office',
      outcome: { status: 'installed' },
      resumed: tally,
    });
  });

  it('FR-058: nothing is resumed when the dependency is still not found', async () => {
    const state = {
      current: { running: running(), last: null } as DependencyInstallState,
    };
    const { b } = backend(state);
    b.resumeWaiting = vi.fn();
    const announce = vi.fn();
    await useInstallStore
      .getState()
      .hydrate(b, { announce, unknownEnd: (n) => n });
    state.current = {
      running: null,
      last: { seq: 7, id: 'libre-office', outcome: outcome('not-detected') },
    };
    await waitFor(() => expect(announce).toHaveBeenCalledTimes(1));
    expect(b.resumeWaiting).not.toHaveBeenCalled();
    expect(announce.mock.calls[0]![0].resumed).toBeUndefined();
  });

  it('FR-058: an end nobody could report is probed again before anything resumes', async () => {
    const state = {
      current: { running: running(), last: null } as DependencyInstallState,
    };
    const { b } = backend(state);
    b.probe = vi.fn(() => Promise.resolve(office(true)));
    b.resumeWaiting = vi.fn(() =>
      Promise.resolve({ resumed: 1, failed: 0, checked: true }),
    );
    const announce = vi.fn();
    await useInstallStore
      .getState()
      .hydrate(b, { announce, unknownEnd: (n) => `unknown ${n}` });
    state.current = idle;
    await waitFor(() => expect(announce).toHaveBeenCalledTimes(1));
    expect(b.probe).toHaveBeenCalledWith('libre-office');
    expect(announce.mock.calls[0]![0]).toMatchObject({
      failure: 'unknown LibreOffice',
      resumed: { resumed: 1 },
    });
  });

  it('FR-058: a Queue dialog that resumes the jobs itself is left to do it', async () => {
    const { b, finish } = backend({ current: idle });
    b.resumeWaiting = vi.fn();
    const queue = { ...handlers(), resumesJobs: true };
    useInstallStore.getState().start(office(), 'winget', b, queue);
    finish(outcome('installed'));
    await waitFor(() => expect(queue.onResolved).toHaveBeenCalledTimes(1));
    expect(b.resumeWaiting).not.toHaveBeenCalled();
  });

  it('FR-058: hydrate finds an installer still finishing and holds Install back', async () => {
    const state = {
      current: {
        running: running({ may_continue: true }),
        last: null,
      } as DependencyInstallState,
    };
    const { b } = backend(state);
    await useInstallStore
      .getState()
      .hydrate(b, { announce: vi.fn(), unknownEnd: (n) => n });
    expect(useInstallStore.getState().active).toBeNull();
    expect(useInstallStore.getState().finishing?.id).toBe('libre-office');
    state.current = idle;
    await waitFor(() =>
      expect(useInstallStore.getState().finishing).toBeNull(),
    );
  });

  it('FR-058: hydrate with nothing running changes nothing', async () => {
    const { b } = backend({ current: idle });
    await useInstallStore
      .getState()
      .hydrate(b, { announce: vi.fn(), unknownEnd: (n) => n });
    expect(useInstallStore.getState().active).toBeNull();
    expect(useInstallStore.getState().finishing).toBeNull();
  });
});
