import {
  render,
  screen,
  fireEvent,
  within,
  waitFor,
  act,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

/**
 * Capture the native drag & drop handler App registers, so a drop can be
 * simulated without a real webview. Hoisted because `vi.mock` is.
 */
const webview = vi.hoisted(() => ({
  handler: null as
    | null
    | ((e: { payload: { type: string; paths?: string[] } }) => void),
}));
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (cb: (e: { payload: { type: string } }) => void) => {
      webview.handler = cb as typeof webview.handler;
      return Promise.resolve(() => {});
    },
  }),
}));

/**
 * The tauri-specta `events` object drives the event plugin over IPC, which
 * mockIPC would also intercept. Swapping just `events` for a controllable fake
 * (commands stay real, so mockIPC still serves invokes) is how the rest of the
 * suite emits backend events on demand — see `queueStore.test.ts`.
 */
type EventListener = (e: { payload: unknown }) => void;
const eventMock = vi.hoisted(() => {
  const registry = {
    updateAvailable: new Set<(e: { payload: unknown }) => void>(),
    queueUpdated: new Set<(e: { payload: unknown }) => void>(),
    jobProgressEvent: new Set<(e: { payload: unknown }) => void>(),
  };
  const listener = (set: Set<(e: { payload: unknown }) => void>) => ({
    listen: (cb: (e: { payload: unknown }) => void) => {
      set.add(cb);
      return Promise.resolve(() => set.delete(cb));
    },
  });
  return {
    registry,
    events: {
      updateAvailable: listener(registry.updateAvailable),
      queueUpdated: listener(registry.queueUpdated),
      jobProgressEvent: listener(registry.jobProgressEvent),
    },
  };
});
vi.mock('@/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/ipc/bindings')>();
  return { ...actual, events: eventMock.events };
});

import App from './App';
import { shortcutLabel } from '@/components/AppShell/MPAppShell';
import { toUiQueueJob } from '@/lib/ipc';
import {
  fixtureAppSettings,
  fixturePrinters,
  fixtureProfiles,
  mkJob,
} from '@/test/fixtures';
import { useProfilesStore } from '@/stores/profilesStore';
import { useQueueStore } from '@/stores/queueStore';
import { useSettingsStore } from '@/stores/settingsStore';
import { BUILT_IN_PROFILES } from '@/lib/profiles';

/** Answer the commands App's screens/stores fire during integration flows. */
function mockShell(overrides: Record<string, unknown> = {}) {
  mockIPC((cmd) => {
    if (cmd in overrides) return overrides[cmd];
    switch (cmd) {
      case 'list_profiles':
        return fixtureProfiles;
      case 'get_app_settings':
        return fixtureAppSettings;
      case 'queue_status':
        return { schema_version: 1, queue_paused: false, jobs: [] };
      case 'list_printers':
        return [];
      case 'get_printer_prefs':
        return {};
      default:
        return null;
    }
  });
}

