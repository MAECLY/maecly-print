import { afterEach, describe, expect, it, vi } from 'vitest';
import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';

import { installGlobalErrorLogging, logError } from './errorLog';

type Report = { source: string; message: string; stack: string | null };

function captureReports(): Report[] {
  const reports: Report[] = [];
  mockIPC((cmd, args) => {
    if (cmd === 'log_frontend_error') {
      reports.push((args as { report: Report }).report);
    }
    return null;
  });
  return reports;
}

describe('errorLog', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    clearMocks();
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__;
  });

  it('NFR-033: forwards a caught error to the backend log', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const reports = captureReports();
    logError('app.drag-drop-listener', new Error('no webview'));
    await vi.waitFor(() => expect(reports).toHaveLength(1));
    expect(reports[0]).toMatchObject({
      source: 'app.drag-drop-listener',
      message: 'no webview',
    });
  });

  it('NFR-033: non-Error values are logged by value', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const reports = captureReports();
    logError('x', { code: 'PrintFailed' });
    logError('y', 'plain string');
    await vi.waitFor(() => expect(reports).toHaveLength(2));
    expect(reports[0]?.message).toBe('{"code":"PrintFailed"}');
    expect(reports[1]?.message).toBe('plain string');
  });

  it('NFR-033: a failing bridge never throws into the caller', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    mockIPC(() => {
      throw new Error('bridge down');
    });
    expect(() => logError('x', new Error('boom'))).not.toThrow();
    // Let the rejected invoke settle: an unhandled rejection would fail the run.
    await new Promise((r) => setTimeout(r, 0));
  });

  it('NFR-033: outside the desktop host only the console sees it', () => {
    const consoleSpy = vi
      .spyOn(console, 'error')
      .mockImplementation(() => undefined);
    logError('x', new Error('boom'));
    expect(consoleSpy).toHaveBeenCalled();
  });

  it('NFR-033: uncaught errors and unhandled rejections reach the log', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const reports = captureReports();
    const uninstall = installGlobalErrorLogging(window);
    try {
      window.dispatchEvent(
        new ErrorEvent('error', { error: new Error('uncaught'), message: 'x' }),
      );
      const rejection = new Event('unhandledrejection') as Event & {
        reason?: unknown;
      };
      rejection.reason = new Error('rejected');
      window.dispatchEvent(rejection);
      await vi.waitFor(() => expect(reports).toHaveLength(2));
      expect(reports.map((r) => r.source)).toEqual([
        'window.error',
        'unhandledrejection',
      ]);
      expect(reports.map((r) => r.message)).toEqual(['uncaught', 'rejected']);
    } finally {
      uninstall();
    }
  });
});
