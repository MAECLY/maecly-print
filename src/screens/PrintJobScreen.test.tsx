import { useState } from 'react';
import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
  act,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import type { FileInfo, PrintRequest } from '@/lib/ipc';
import type { UiFile } from '@/types/domain';
import { type PrintJobOverrides } from '@/lib/profiles';
import { Toaster } from '@/components/AppShell/Toaster';

// The OS file picker and native drag-drop are module-level Tauri APIs; mock
// them so tests can drive selections/drops deterministically.
const openMock = vi.hoisted(() => vi.fn());
const dnd = vi.hoisted(() => ({
  handler: null as
    | ((e: { payload: { type: string; paths?: string[] } }) => void)
    | null,
}));
// Stub the tauri-specta `events` object (the queue store subscribes on App
// init); real `commands` stay so mockIPC serves the print/inspect commands.
const eventMock = vi.hoisted(() => {
  const noop = { listen: () => Promise.resolve(() => {}) };
  return { events: { queueUpdated: noop, jobProgressEvent: noop } };
});

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openMock }));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (
      h: (e: { payload: { type: string; paths?: string[] } }) => void,
    ) => {
      dnd.handler = h;
      return Promise.resolve(() => {});
    },
  }),
}));
vi.mock('@/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/ipc/bindings')>();
  return { ...actual, events: eventMock.events };
});

import { PrintJobScreen } from './PrintJobScreen';
import App from '../App';
import { useQueueStore } from '@/stores/queueStore';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

interface MockOpts {
  inspect?: (paths: string[]) => FileInfo[];
  preview?: string;
  printError?: string;
  jobIds?: string[];
}

/** Install IPC mocks for the print-dialog commands; returns invoked calls. */
function mockJob(opts: MockOpts = {}): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    switch (cmd) {
      case 'inspect_files':
        return opts.inspect?.((args as { paths: string[] }).paths) ?? [];
      case 'preview_command':
        return opts.preview ?? 'lp -d Printer file.pdf';
      case 'print_files':
        if (opts.printError) throw new Error(opts.printError);
        return { batch_id: 'b1', job_ids: opts.jobIds ?? ['j1', 'j2'] };
      case 'queue_status':
        return { schema_version: 1, queue_paused: false, jobs: [] };
      case 'list_printers':
        return [];
      case 'default_printer':
        return null;
      case 'get_printer_prefs':
        return {};
      default:
        return null;
    }
  });
  return calls;
}

/** A printable `FileInfo`, extension inferred from the path unless overridden. */
function fileInfo(path: string, over: Partial<FileInfo> = {}): FileInfo {
  const name = path.split('/').pop() ?? path;
  const ext = name.includes('.') ? (name.split('.').pop() ?? '') : '';
  return {
    path,
    name,
    ext: ext.toLowerCase(),
    bytes: 2048,
    pages: 3,
    supported: true,
    reason: null,
    ...over,
  };
}

/** A `UiFile` already in the list (id/path keyed off the path). */
function uiFile(path: string): UiFile {
  const name = path.split('/').pop() ?? path;
  const ext = name.split('.').pop() ?? '';
  return {
    id: path,
    name: name.replace(/\.[^.]+$/, ''),
    ext,
    kb: 2,
    pages: 3,
    path,
  };
}

