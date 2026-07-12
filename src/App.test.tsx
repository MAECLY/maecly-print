import {
  render,
  screen,
  fireEvent,
  within,
  waitFor,
  act,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import App from './App';
import {
  fixtureAppSettings,
  fixturePrinters,
  fixtureProfiles,
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
    expect(
      screen.getByText(/Drop files or folders to print/i),
    ).toBeInTheDocument();
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
});
