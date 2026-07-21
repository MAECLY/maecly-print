import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, afterEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import { PrintersScreen } from './PrintersScreen';
import { Toaster } from '@/components/AppShell/Toaster';
import {
  fixturePrinters,
  fixturePrintersWithVirtual,
  fixtureDiscovered,
} from '@/test/fixtures';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

interface MockOpts {
  prefs?: unknown;
  fail?: string;
  discovered?: unknown;
  printers?: unknown;
  policy?: unknown;
  /** When set, `add_printer` throws with this message. */
  addError?: string;
}

/** Install IPC mocks and return the list of commands invoked. */
function mockPrinters(opts: MockOpts = {}): Call[] {
  const calls: Call[] = [];
  let prefs: unknown = opts.prefs ?? {};
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    if (cmd === 'add_printer') {
      if (opts.addError) throw new Error(opts.addError);
      return null;
    }
    if (cmd === 'list_printers') return opts.printers ?? fixturePrinters;
    if (cmd === 'default_printer') return 'EPSON_L15160';
    if (cmd === 'discover_printers')
      return opts.discovered ?? fixtureDiscovered;
    if (cmd === 'get_printer_prefs') return prefs;
    if (cmd === 'set_printer_prefs') {
      prefs = (args as { prefs: unknown }).prefs;
      return prefs;
    }
    if (cmd === 'get_printer_policy')
      return opts.policy ?? { default_printer: null };
    return null;
  });
  return calls;
}

