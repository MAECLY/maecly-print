import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import type { AppSettings } from '@/ipc/bindings';
import { fixtureAppSettings } from '@/test/fixtures';
import { Toaster } from '@/components/AppShell/Toaster';
import type { Density, ResolvedTheme, ThemeMode } from '@/lib/theme';

// The Default-profile row reads the profiles store; mock it so this suite
// stays decoupled from that file. The mock is a plain selector over a static
// state object — enough for the screen, which only reads id/name/default.
const profilesMock = vi.hoisted(() => {
  const setDefault = vi.fn((_id: string) => Promise.resolve(true));
  const init = vi.fn(() => Promise.resolve());
  const state = {
    profiles: [
      { id: 'builtin-quick', name: 'Quick', default: true },
      { id: 'builtin-duplex-bw', name: 'Duplex BW', default: false },
      { id: 'builtin-photo-color', name: 'Photo Color', default: false },
    ],
    setDefault,
    init,
  };
  return { setDefault, init, state };
});

vi.mock('@/stores/profilesStore', () => ({
  useProfilesStore: <T,>(selector: (s: typeof profilesMock.state) => T): T =>
    selector(profilesMock.state),
}));

// Imported after the mock is registered (vi.mock hoists regardless).
import { SettingsScreen } from './SettingsScreen';
import { useSettingsStore } from '@/stores/settingsStore';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

/** Install settings IPC mocks and return the list of commands invoked. */
function mockSettings(
  opts: { initial?: AppSettings; fail?: string } = {},
): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    if (cmd === 'get_app_settings') return opts.initial ?? fixtureAppSettings;
    if (cmd === 'set_app_settings')
      return (args as { settings: AppSettings }).settings;
    return null;
  });
  return calls;
}

interface RenderOver {
  theme?: ResolvedTheme;
  density?: Density;
  mode?: ThemeMode;
  setMode?: (m: ThemeMode) => void;
}

function renderSettings(over: RenderOver = {}) {
  const setTheme = vi.fn();
  const setDensity = vi.fn();
  render(
    <>
      <SettingsScreen
        theme={over.theme ?? 'light'}
        setTheme={setTheme}
        density={over.density ?? 'comfortable'}
        setDensity={setDensity}
        {...(over.mode !== undefined ? { mode: over.mode } : {})}
        {...(over.setMode ? { setMode: over.setMode } : {})}
      />
      <Toaster theme="light" />
    </>,
  );
  return { setTheme, setDensity };
}

async function waitReady() {
  await waitFor(() => expect(useSettingsStore.getState().status).toBe('ready'));
}

/** The control cell (right column) of the Row whose label is `labelText`. */
function controlOf(labelText: string): HTMLElement {
  const labelDiv = screen.getByText(labelText);
  const rowRoot = labelDiv.parentElement!.parentElement!;
  return rowRoot.children[1] as HTMLElement;
}

/** Assert `set_app_settings` was called, and return the settings it was sent. */
async function savedSettings(calls: Call[]): Promise<AppSettings> {
  let sent: AppSettings | undefined;
  await waitFor(() => {
    const save = calls.find((c) => c.cmd === 'set_app_settings');
    expect(save).toBeTruthy();
    sent = save!.args.settings as AppSettings;
  });
  return sent!;
}

