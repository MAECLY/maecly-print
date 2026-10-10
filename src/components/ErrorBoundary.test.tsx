import type { JSX } from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import { ErrorBoundary } from './ErrorBoundary';

function Boom(): JSX.Element {
  throw new Error('render exploded');
}

describe('ErrorBoundary', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    clearMocks();
    // `clearMocks` only strips the sub-properties of `__TAURI_INTERNALS__`, so
    // the object itself would keep `isTauri()` true for the next test.
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__;
  });

  it('NFR-034: a throwing child renders the panel instead of a blank window', () => {
    // React logs the caught error itself; silence it so the run stays readable.
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(
      screen.getByText('The app hit an unexpected error'),
    ).toBeInTheDocument();
    // The thrown message is surfaced so a report can name it.
    expect(screen.getByText('render exploded')).toBeInTheDocument();
  });

  it('NFR-034: renders children untouched when nothing throws', () => {
    render(
      <ErrorBoundary>
        <span>all good</span>
      </ErrorBoundary>,
    );
    expect(screen.getByText('all good')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('NFR-034: the reload button reloads the app', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const onReload = vi.fn();
    render(
      <ErrorBoundary onReload={onReload}>
        <Boom />
      </ErrorBoundary>,
    );
    fireEvent.click(screen.getByText('Reload the app'));
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it('NFR-034: the logs-folder button is desktop-only', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    // Outside Tauri there is no logs folder to reveal, so the button is hidden.
    expect(screen.queryByText('Open the logs folder')).not.toBeInTheDocument();
  });

  it('NFR-034: on the desktop the logs-folder button is there and opens the folder', async () => {
    // The absence check above passes just as well if the button is deleted;
    // this is the half that notices. Inside the host it must be offered, and
    // clicking it must actually reach the backend.
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return null;
    });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    fireEvent.click(screen.getByText('Open the logs folder'));
    await waitFor(() => expect(calls).toContain('open_logs_folder'));
  });

  it('NFR-034: a logs-folder failure does not replace the panel with another crash', async () => {
    const logged = vi
      .spyOn(console, 'error')
      .mockImplementation(() => undefined);
    mockIPC((cmd) => {
      if (cmd === 'open_logs_folder') throw new Error('no such folder');
      return null;
    });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    fireEvent.click(screen.getByText('Open the logs folder'));
    await waitFor(() =>
      expect(
        logged.mock.calls.some(
          (c) => c[0] === 'could not open the logs folder',
        ),
      ).toBe(true),
    );
    // Still the recovery panel, not a blank window.
    expect(screen.getByRole('alert')).toBeInTheDocument();
  });

  it('NFR-033: a render crash is written to the log file, stack included', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const reports: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === 'log_frontend_error') reports.push(args);
      return null;
    });

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    await waitFor(() => expect(reports).toHaveLength(1));
    const { report } = reports[0] as {
      report: { source: string; message: string; stack: string | null };
    };
    expect(report.source).toBe('error-boundary');
    expect(report.message).toBe('render exploded');
    // The JS stack and React's component stack both reach the log.
    expect(report.stack).toContain('Boom');
  });
});