/** Stateful wrapper so `setFiles`/`setOverrides` behave like the real App. */
function Harness({ initial = [] }: { initial?: UiFile[] }) {
  const [files, setFiles] = useState<UiFile[]>(initial);
  const [profile, setProfile] = useState('p1');
  const [overrides, setOverrides] = useState<PrintJobOverrides>({});
  return (
    <>
      <PrintJobScreen
        files={files}
        setFiles={setFiles}
        profile={profile}
        setProfile={setProfile}
        overrides={overrides}
        setOverrides={setOverrides}
        onPrint={() => {}}
      />
      <Toaster theme="light" />
    </>
  );
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

describe('PrintJobScreen', () => {
  beforeEach(() => {
    resetStore();
    openMock.mockReset();
    dnd.handler = null;
    window.localStorage.setItem('maeclyprint:onboarding-complete', '1');
  });
  afterEach(() => clearMocks());

  it('adds picked files and skips unsupported ones with a toast', async () => {
    const calls = mockJob({
      inspect: () => [
        fileInfo('/docs/a.pdf'),
        fileInfo('/docs/b.png'),
        fileInfo('/docs/notes.txt', {
          ext: 'txt',
          supported: false,
          reason: 'Unsupported file type',
        }),
      ],
    });
    openMock.mockResolvedValue([
      '/docs/a.pdf',
      '/docs/b.png',
      '/docs/notes.txt',
    ]);

    render(<Harness />);
    fireEvent.click(screen.getByRole('button', { name: 'Add files' }));

    expect(await screen.findByText('a.pdf')).toBeInTheDocument();
    expect(screen.getByText('b.png')).toBeInTheDocument();
    expect(screen.queryByText('notes.txt')).not.toBeInTheDocument();
    // One toast lists the rejected file with its reason.
    expect(
      await screen.findByText(/notes\.txt — Unsupported file type/),
    ).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'inspect_files')).toBe(true);
  });

  it('does not re-add a file already in the list', async () => {
    mockJob({ inspect: (paths) => paths.map((p) => fileInfo(p)) });
    openMock.mockResolvedValue(['/docs/a.pdf', '/docs/c.pdf']);

    render(<Harness initial={[uiFile('/docs/a.pdf')]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add files' }));

    expect(await screen.findByText('c.pdf')).toBeInTheDocument();
    expect(screen.getAllByText('a.pdf')).toHaveLength(1);
  });

  it('adds files dropped onto the window (native drag & drop)', async () => {
    mockJob({ inspect: (paths) => paths.map((p) => fileInfo(p)) });
    render(<Harness />);

    await waitFor(() => expect(dnd.handler).not.toBeNull());
    await act(async () => {
      dnd.handler?.({
        payload: { type: 'drop', paths: ['/docs/dropped.pdf'] },
      });
    });
    expect(await screen.findByText('dropped.pdf')).toBeInTheDocument();
  });

  it('flags out-of-range copies and disables Print', async () => {
    mockJob();
    render(<Harness initial={[uiFile('/docs/a.pdf')]} />);

    const printBtn = screen.getByRole('button', { name: /Print 1/ });
    expect(printBtn).not.toBeDisabled();

    const copies = screen.getByDisplayValue('1');
    fireEvent.change(copies, { target: { value: '0' } });
    expect(screen.getByText(/whole number from 1 to 999/)).toBeInTheDocument();
    expect(printBtn).toBeDisabled();

    fireEvent.change(copies, { target: { value: '1000' } });
    expect(printBtn).toBeDisabled();

    fireEvent.change(copies, { target: { value: '3' } });
    expect(printBtn).not.toBeDisabled();
  });

  it('flags an inverted page range and disables Print', async () => {
    mockJob();
    render(<Harness initial={[uiFile('/docs/a.pdf')]} />);

    const printBtn = screen.getByRole('button', { name: /Print 1/ });
    const range = screen.getByPlaceholderText('e.g. 1-5,10');

    fireEvent.change(range, { target: { value: '5-1' } });
    expect(screen.getByText(/ranges must ascend/)).toBeInTheDocument();
    expect(printBtn).toBeDisabled();

    fireEvent.change(range, { target: { value: '1-5,10' } });
    expect(printBtn).not.toBeDisabled();
  });

  it('renders the resolved-command preview, debounced', async () => {
    vi.useFakeTimers();
    try {
      const calls = mockJob({
        preview: 'lp -d Office -o sides=two-sided a.pdf',
      });
      render(<Harness initial={[uiFile('/docs/a.pdf')]} />);

      // Nothing fetched before the debounce elapses.
      expect(calls.some((c) => c.cmd === 'preview_command')).toBe(false);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(350);
      });
      expect(calls.some((c) => c.cmd === 'preview_command')).toBe(true);
      expect(screen.getByText(/lp -d Office/)).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('submits with correctly mapped settings, then clears the files', async () => {
    const calls = mockJob({
      inspect: (paths) => paths.map((p) => fileInfo(p)),
      jobIds: ['j1', 'j2'],
    });
    openMock.mockResolvedValue(['/docs/a.pdf', '/docs/b.pdf']);

    render(<App />);
    fireEvent.click(
      within(screen.getByRole('navigation')).getByText('Print Job'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add files' }));
    await screen.findByText('a.pdf');
    await screen.findByText('b.pdf');

    fireEvent.click(screen.getByRole('button', { name: /Print 2/ }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'print_files')).toBe(true),
    );
    const req = calls.find((c) => c.cmd === 'print_files')!.args
      .req as PrintRequest;
    expect(req.files).toEqual(['/docs/a.pdf', '/docs/b.pdf']);
    expect(req.profile_name).toBe('Duplex BW'); // App's default profile (p2)
    expect(req.settings).toMatchObject({
      printer: null,
      duplex: 'long',
      color_mode: 'monochrome',
      copies: 1,
      paper_size: 'a4',
      orientation: 'auto',
      scaling: 'fit',
      page_ranges: null,
    });

    // Success clears the job and navigates away, so the rows disappear.
    await waitFor(() =>
      expect(screen.queryByText('a.pdf')).not.toBeInTheDocument(),
    );
  });

  it('keeps the files and shows an error toast when submit fails', async () => {
    mockJob({
      inspect: (paths) => paths.map((p) => fileInfo(p)),
      printError: 'Printer offline',
    });
    openMock.mockResolvedValue(['/docs/a.pdf']);

    render(<App />);
    fireEvent.click(
      within(screen.getByRole('navigation')).getByText('Print Job'),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Add files' }));
    await screen.findByText('a.pdf');

    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));

    expect(await screen.findByText('Printer offline')).toBeInTheDocument();
    // The file is retained so the user can fix and resubmit.
    expect(screen.getByText('a.pdf')).toBeInTheDocument();
  });
});