describe('SettingsScreen', () => {
  beforeEach(() => {
    useSettingsStore.setState({
      settings: null,
      status: 'idle',
      error: undefined,
    });
    profilesMock.setDefault.mockClear();
    profilesMock.init.mockClear();
  });
  afterEach(() => clearMocks());

  it('FR: persists a 3-way theme change (System included) and drives setMode', async () => {
    // Backend theme matches the passed mode so the load reconcile is a no-op.
    const initial: AppSettings = {
      ...fixtureAppSettings,
      app: { ...fixtureAppSettings.app, theme: 'light' },
    };
    const calls = mockSettings({ initial });
    const setMode = vi.fn();
    renderSettings({ mode: 'light', setMode });
    await waitReady();

    fireEvent.click(screen.getByText('System'));

    expect(setMode).toHaveBeenCalledWith('system');
    const sent = await savedSettings(calls);
    expect(sent.app.theme).toBe('system');
  });

  it('persists a 2-way theme change via setTheme when no mode is supplied', async () => {
    const calls = mockSettings();
    const { setTheme } = renderSettings({ theme: 'light' });
    await waitReady();

    fireEvent.click(screen.getByText('Dark'));

    expect(setTheme).toHaveBeenCalledWith('dark');
    const sent = await savedSettings(calls);
    expect(sent.app.theme).toBe('dark');
  });

  it('dual-writes a density change (setter + backend)', async () => {
    const calls = mockSettings();
    const { setDensity } = renderSettings({ density: 'comfortable' });
    await waitReady();

    fireEvent.click(screen.getByText('Compact'));

    expect(setDensity).toHaveBeenCalledWith('compact');
    const sent = await savedSettings(calls);
    expect(sent.app.density).toBe('compact');
  });

  it('FR: the default-profile select calls setDefault on the profiles store', async () => {
    mockSettings();
    renderSettings();
    await waitReady();

    const select = within(controlOf('Default profile')).getByRole('combobox');
    fireEvent.change(select, { target: { value: 'builtin-duplex-bw' } });

    expect(profilesMock.setDefault).toHaveBeenCalledWith('builtin-duplex-bw');
  });

  it('persists an update channel change', async () => {
    const calls = mockSettings();
    renderSettings();
    await waitReady();

    fireEvent.click(screen.getByText('Beta'));

    const sent = await savedSettings(calls);
    expect(sent.updater.channel).toBe('beta');
  });

  it('persists the check-on-startup toggle', async () => {
    const calls = mockSettings();
    renderSettings();
    await waitReady();

    fireEvent.click(controlOf('Check on startup').querySelector('label')!);

    const sent = await savedSettings(calls);
    expect(sent.updater.check_on_startup).toBe(false);
  });

  it('persists the crash-reports consent toggle', async () => {
    const calls = mockSettings();
    renderSettings();
    await waitReady();

    fireEvent.click(controlOf('Crash reports').querySelector('label')!);

    const sent = await savedSettings(calls);
    expect(sent.telemetry.crash_reports).toBe(true);
  });

  it('persists the anonymous-usage-metrics consent toggle', async () => {
    const calls = mockSettings();
    renderSettings();
    await waitReady();

    fireEvent.click(
      controlOf('Anonymous usage metrics').querySelector('label')!,
    );

    const sent = await savedSettings(calls);
    expect(sent.telemetry.anonymous_usage_metrics).toBe(true);
  });

  it('FR: an in-bounds engine value commits on blur and persists', async () => {
    const calls = mockSettings();
    renderSettings();
    await waitReady();

    const input = within(controlOf('Concurrent conversions')).getByRole(
      'textbox',
    );
    fireEvent.change(input, { target: { value: '4' } });
    fireEvent.focusOut(input);

    const sent = await savedSettings(calls);
    expect(sent.engine.max_concurrent_conversions).toBe(4);
  });

  it('FR: an out-of-bounds engine value shows an inline error and does NOT persist', async () => {
    const calls = mockSettings();
    renderSettings();
    await waitReady();

    const input = within(controlOf('Concurrent conversions')).getByRole(
      'textbox',
    );
    fireEvent.change(input, { target: { value: '99' } });
    fireEvent.focusOut(input);

    expect(
      await screen.findByText('Enter a whole number from 1 to 8.'),
    ).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'set_app_settings')).toBe(false);
  });

  it('accepts 0 for the conversion timeout (disables it)', async () => {
    const calls = mockSettings();
    renderSettings();
    await waitReady();

    const input = within(controlOf('Conversion timeout')).getByRole('textbox');
    fireEvent.change(input, { target: { value: '0' } });
    fireEvent.focusOut(input);

    const sent = await savedSettings(calls);
    expect(sent.engine.conversion_timeout_seconds).toBe(0);
  });

  it('renders the M2/M4 milestone notes and the consent note', async () => {
    mockSettings();
    renderSettings();
    await waitReady();

    expect(screen.getByText(/auto-updater ships \(M4\)/)).toBeInTheDocument();
    expect(
      screen.getByText(/these switches record consent only/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/shell integration milestone \(M2\)/),
    ).toBeInTheDocument();
  });

  it('drops the old shell-integration switches', async () => {
    mockSettings();
    renderSettings();
    await waitReady();

    expect(screen.queryByText('Right-click menu')).not.toBeInTheDocument();
    expect(
      screen.queryByText('Filter unsupported files'),
    ).not.toBeInTheDocument();
  });

  it('disables backend rows but keeps appearance working when unavailable', async () => {
    // No Tauri host: drop the internals a prior mockIPC may have left behind.
    const w = window as unknown as {
      __TAURI_INTERNALS__?: unknown;
      __TAURI__?: unknown;
    };
    delete w.__TAURI_INTERNALS__;
    delete w.__TAURI__;

    const { setTheme } = renderSettings({ theme: 'light' });

    await waitFor(() =>
      expect(useSettingsStore.getState().status).toBe('unavailable'),
    );
    expect(
      screen.getByText(/only appearance is adjustable/),
    ).toBeInTheDocument();

    // A backend row is visually + interactively disabled.
    expect(controlOf('Crash reports').style.pointerEvents).toBe('none');

    // Appearance stays enabled offline (local-only, nothing to revert).
    expect(controlOf('Theme').style.pointerEvents).not.toBe('none');
    fireEvent.click(screen.getByText('Dark'));
    expect(setTheme).toHaveBeenCalledWith('dark');
  });

  it('E: disables the appearance controls while the settings are loading', () => {
    // Preset 'loading' so the mount init() short-circuits and the status holds.
    useSettingsStore.setState({ status: 'loading', settings: null });
    mockSettings();
    renderSettings({ mode: 'light', setMode: vi.fn() });

    expect(useSettingsStore.getState().status).toBe('loading');
    // Theme + Density are dimmed and non-interactive so a change can't be
    // silently reverted by the arriving backend snapshot.
    expect(controlOf('Theme').style.pointerEvents).toBe('none');
    expect(controlOf('Density').style.pointerEvents).toBe('none');
  });
});
