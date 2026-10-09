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

import type { FileInfo, FolderExpansion, PrintRequest } from '@/lib/ipc';
import type { UiFile } from '@/types/domain';
import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  findProfileIn,
  type PrintJobOverrides,
} from '@/lib/profiles';
import { I18nProvider } from '@/i18n/I18nProvider';
import { clearPendingFolders } from '@/lib/jobFiles';
import { useSettingsStore } from '@/stores/settingsStore';
import { fixtureAppInfo, fixtureAppSettings } from '@/test/fixtures';
import { fixtureProfiles } from '@/test/fixtures';
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
// Every event the app subscribes to has to appear here: the stub replaces the
// `events` object wholesale, so a missing name surfaces as an undefined
// `.listen` deep inside a screen rather than as an obvious gap in the mock.
const eventMock = vi.hoisted(() => {
  const noop = { listen: () => Promise.resolve(() => {}) };
  return {
    events: {
      queueUpdated: noop,
      jobProgressEvent: noop,
      updateAvailable: noop,
      selectionForwarded: noop,
    },
  };
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
import { useProfilesStore } from '@/stores/profilesStore';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

interface MockOpts {
  inspect?: (paths: string[]) => FileInfo[];
  expand?: (paths: string[], scan: string) => FolderExpansion;
  printers?: { id: string; name: string; papers?: string[] }[];
  preview?: string;
  printError?: string;
  jobIds?: string[];
}

/** A `FolderExpansion` with everything empty unless overridden. */
function expansion(over: Partial<FolderExpansion> = {}): FolderExpansion {
  return {
    root: '/docs/reports',
    scan: 'top_level',
    files: [],
    skipped: [],
    files_truncated: false,
    depth_truncated: false,
    limits: { max_files: 5000, max_depth: 16 },
    ...over,
  };
}

/** Minimal `Printer` wire rows — only id/name/duplex matter to this screen. */
function printerRow(id: string, name: string) {
  return {
    id,
    name,
    kind: 'laser',
    color: true,
    duplex: true,
    status: 'ready',
    is_default: false,
    location: '',
    connection: 'usb',
    driver: '',
    shared: false,
    queued_jobs: 0,
    max_dpi: null,
    papers: [],
    trays: [],
    supplies: {},
    default_tray: null,
    default_paper: null,
  };
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
      case 'list_profiles':
        return fixtureProfiles;
      case 'expand_folders': {
        const a = args as { paths: string[]; scan: string };
        const one = opts.expand?.(a.paths, a.scan);
        return one ? [one] : [];
      }
      case 'list_printers':
        return (opts.printers ?? []).map((p) => ({
          ...printerRow(p.id, p.name),
          papers: p.papers ?? [],
        }));
      case 'default_printer':
        return null;
      case 'get_printer_prefs':
        return {};
      // App drains the context-menu store on mount (FR-001); these tests
      // start from an empty one.
      case 'take_pending_selections':
        return [];
      // The screen probes external dependencies (FR-058) through app_info;
      // answering with the real shape keeps that path exercised rather than
      // failing on a null payload.
      case 'app_info':
        return fixtureAppInfo;
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
    reason_code: null,
    is_dir: false,
    has_subfolders: false,
    ...over,
  };
}

/** A folder as `inspect_files` reports it (FR-004: a question, not a refusal). */
function folderInfo(path: string, hasSubfolders: boolean): FileInfo {
  return fileInfo(path, {
    ext: '',
    bytes: 0,
    pages: null,
    supported: false,
    reason: 'Folder — expand it to print the files inside',
    is_dir: true,
    has_subfolders: hasSubfolders,
  });
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
function Harness({
  initial = [],
  profileId = 'builtin-quick',
  onRequest,
}: {
  initial?: UiFile[];
  profileId?: string;
  onRequest?: (req: PrintRequest) => void;
}) {
  const [files, setFiles] = useState<UiFile[]>(initial);
  const [profile, setProfile] = useState(profileId);
  const [overrides, setOverrides] = useState<PrintJobOverrides>({});
  return (
    <I18nProvider>
      <PrintJobScreen
        files={files}
        setFiles={setFiles}
        profile={profile}
        setProfile={setProfile}
        overrides={overrides}
        setOverrides={setOverrides}
        onPrint={() =>
          onRequest?.(
            buildPrintRequest(
              files.map((f) => f.path),
              findProfileIn(useProfilesStore.getState().profiles, profile),
              overrides,
            ),
          )
        }
      />
      <Toaster theme="light" />
    </I18nProvider>
  );
}

function resetStore() {
  clearPendingFolders();
  useSettingsStore.setState({ settings: fixtureAppSettings });
  useQueueStore.setState({
    status: 'idle',
    snapshot: null,
    uiJobs: [],
    counts: { active: 0, pending: 0, done: 0 },
    queuePaused: false,
    recentDone: [],
  });
  // Seed as ready so PrintJobScreen's idempotent init() short-circuits — no
  // async store update lands after a synchronous test body (act warnings).
  useProfilesStore.setState({
    status: 'ready',
    profiles: BUILT_IN_PROFILES,
    error: undefined,
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

    fireEvent.change(range, { target: { value: '1-3' } });
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
    // The job follows the app's default profile (Quick) until the user picks
    // another one explicitly (FR-022).
    expect(req.profile_name).toBe('Quick');
    expect(req.settings).toMatchObject({
      printer: null,
      duplex: 'none',
      color_mode: 'color',
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
  // -- printer identity (FR-044) ----------------------------------------

  it('FR-044: sends the printer queue id, not its display name', async () => {
    // On CUPS the id (queue name) and the display name (`printer-info`) differ;
    // `lp -d "Front desk (2nd floor)"` would fail. The dropdown must therefore
    // carry the id as its value and use the name only as a label.
    const seen: PrintRequest[] = [];
    mockJob({
      printers: [{ id: 'HP_LaserJet_2', name: 'Front desk (2nd floor)' }],
    });

    render(
      <Harness
        initial={[uiFile('/docs/a.pdf')]}
        onRequest={(req) => seen.push(req)}
      />,
    );

    const select = await screen.findByRole('combobox', { name: 'Printer' });
    await waitFor(() =>
      expect(
        within(select).getByText('Front desk (2nd floor)'),
      ).toBeInTheDocument(),
    );
    expect(
      (within(select).getByText('Front desk (2nd floor)') as HTMLOptionElement)
        .value,
    ).toBe('HP_LaserJet_2');

    fireEvent.change(select, { target: { value: 'HP_LaserJet_2' } });
    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));

    expect(seen.at(-1)?.settings.printer).toBe('HP_LaserJet_2');
  });

  it('FR-041: offers every paper size the chosen printer reports, plus its own default', async () => {
    const seen: PrintRequest[] = [];
    mockJob({
      printers: [
        {
          id: 'Photo',
          name: 'Photo printer',
          papers: ['A4', 'Letter', '4x6', 'Tabloid'],
        },
      ],
    });
    render(
      <Harness
        initial={[uiFile('/docs/a.pdf')]}
        onRequest={(req) => seen.push(req)}
      />,
    );

    const printer = await screen.findByRole('combobox', { name: 'Printer' });
    await waitFor(() =>
      expect(within(printer).getByText('Photo printer')).toBeInTheDocument(),
    );
    fireEvent.change(printer, { target: { value: 'Photo' } });

    const paper = screen.getByRole('combobox', { name: 'Paper' });
    for (const size of ['Printer default', 'A4', 'Legal', '4x6', 'Tabloid']) {
      expect(within(paper).getByText(size)).toBeInTheDocument();
    }

    fireEvent.change(paper, { target: { value: '4x6' } });
    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));
    expect(seen.at(-1)?.settings.paper_size).toBe('4x6');

    fireEvent.change(paper, { target: { value: 'auto' } });
    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));
    expect(seen.at(-1)?.settings.paper_size).toBe('auto');
  });

  it('FR-041: the collate toggle reaches the request', async () => {
    const seen: PrintRequest[] = [];
    mockJob();
    render(
      <Harness
        initial={[uiFile('/docs/a.pdf')]}
        onRequest={(req) => seen.push(req)}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));
    expect(seen.at(-1)?.settings.collate).toBe(true);

    fireEvent.click(screen.getByRole('switch', { name: 'Collate copies' }));
    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));
    expect(seen.at(-1)?.settings.collate).toBe(false);
  });

  it('FR-044: keeps a stored printer selectable even when it is not installed', async () => {
    // A profile saved against an offline (or pre-id) printer must not snap
    // silently to the system default.
    mockJob({ printers: [{ id: 'HP_LaserJet_2', name: 'Front desk' }] });
    useProfilesStore.setState({
      status: 'ready',
      profiles: [
        { ...BUILT_IN_PROFILES[0]!, id: 'p-stored', printer: 'Old Printer' },
      ],
      error: undefined,
    });

    render(<Harness initial={[uiFile('/docs/a.pdf')]} profileId="p-stored" />);

    const select = await screen.findByRole('combobox', { name: 'Printer' });
    expect((select as HTMLSelectElement).value).toBe('Old Printer');
    expect(within(select).getByText('Old Printer')).toBeInTheDocument();
  });

  // -- page range (FR-041 / FR-042) --------------------------------------

  it('FR-041: pre-fills the page range from the profile so what is shown is what is sent', async () => {
    const seen: PrintRequest[] = [];
    mockJob();
    useProfilesStore.setState({
      status: 'ready',
      profiles: [{ ...BUILT_IN_PROFILES[0]!, id: 'p-range', pageRange: '1-3' }],
      error: undefined,
    });

    render(
      <Harness
        initial={[uiFile('/docs/a.pdf')]}
        profileId="p-range"
        onRequest={(req) => seen.push(req)}
      />,
    );

    const range = screen.getByPlaceholderText('e.g. 1-5,10');
    expect((range as HTMLInputElement).value).toBe('1-3');

    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));
    expect(seen.at(-1)?.settings.page_ranges).toBe('1-3');
  });

  it('FR-041: clearing the pre-filled range prints every page', async () => {
    const seen: PrintRequest[] = [];
    mockJob();
    useProfilesStore.setState({
      status: 'ready',
      profiles: [{ ...BUILT_IN_PROFILES[0]!, id: 'p-range', pageRange: '1-3' }],
      error: undefined,
    });

    render(
      <Harness
        initial={[uiFile('/docs/a.pdf')]}
        profileId="p-range"
        onRequest={(req) => seen.push(req)}
      />,
    );

    const range = screen.getByPlaceholderText('e.g. 1-5,10');
    fireEvent.change(range, { target: { value: '' } });
    expect((range as HTMLInputElement).value).toBe('');

    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));
    expect(seen.at(-1)?.settings.page_ranges).toBeNull();
  });

  it('FR-042: flags a range that runs past the document and blocks Print', async () => {
    mockJob();
    // uiFile() reports 3 pages.
    render(<Harness initial={[uiFile('/docs/short.pdf')]} />);

    const printBtn = screen.getByRole('button', { name: /Print 1/ });
    const range = screen.getByPlaceholderText('e.g. 1-5,10');

    fireEvent.change(range, { target: { value: '2-9' } });
    expect(screen.getByRole('alert')).toHaveTextContent(
      /short\.pdf.*only has 3 pages/,
    );
    expect(printBtn).toBeDisabled();

    fireEvent.change(range, { target: { value: '2-3' } });
    expect(printBtn).not.toBeDisabled();
  });

  it('FR-042: a one-page overflow reads in the singular', async () => {
    mockJob();
    render(<Harness initial={[{ ...uiFile('/docs/one.pdf'), pages: 1 }]} />);
    fireEvent.change(screen.getByPlaceholderText('e.g. 1-5,10'), {
      target: { value: '2' },
    });
    expect(screen.getByRole('alert')).toHaveTextContent(
      '“one.pdf” only has 1 page.',
    );
  });

  it('a PDF whose page count is unknown is not called an image, nor counted as one page', async () => {
    mockJob();
    render(
      <Harness
        initial={[
          { ...uiFile('/docs/huge.pdf'), pages: null },
          uiFile('/docs/three.pdf'),
        ]}
      />,
    );
    expect(screen.getByText(/pages unknown/)).toBeInTheDocument();
    expect(screen.queryByText(/^image/)).not.toBeInTheDocument();
    // 3 known pages, plus one document that has at least… something.
    expect(screen.getByText(/at least 3 pages/)).toBeInTheDocument();
  });

  it('FR-041: picking another profile drops the overrides made against the old one', async () => {
    mockJob();
    let last: PrintRequest | undefined;
    render(
      <Harness
        initial={[uiFile('/docs/a.pdf')]}
        onRequest={(r) => (last = r)}
      />,
    );
    fireEvent.change(screen.getByDisplayValue('1'), {
      target: { value: '4' },
    });
    fireEvent.change(screen.getByRole('combobox', { name: 'Profile' }), {
      target: { value: 'builtin-duplex-bw' },
    });
    // The field shows the new profile's own copies…
    expect(screen.getByDisplayValue('1')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Print 1/ }));
    // …and that is what is sent, with the profile's own duplex.
    expect(last?.settings.copies).toBe(1);
    expect(last?.settings.duplex).toBe('long');
  });

  it('FR-042: an image never invalidates a range (the engine drops it)', async () => {
    mockJob();
    render(
      <Harness
        initial={[{ ...uiFile('/docs/chart.png'), ext: 'png', pages: 1 }]}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText('e.g. 1-5,10'), {
      target: { value: '2-9' },
    });
    expect(screen.getByRole('button', { name: /Print 1/ })).not.toBeDisabled();
  });

  // -- folders (FR-004 / FR-005) -----------------------------------------

  it('FR-004: asks before recursing into a dropped folder', async () => {
    const calls = mockJob({
      inspect: (paths) =>
        paths.map((p) =>
          p === '/docs/reports' ? folderInfo(p, true) : fileInfo(p),
        ),
      expand: () =>
        expansion({ scan: 'recursive', files: ['/docs/reports/deep/a.pdf'] }),
    });
    render(<Harness />);

    await waitFor(() => expect(dnd.handler).not.toBeNull());
    await act(async () => {
      dnd.handler?.({ payload: { type: 'drop', paths: ['/docs/reports'] } });
    });

    // Nothing is walked until the question is answered.
    expect(calls.some((c) => c.cmd === 'expand_folders')).toBe(false);
    expect(
      await screen.findByText(/Print everything inside .reports./),
    ).toBeInTheDocument();
    // Neutral wording: the same banner follows the Add folder button.
    expect(screen.getByText('1 folder added')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Include subfolders' }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'expand_folders')).toBe(true),
    );
    expect(calls.find((c) => c.cmd === 'expand_folders')!.args.scan).toBe(
      'recursive',
    );
    expect(await screen.findByText('a.pdf')).toBeInTheDocument();
    // The question is gone once answered.
    expect(
      screen.queryByRole('button', { name: 'Include subfolders' }),
    ).not.toBeInTheDocument();
  });

  it('FR-004: "Top level only" walks one level deep', async () => {
    const calls = mockJob({
      inspect: (paths) =>
        paths.map((p) =>
          p === '/docs/reports' ? folderInfo(p, true) : fileInfo(p),
        ),
      expand: () => expansion({ files: ['/docs/reports/a.pdf'] }),
    });
    render(<Harness />);

    await waitFor(() => expect(dnd.handler).not.toBeNull());
    await act(async () => {
      dnd.handler?.({ payload: { type: 'drop', paths: ['/docs/reports'] } });
    });
    fireEvent.click(
      await screen.findByRole('button', { name: 'Top level only' }),
    );

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'expand_folders')).toBe(true),
    );
    expect(calls.find((c) => c.cmd === 'expand_folders')!.args.scan).toBe(
      'top_level',
    );
  });

  it('FR-004: skipping a folder leaves the job untouched', async () => {
    const calls = mockJob({
      inspect: () => [folderInfo('/docs/reports', true)],
    });
    render(<Harness />);

    await waitFor(() => expect(dnd.handler).not.toBeNull());
    await act(async () => {
      dnd.handler?.({ payload: { type: 'drop', paths: ['/docs/reports'] } });
    });
    fireEvent.click(
      await screen.findByRole('button', { name: 'Skip this folder' }),
    );

    expect(
      screen.queryByText(/Print everything inside/),
    ).not.toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'expand_folders')).toBe(false);
  });

  it('FR-004: a folder without subfolders is expanded without a question', async () => {
    const calls = mockJob({
      inspect: (paths) =>
        paths.map((p) =>
          p === '/docs/flat' ? folderInfo(p, false) : fileInfo(p),
        ),
      expand: () => expansion({ files: ['/docs/flat/a.pdf'] }),
    });
    render(<Harness />);

    await waitFor(() => expect(dnd.handler).not.toBeNull());
    await act(async () => {
      dnd.handler?.({ payload: { type: 'drop', paths: ['/docs/flat'] } });
    });

    expect(await screen.findByText('a.pdf')).toBeInTheDocument();
    expect(
      screen.queryByText(/Print everything inside/),
    ).not.toBeInTheDocument();
    expect(calls.find((c) => c.cmd === 'expand_folders')!.args.scan).toBe(
      'top_level',
    );
  });

  it('FR-005: shows what a folder walk discarded', async () => {
    mockJob({
      inspect: (paths) =>
        paths.map((p) =>
          p === '/docs/flat' ? folderInfo(p, false) : fileInfo(p),
        ),
      expand: () =>
        expansion({
          files: ['/docs/flat/a.pdf'],
          skipped: [
            {
              path: '/docs/flat/notes.docx',
              name: 'notes.docx',
              reason: 'unsupported_format',
              detail: 'Unsupported file type (.docx)',
            },
          ],
        }),
    });
    render(<Harness />);

    await waitFor(() => expect(dnd.handler).not.toBeNull());
    await act(async () => {
      dnd.handler?.({ payload: { type: 'drop', paths: ['/docs/flat'] } });
    });

    expect(
      await screen.findByText(/notes\.docx — Unsupported file type \(\.docx\)/),
    ).toBeInTheDocument();
  });

  // -- reordering --------------------------------------------------------

  it('reorders a file with the buttons', async () => {
    mockJob();
    render(
      <Harness initial={[uiFile('/docs/a.pdf'), uiFile('/docs/b.pdf')]} />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Move b.pdf up' }));

    const rows = screen.getAllByRole('listitem');
    expect(rows[0]).toHaveTextContent('b.pdf');
    expect(rows[1]).toHaveTextContent('a.pdf');
  });

  it('reorders a focused file row with Alt + arrow keys', async () => {
    // Regression: the header advertised an "↑↓ reorder" shortcut that nothing
    // implemented. The hint now names Alt + arrows, and they work.
    mockJob();
    render(
      <Harness initial={[uiFile('/docs/a.pdf'), uiFile('/docs/b.pdf')]} />,
    );

    const second = screen.getAllByRole('listitem')[1]!;
    second.focus();
    fireEvent.keyDown(second, { key: 'ArrowUp', altKey: true });

    expect(screen.getAllByRole('listitem')[0]).toHaveTextContent('b.pdf');

    // A bare arrow must not reorder — it belongs to the scroll container.
    fireEvent.keyDown(screen.getAllByRole('listitem')[0]!, {
      key: 'ArrowDown',
    });
    expect(screen.getAllByRole('listitem')[0]).toHaveTextContent('b.pdf');
  });

  it('does not move the first row up or the last row down', async () => {
    mockJob();
    render(
      <Harness initial={[uiFile('/docs/a.pdf'), uiFile('/docs/b.pdf')]} />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Move a.pdf up' }));
    fireEvent.click(screen.getByRole('button', { name: 'Move b.pdf down' }));

    const rows = screen.getAllByRole('listitem');
    expect(rows[0]).toHaveTextContent('a.pdf');
    expect(rows[1]).toHaveTextContent('b.pdf');
    // NFR-041: and those two no-op buttons say so instead of being announced
    // as working actions.
    expect(
      screen.getByRole('button', { name: 'Move a.pdf up' }),
    ).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Move b.pdf down' }),
    ).toBeDisabled();
  });

  it('NFR-041: keeps focus on a row moved down with Alt + arrow, step after step', async () => {
    mockJob();
    render(
      <Harness
        initial={[
          uiFile('/docs/a.pdf'),
          uiFile('/docs/b.pdf'),
          uiFile('/docs/c.pdf'),
        ]}
      />,
    );
    const first = screen.getAllByRole('listitem')[0]!;
    first.focus();
    fireEvent.keyDown(first, { key: 'ArrowDown', altKey: true });
    await waitFor(() =>
      expect(document.activeElement).toHaveTextContent('a.pdf'),
    );
    fireEvent.keyDown(document.activeElement!, {
      key: 'ArrowDown',
      altKey: true,
    });
    await waitFor(() =>
      expect(screen.getAllByRole('listitem')[2]).toHaveTextContent('a.pdf'),
    );
    expect(document.activeElement).toHaveTextContent('a.pdf');
  });

  it('NFR-041: removing a file moves focus to the next row, not to <body>', async () => {
    mockJob();
    render(
      <Harness initial={[uiFile('/docs/a.pdf'), uiFile('/docs/b.pdf')]} />,
    );
    const removeA = screen.getByRole('button', { name: 'Remove a.pdf' });
    removeA.focus();
    fireEvent.click(removeA);
    await waitFor(() =>
      expect(document.activeElement).toHaveTextContent('b.pdf'),
    );
    expect(document.activeElement).toHaveAttribute('role', 'listitem');
  });

  it('NFR-043: names the copies and page-range fields and the option groups', () => {
    mockJob();
    render(<Harness initial={[uiFile('/docs/a.pdf')]} />);
    const copies = screen.getByRole('spinbutton', { name: 'Copies' });
    // The real range, so a screen reader never announces 0–0.
    expect(copies).toHaveAttribute('min', '1');
    expect(copies).toHaveAttribute('max', '999');
    expect(
      screen.getByRole('textbox', { name: 'Page range' }),
    ).toHaveAccessibleDescription();
    for (const name of ['Sides', 'Color', 'Scaling']) {
      expect(screen.getByRole('radiogroup', { name })).toBeInTheDocument();
    }
  });

  it('removes a file from the batch', async () => {
    mockJob();
    render(
      <Harness initial={[uiFile('/docs/a.pdf'), uiFile('/docs/b.pdf')]} />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Remove a.pdf' }));

    expect(screen.queryByText('a.pdf')).not.toBeInTheDocument();
    expect(screen.getByText('b.pdf')).toBeInTheDocument();
  });

  // -- i18n (FR-092) -----------------------------------------------------

  it('FR-092: renders every visible string in Spanish when the locale is es-ES', () => {
    mockJob();
    useSettingsStore.setState({
      settings: {
        ...fixtureAppSettings,
        app: { ...fixtureAppSettings.app, locale: 'es-ES' },
      },
    });

    render(<Harness initial={[uiFile('/docs/a.pdf')]} />);

    expect(screen.getByText('Trabajo de impresión')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Añadir archivos' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /Imprimir 1/ }),
    ).toBeInTheDocument();
    expect(screen.getByText('Archivos de este trabajo')).toBeInTheDocument();
    expect(screen.getByText(/1 archivo · 3 páginas/)).toBeInTheDocument();
    expect(
      screen.getByText(/Se aplica a los PDF y documentos/),
    ).toBeInTheDocument();
  });

  // -- smoke -------------------------------------------------------------

  it('smoke: renders an explicit empty state without the desktop host', () => {
    // `clearMocks()` unregisters the IPC handler but leaves the injected
    // globals, so drop them by hand — this test is specifically about the
    // standalone Vite preview, where they never existed.
    clearMocks();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI__;
    render(<Harness />);

    expect(screen.getByText('Print Job')).toBeInTheDocument();
    expect(
      screen.getByText('Adding files requires the desktop app.'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Add files' })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Print 0/ })).toBeDisabled();
  });
});