describe('App', () => {
  beforeEach(() => {
    // Skip onboarding modal so the main shell is visible.
    window.localStorage.setItem('maeclyprint:onboarding-complete', '1');
    window.localStorage.removeItem('maeclyprint:theme');
    window.localStorage.removeItem('maeclyprint:density');
    document.documentElement.classList.remove('mp-light', 'mp-dark');
    // Module-level zustand stores leak state across tests — reset them.
    useProfilesStore.setState({
      status: 'idle',
      profiles: BUILT_IN_PROFILES,
      error: undefined,
    });
    useSettingsStore.setState({
      status: 'idle',
      settings: null,
      error: undefined,
    });
    useQueueStore.setState({
      status: 'idle',
      snapshot: null,
      subscribed: false,
      uiJobs: [],
      counts: { active: 0, pending: 0, done: 0 },
      queuePaused: false,
      recentDone: [],
    });
    webview.handler = null;
    for (const set of Object.values(eventMock.registry)) set.clear();
  });
  afterEach(() => clearMocks());

  it('renders the sidebar nav items', () => {
    render(<App />);
    // Sidebar items live inside <nav>; pick that scope to avoid false
    // positives from screen content that mentions the same words.
    const nav = screen.getByRole('navigation');
    expect(nav).toBeInTheDocument();
    // "Print Job" is unique to the sidebar; a sanity check for the rest.
    expect(screen.getByText('Print Job')).toBeInTheDocument();
    expect(within(nav).getByText('Printers')).toBeInTheDocument();
    expect(screen.getByText('Profiles')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });

  it('starts on the Home screen with the drop zone visible', () => {
    render(<App />);
    expect(screen.getByText(/Drop files to print/i)).toBeInTheDocument();
  });

  it('navigates to the Printers screen and lists real printers from IPC', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_printers') return fixturePrinters;
      if (cmd === 'default_printer') return 'EPSON_L15160';
      if (cmd === 'discover_printers') return [];
      return null;
    });
    render(<App />);
    const nav = screen.getByRole('navigation');
    fireEvent.click(within(nav).getByText('Printers'));
    // The enumerated printer name appears (master list + detail) once the
    // screen's IPC load resolves.
    await waitFor(() =>
      expect(screen.getAllByText(/EPSON L15160 Series/).length).toBeGreaterThan(
        0,
      ),
    );
  });

  it('integration: store profiles (custom included) feed the Print Job dropdown', async () => {
    mockShell();
    render(<App />);
    fireEvent.click(
      within(screen.getByRole('navigation')).getByText('Print Job'),
    );
    // The custom profile from list_profiles is offered alongside builtins.
    await waitFor(() =>
      expect(
        screen.getByRole('option', { name: 'Contracts duplex' }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole('option', { name: 'Quick · built-in' }),
    ).toBeInTheDocument();
  });

  it('integration: the app default profile drives the initial Print Job selection (FR-022)', async () => {
    // Server marks the custom profile as the default.
    mockShell({
      list_profiles: fixtureProfiles.map((p) => ({
        ...p,
        default: p.name === 'Contracts duplex',
      })),
    });
    render(<App />);
    fireEvent.click(
      within(screen.getByRole('navigation')).getByText('Print Job'),
    );
    await waitFor(() =>
      expect(screen.getByDisplayValue('Contracts duplex')).toBeInTheDocument(),
    );
  });

  it('integration: an explicit profile choice survives navigating away and back', async () => {
    mockShell();
    render(<App />);
    const nav = screen.getByRole('navigation');
    fireEvent.click(within(nav).getByText('Print Job'));
    const select = await screen.findByDisplayValue('Quick · built-in');
    fireEvent.change(select, { target: { value: 'builtin-duplex-bw' } });
    expect(
      screen.getByDisplayValue('Duplex BW · built-in'),
    ).toBeInTheDocument();

    fireEvent.click(within(nav).getByText('Home'));
    fireEvent.click(within(nav).getByText('Print Job'));
    expect(
      screen.getByDisplayValue('Duplex BW · built-in'),
    ).toBeInTheDocument();
  });

  it('an explicit profile choice survives a transient profiles-store error', async () => {
    const CUSTOM_ID = '01890a5e-7b2c-7c3d-8e4f-1a2b3c4d5e6f';
    mockShell();
    render(<App />);
    const nav = screen.getByRole('navigation');
    fireEvent.click(within(nav).getByText('Print Job'));

    // Explicitly choose the custom profile.
    const select = await screen.findByDisplayValue('Quick · built-in');
    fireEvent.change(select, { target: { value: CUSTOM_ID } });
    expect(screen.getByDisplayValue('Contracts duplex')).toBeInTheDocument();

    // Snapshot the adapted list, then drive the store through a transient
    // error (profiles momentarily empty) and back to ready. The fallback
    // effect must not clear the explicit choice while the store is not ready.
    const adapted = useProfilesStore.getState().profiles;
    act(() => {
      useProfilesStore.setState({
        status: 'error',
        profiles: [],
        error: 'boom',
      });
    });
    act(() => {
      useProfilesStore.setState({
        status: 'ready',
        profiles: adapted,
        error: undefined,
      });
    });

    // The choice was preserved across the error (not reset to the default).
    expect(screen.getByDisplayValue('Contracts duplex')).toBeInTheDocument();
  });

  it('integration: the backend theme wins over the localStorage cache on boot', async () => {
    window.localStorage.setItem('maeclyprint:theme', 'light');
    mockShell({
      get_app_settings: {
        ...fixtureAppSettings,
        app: { ...fixtureAppSettings.app, theme: 'dark' },
      },
    });
    render(<App />);
    await waitFor(() =>
      expect(document.documentElement.classList.contains('mp-dark')).toBe(true),
    );
    // The reconciled value is mirrored back into the localStorage cache.
    expect(window.localStorage.getItem('maeclyprint:theme')).toBe('dark');
  });

  // -------------------------------------------------------------------------
  // Launch-time update check (FR-080 / FR-084)
  // -------------------------------------------------------------------------

  describe('update-available notification', () => {
    /**
     * What the backend pushes after a launch-time check found something. The
     * decision to push at all (preference, 24h throttle, skipped version) is
     * made in Rust and covered by `src-tauri/tests/update_startup.rs`; the
     * frontend's job starts here.
     */
    const CHECK = {
      current: '1.0.0',
      latest: 'v1.2.0',
      available: true,
      url: 'https://github.com/MAECLY/maecly-print/releases/tag/v1.2.0',
      notes: 'Faster printing',
      checked_at: '2026-07-20T12:00:00Z',
    };

    function emitUpdateAvailable(payload: unknown = CHECK) {
      const set = eventMock.registry.updateAvailable as Set<EventListener>;
      act(() => {
        for (const cb of set) cb({ payload });
      });
    }

    /** Render App and wait until its event listener is registered. */
    async function renderSubscribed() {
      render(<App />);
      await waitFor(() =>
        expect(eventMock.registry.updateAvailable.size).toBe(1),
      );
    }

    it('FR-080: tells the user from whatever screen they are on', async () => {
      mockShell();
      await renderSubscribed();
      // Still on Home — the point is that the About screen is not required.
      expect(screen.getByText(/Drop files to print/i)).toBeInTheDocument();

      emitUpdateAvailable();
      expect(
        await screen.findByText('Version v1.2.0 is available'),
      ).toBeInTheDocument();
      expect(screen.getByText("You're on 1.0.0.")).toBeInTheDocument();
    });

    it('links to the release page instead of offering an install this build cannot do', async () => {
      const calls: string[] = [];
      mockIPC((cmd, args) => {
        calls.push(cmd);
        if (cmd === 'open_external')
          return (args as { url: string } | undefined)?.url ?? null;
        if (cmd === 'get_app_settings') return fixtureAppSettings;
        if (cmd === 'list_profiles') return fixtureProfiles;
        if (cmd === 'queue_status')
          return { schema_version: 1, queue_paused: false, jobs: [] };
        return null;
      });
      await renderSubscribed();
      emitUpdateAvailable();

      // FR-082/FR-083 land with the signed auto-updater: nothing here may
      // suggest the app can install anything.
      const action = await screen.findByRole('button', {
        name: 'Release notes',
      });
      expect(
        screen.queryByRole('button', { name: /install|update now/i }),
      ).toBeNull();

      fireEvent.click(action);
      await waitFor(() => expect(calls).toContain('open_external'));
    });

    it('says nothing when the payload names no version', async () => {
      mockShell();
      await renderSubscribed();
      emitUpdateAvailable({ ...CHECK, latest: null, url: null });
      await waitFor(() =>
        expect(screen.queryByText(/is available/)).toBeNull(),
      );
    });
  });

  // -------------------------------------------------------------------------
  // Keyboard shortcuts (NFR-034)
  // -------------------------------------------------------------------------

  describe('keyboard shortcuts', () => {
    interface Call {
      cmd: string;
      args: Record<string, unknown>;
    }

    /** `mockShell` plus a recording of every command that went out. */
    function recordShell(overrides: Record<string, unknown> = {}): Call[] {
      const calls: Call[] = [];
      mockIPC((cmd, args) => {
        calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
        if (cmd in overrides) return overrides[cmd];
        switch (cmd) {
          case 'list_profiles':
            return fixtureProfiles;
          case 'get_app_settings':
            return fixtureAppSettings;
          case 'queue_status':
            return { schema_version: 1, queue_paused: false, jobs: [] };
          case 'list_printers':
            return [];
          case 'get_printer_prefs':
            return {};
          case 'inspect_files':
            return [
              {
                path: '/tmp/Report.pdf',
                name: 'Report.pdf',
                ext: 'pdf',
                bytes: 2048,
                pages: 3,
                supported: true,
                reason: null,
                is_dir: false,
                has_subfolders: false,
              },
            ];
          case 'print_files':
            return { batch_id: 'b1', job_ids: ['j1'] };
          default:
            return null;
        }
      });
      return calls;
    }

    /** Press a shortcut the way the platform modifier requires. */
    function press(key: string, target: Element | Window = window) {
      fireEvent.keyDown(target, {
        key,
        ctrlKey: !shortcutLabel('newJob').startsWith('⌘'),
        metaKey: shortcutLabel('newJob').startsWith('⌘'),
      });
    }

    it('NFR-034: opens a new print job and the queue from anywhere', async () => {
      recordShell();
      render(<App />);
      press('n');
      expect(await screen.findByText('Add files')).toBeInTheDocument();
      press('j');
      // The Queue screen's own body, not the sidebar entry.
      expect(await screen.findByText(/No jobs in the queue/i)).toBeVisible();
    });

    it('NFR-034: switches the job to the next profile', async () => {
      recordShell();
      render(<App />);
      press('n');
      const select = await screen.findByDisplayValue('Quick · built-in');
      expect(select).toBeInTheDocument();
      press('k');
      await waitFor(() =>
        expect(screen.queryByDisplayValue('Quick · built-in')).toBeNull(),
      );
    });

    it('NFR-034: prints the current job, after a drop put files in it', async () => {
      const calls = recordShell();
      render(<App />);
      await waitFor(() => expect(webview.handler).not.toBeNull());
      await act(async () => {
        webview.handler!({
          payload: { type: 'drop', paths: ['/tmp/Report.pdf'] },
        });
      });
      // The dropped file is now a row of the job (its per-row controls name it).
      expect(
        (await screen.findAllByRole('button', { name: /Report\.pdf/ })).length,
      ).toBeGreaterThan(0);

      press('p');
      await waitFor(() =>
        expect(calls.some((c) => c.cmd === 'print_files')).toBe(true),
      );
    });

    it('NFR-034: printing with an empty job opens it instead of spooling nothing', async () => {
      const calls = recordShell();
      render(<App />);
      press('p');
      expect(await screen.findByText('Add files')).toBeInTheDocument();
      expect(calls.some((c) => c.cmd === 'print_files')).toBe(false);
    });

    it('NFR-034: cancel asks first, then cancels the job on the printer', async () => {
      const calls = recordShell();
      render(<App />);
      act(() => {
        useQueueStore.setState({
          status: 'ready',
          uiJobs: [
            toUiQueueJob(
              mkJob({
                id: 'j-printing',
                state: 'printing',
                file_name: 'Q3-Report.pdf',
              }),
            ),
          ],
        });
      });
      press('.');
      // Never `window.confirm`: a real in-app dialog naming the job.
      const dialog = await screen.findByRole('dialog');
      expect(within(dialog).getByText(/Q3-Report\.pdf/)).toBeInTheDocument();
      expect(calls.some((c) => c.cmd === 'cancel_job')).toBe(false);

      fireEvent.click(
        within(dialog).getByRole('button', { name: 'Cancel the job' }),
      );
      await waitFor(() =>
        expect(calls.find((c) => c.cmd === 'cancel_job')?.args).toEqual({
          id: 'j-printing',
        }),
      );
    });

    it('NFR-034: cancel with an idle queue says so instead of opening a dialog', async () => {
      recordShell();
      render(<App />);
      press('.');
      await waitFor(() =>
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
      );
    });

    it('NFR-041: shortcuts stand down while the user is typing in a field', async () => {
      recordShell();
      render(<App />);
      press('n');
      const input = await screen.findByPlaceholderText(/1-5/i);
      fireEvent.change(input, { target: { value: '1-3' } });
      // Ctrl+J inside the field must reach the field, not the shell.
      press('j', input);
      expect(screen.queryByText(/No jobs in the queue/i)).toBeNull();
      expect(input).toBeInTheDocument();
    });

    it('regression: a bare keypress (no modifier) never triggers a shortcut', async () => {
      recordShell();
      render(<App />);
      fireEvent.keyDown(window, { key: 'j' });
      fireEvent.keyDown(window, { key: 'n' });
      await waitFor(() =>
        expect(screen.getByText(/Drop files to print/i)).toBeInTheDocument(),
      );
    });
  });
});