/** Open the add-printer modal and return its dialog element. */
async function openAddModal(): Promise<HTMLElement> {
  fireEvent.click(screen.getByText('Add printer'));
  return screen.findByRole('dialog');
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

  it('shows three add-printer tabs and switches their content', async () => {
    mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    expect(
      within(dialog).getByRole('radio', { name: 'Scan network' }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole('radio', { name: 'By IP address' }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole('radio', { name: 'From system' }),
    ).toBeInTheDocument();

    // Scan network is the default tab (discovery running).
    expect(await within(dialog).findByText('Canon TR8600')).toBeInTheDocument();

    // By IP address swaps in the manual-entry form.
    fireEvent.click(
      within(dialog).getByRole('radio', { name: 'By IP address' }),
    );
    expect(
      within(dialog).getByPlaceholderText(/10\.0\.0\.41/),
    ).toBeInTheDocument();

    // From system swaps in the soft-disable explainer + queue checkboxes.
    fireEvent.click(within(dialog).getByRole('radio', { name: 'From system' }));
    expect(within(dialog).getByText(/soft-disable/i)).toBeInTheDocument();
  });

  it('adds a printer by IP address with a built device URI', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(
      within(dialog).getByRole('radio', { name: 'By IP address' }),
    );

    fireEvent.change(within(dialog).getByPlaceholderText(/10\.0\.0\.41/), {
      target: { value: '10.0.0.41' },
    });
    fireEvent.change(
      within(dialog).getByPlaceholderText('Front desk printer'),
      {
        target: { value: 'Office IPP' },
      },
    );

    const connect = within(dialog).getByRole('button', { name: 'Connect' });
    expect(connect).not.toBeDisabled();
    fireEvent.click(connect);

    await waitFor(() => {
      const add = calls.find((c) => c.cmd === 'add_printer');
      expect(add).toBeTruthy();
      expect(add!.args).toEqual({
        name: 'Office IPP',
        uri: 'ipp://10.0.0.41:631/ipp/print',
      });
    });
    // onAdded re-runs load(), so list_printers is invoked again.
    expect(
      calls.filter((c) => c.cmd === 'list_printers').length,
    ).toBeGreaterThan(1);
  });

  it('keeps Connect disabled for an invalid host and never calls add_printer', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(
      within(dialog).getByRole('radio', { name: 'By IP address' }),
    );

    fireEvent.change(within(dialog).getByPlaceholderText(/10\.0\.0\.41/), {
      target: { value: 'bad host' },
    });

    const connect = within(dialog).getByRole('button', { name: 'Connect' });
    expect(connect).toBeDisabled();
    fireEvent.click(connect);
    expect(calls.some((c) => c.cmd === 'add_printer')).toBe(false);
  });

  it('adds a discovered printer via add_printer, not open_printer_settings', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    await within(dialog).findByText('Canon TR8600');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Add' }));

    await waitFor(() => {
      const add = calls.find((c) => c.cmd === 'add_printer');
      expect(add).toBeTruthy();
      expect(add!.args).toEqual({
        name: 'Canon TR8600',
        uri: 'ipp://canon.local/ipp/print',
      });
    });
    expect(calls.some((c) => c.cmd === 'open_printer_settings')).toBe(false);
  });

  it('soft-disables and re-enables a queue from the system-spooler tab', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('radio', { name: 'From system' }));

    // Untick Brother — persisted into `disabled`.
    const brother = within(dialog).getByText('Brother HL-L2350DW');
    fireEvent.click(brother.closest('label')!);
    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      expect((save!.args.prefs as { disabled: string[] }).disabled).toContain(
        'HL_L2350',
      );
    });

    // Re-tick it — removed from `disabled` again.
    fireEvent.click(
      within(dialog).getByText('Brother HL-L2350DW').closest('label')!,
    );
    await waitFor(() => {
      const saves = calls.filter((c) => c.cmd === 'set_printer_prefs');
      const last = saves[saves.length - 1];
      expect(
        (last!.args.prefs as { disabled: string[] }).disabled,
      ).not.toContain('HL_L2350');
    });
  });

  it('reconciles a missing printer: reassign rewrites routes and drops overrides', async () => {
    const calls = mockPrinters({
      prefs: {
        routes: { '.pdf': 'GONE' },
        overrides: { GONE: { tray: 'Tray 2' } },
      },
    });
    renderScreen();
    await screen.findByText(/2 configured/);

    // Banner names the missing id.
    expect(
      await screen.findByText(/no longer\s+available/i),
    ).toBeInTheDocument();
    expect(screen.getByText('GONE')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Update references'));
    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      const prefs = save!.args.prefs as {
        routes: Record<string, string>;
        overrides: Record<string, unknown>;
      };
      expect(prefs.routes['.pdf']).toBe('EPSON_L15160');
      expect(prefs.overrides.GONE).toBeUndefined();
    });
  });

  it('reconciles a missing printer: Forget removes every reference', async () => {
    const calls = mockPrinters({
      prefs: {
        routes: { '.pdf': 'GONE' },
        overrides: { GONE: { tray: 'Tray 2' } },
      },
    });
    renderScreen();
    await screen.findByText(/2 configured/);
    await screen.findByText('GONE');

    fireEvent.click(screen.getByText('Forget'));
    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      const prefs = save!.args.prefs as {
        routes: Record<string, string>;
        overrides: Record<string, unknown>;
      };
      expect(prefs.routes['.pdf']).toBeUndefined();
      expect(prefs.overrides.GONE).toBeUndefined();
    });
  });

  it('reconciles a missing printer: Keep paused hides the banner without saving', async () => {
    const calls = mockPrinters({ prefs: { routes: { '.pdf': 'GONE' } } });
    render(<PrintersScreen />);
    await screen.findByText(/2 configured/);
    await screen.findByText('GONE');

    fireEvent.click(screen.getByText('Keep paused'));
    await waitFor(() =>
      expect(screen.queryByText('GONE')).not.toBeInTheDocument(),
    );
    expect(calls.some((c) => c.cmd === 'set_printer_prefs')).toBe(false);
  });

  it('locks the default printer under org policy', async () => {
    mockPrinters({ policy: { default_printer: 'HL_L2350' } });
    render(<PrintersScreen />);
    await screen.findByText(/2 configured/);

    // Org note references the locked printer by name.
    const note = await screen.findByText(/managed by your organization/i);
    expect(note).toHaveTextContent('Brother HL-L2350DW');

    // Set default is read-only while EPSON (a non-default) is selected.
    expect(screen.getByText('Set default').closest('button')).toBeDisabled();

    // The effective default star follows policy: selecting Brother shows the
    // header default badge.
    fireEvent.click(screen.getByRole('button', { name: /Brother HL-L2350DW/ }));
    expect(await screen.findByText('default')).toBeInTheDocument();
  });

  it('hides Remove for a virtual printer but keeps Restore working', async () => {
    const calls = mockPrinters({
      printers: fixturePrintersWithVirtual,
      prefs: { disabled: ['SAVE_AS_PDF'] },
    });
    renderScreen();
    await screen.findByText(/3 configured/);

    fireEvent.click(screen.getByText('Save as PDF'));

    // Virtual printers are never removable.
    expect(screen.queryByText(/^Remove$/)).not.toBeInTheDocument();

    // But a soft-disabled one can still be restored.
    fireEvent.click(await screen.findByText(/^Restore$/));
    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      expect(
        (save!.args.prefs as { disabled: string[] }).disabled,
      ).not.toContain('SAVE_AS_PDF');
    });
  });
});
