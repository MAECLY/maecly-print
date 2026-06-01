import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, afterEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import { PrintersScreen } from './PrintersScreen';
import { Toaster } from '@/components/AppShell/Toaster';
import { fixturePrinters, fixtureDiscovered } from '@/test/fixtures';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

/** Install IPC mocks and return the list of commands invoked. */
function mockPrinters(
  opts: { prefs?: unknown; fail?: string; discovered?: unknown } = {},
): Call[] {
  const calls: Call[] = [];
  let prefs: unknown = opts.prefs ?? {};
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    if (cmd === 'list_printers') return fixturePrinters;
    if (cmd === 'default_printer') return 'EPSON_L15160';
    if (cmd === 'discover_printers')
      return opts.discovered ?? fixtureDiscovered;
    if (cmd === 'get_printer_prefs') return prefs;
    if (cmd === 'set_printer_prefs') {
      prefs = (args as { prefs: unknown }).prefs;
      return prefs;
    }
    return null;
  });
  return calls;
}

/** Render the screen with a mounted Toaster so toast output is queryable. */
function renderScreen() {
  return render(
    <>
      <PrintersScreen />
      <Toaster theme="light" />
    </>,
  );
}

describe('PrintersScreen', () => {
  afterEach(() => clearMocks());

  it('renders real printers with capabilities, and degrades when data is absent', async () => {
    mockPrinters();
    render(<PrintersScreen />);

    await waitFor(() =>
      expect(screen.getAllByText(/EPSON L15160 Series/).length).toBeGreaterThan(
        1,
      ),
    );
    expect(screen.getByText(/2 configured/)).toBeInTheDocument();
    expect(screen.getByText(/Duplex/)).toBeInTheDocument();
    expect(screen.getByText(/supply levels/i)).toBeInTheDocument();

    fireEvent.click(screen.getByText('Brother HL-L2350DW'));
    await waitFor(() =>
      expect(screen.queryByText(/supply levels/i)).not.toBeInTheDocument(),
    );
  });

  it('shows an unavailable state outside the Tauri host (no IPC)', async () => {
    render(<PrintersScreen />);
    await waitFor(() =>
      expect(screen.getByText(/Printers unavailable/i)).toBeInTheDocument(),
    );
  });

  it('removes (soft-disables) a printer, then restores it', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText(/^Remove$/));
    // Persisted with the printer in the disabled list.
    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      expect(save).toBeTruthy();
      expect((save!.args.prefs as { disabled: string[] }).disabled).toContain(
        'EPSON_L15160',
      );
    });
    // Header now offers Restore, and a success toast appears.
    expect(await screen.findByText(/^Restore$/)).toBeInTheDocument();
    expect(
      await screen.findByText(/Removed EPSON L15160 Series from MaeclyPrint/),
    ).toBeInTheDocument();
  });

  it('confirms via an in-app dialog before sending a test page', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Test page'));
    // An in-app modal appears (window.confirm is a no-op in WKWebView).
    expect(
      await screen.findByText(/Send a test page to EPSON L15160 Series/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Send test page' }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'print_test_page')).toBe(true),
    );
  });

  it('cancels the test page without printing', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Test page'));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));

    await waitFor(() =>
      expect(screen.queryByText(/Send a test page to/)).not.toBeInTheDocument(),
    );
    expect(calls.some((c) => c.cmd === 'print_test_page')).toBe(false);
  });

  it('sets a non-default printer as default via IPC', async () => {
    const calls = mockPrinters();
    render(<PrintersScreen />);
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Brother HL-L2350DW'));
    fireEvent.click(screen.getByText('Set default'));

    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'set_default_printer')).toBe(true);
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      expect(
        (save?.args.prefs as { default_printer?: string }).default_printer,
      ).toBe('HL_L2350');
    });
  });

  it('adds a file-type routing rule and persists it', async () => {
    const calls = mockPrinters();
    render(<PrintersScreen />);
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText(/add rule/i));
    const input = screen.getByPlaceholderText('.ext');
    fireEvent.change(input, { target: { value: 'pdf' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      expect(
        (save?.args.prefs as { routes: Record<string, string> }).routes['.pdf'],
      ).toBe('EPSON_L15160');
    });
  });

  it('surfaces an error toast when an action fails', async () => {
    mockPrinters({ fail: 'set_printer_prefs' });
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText(/^Remove$/));
    expect(await screen.findByText(/boom/)).toBeInTheDocument();
  });

  it('opens OS printer settings from the add-printer modal', async () => {
    const calls = mockPrinters({ discovered: [] });
    render(<PrintersScreen />);
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Add printer'));
    fireEvent.click(
      await screen.findByRole('button', { name: /open printer settings/i }),
    );

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'open_printer_settings')).toBe(true),
    );
  });

  it('discovers network printers in the add-printer modal', async () => {
    mockPrinters();
    render(<PrintersScreen />);
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Add printer'));
    expect(await screen.findByText('Canon TR8600')).toBeInTheDocument();
  });

  it('shows the empty state when no printers are installed', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_printers') return [];
      if (cmd === 'default_printer') return null;
      if (cmd === 'get_printer_prefs') return {};
      return null;
    });
    render(<PrintersScreen />);
    await waitFor(() =>
      expect(screen.getByText(/No printers installed/i)).toBeInTheDocument(),
    );
  });
});
