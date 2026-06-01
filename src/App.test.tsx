import {
  render,
  screen,
  fireEvent,
  within,
  waitFor,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import App from './App';
import { fixturePrinters } from '@/test/fixtures';

describe('App', () => {
  beforeEach(() => {
    // Skip onboarding modal so the main shell is visible.
    window.localStorage.setItem('maeclyprint:onboarding-complete', '1');
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
});
