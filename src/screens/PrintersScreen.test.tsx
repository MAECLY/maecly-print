import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, afterEach, beforeEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import { PrintersScreen } from './PrintersScreen';
import { Toaster } from '@/components/AppShell/Toaster';
import { I18nProvider } from '@/i18n/I18nProvider';
import { useSettingsStore } from '@/stores/settingsStore';
import {
  fixtureAppInfo,
  fixtureAppSettings,
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
  /** `app_info.os` — the authoritative platform the screen branches on. */
  os?: string;
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
    if (cmd === 'app_info')
      return { ...fixtureAppInfo, os: opts.os ?? 'macos' };
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

/**
 * Render inside the i18n provider so every assertion runs against the real
 * catalog rather than literals baked into the component.
 */
function renderScreen(withToaster = true) {
  return render(
    <I18nProvider>
      <PrintersScreen />
      {withToaster && <Toaster theme="light" />}
    </I18nProvider>,
  );
}

/** The persisted prefs of the most recent `set_printer_prefs` call. */
function lastSaved(calls: Call[]): Record<string, unknown> | undefined {
  const saves = calls.filter((c) => c.cmd === 'set_printer_prefs');
  return saves[saves.length - 1]?.args.prefs as
    | Record<string, unknown>
    | undefined;
}

describe('PrintersScreen', () => {
  beforeEach(() => {
    // Pin the locale so the default-language assertions are deterministic.
    useSettingsStore.setState({ settings: fixtureAppSettings });
  });
  afterEach(() => clearMocks());

  // -- enumeration + degradation ----------------------------------------

  it('FR-044: renders real printers with capabilities, and degrades when data is absent', async () => {
    mockPrinters();
    renderScreen(false);

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

  it('FR-044: an inkjet and an unidentified printer are not labelled laser', async () => {
    mockPrinters({
      printers: [
        { ...fixturePrinters[0], kind: 'inkjet' },
        { ...fixturePrinters[1], kind: 'printer' },
      ],
    });
    renderScreen(false);
    await screen.findByText(/2 configured/);
    expect(screen.getAllByText(/Inkjet/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/· Printer/).length).toBeGreaterThan(0);
    expect(screen.queryByText(/Laser/)).not.toBeInTheDocument();
  });

  it('FR-044 regression: hides the Max resolution row when the driver reports no DPI', async () => {
    mockPrinters();
    renderScreen(false);
    await screen.findByText(/2 configured/);

    // EPSON reports 1200 dpi — the row is present and carries the value.
    expect(screen.getByText('Max resolution')).toBeInTheDocument();
    expect(screen.getByText('1200 dpi')).toBeInTheDocument();

    // Brother reports none (`max_dpi: null`): the row must disappear, not
    // render a stray " dpi" next to an empty value.
    fireEvent.click(screen.getByText('Brother HL-L2350DW'));
    await waitFor(() =>
      expect(screen.queryByText('Max resolution')).not.toBeInTheDocument(),
    );
    expect(screen.queryByText(/\bdpi\b/)).not.toBeInTheDocument();
  });

  it('FR-044 regression: says a location was not reported instead of leaving the cell blank', async () => {
    mockPrinters();
    renderScreen(false);
    await screen.findByText(/2 configured/);

    expect(screen.getByText('Front office')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Brother HL-L2350DW'));
    await waitFor(() =>
      expect(screen.getByText('Not reported')).toBeInTheDocument(),
    );
  });

  it('FR-044 regression: capability pills do not affirm zero papers or zero trays', async () => {
    const bare = [
      { ...fixturePrinters[0]!, papers: [], trays: [], supplies: {} },
    ];
    mockPrinters({ printers: bare });
    renderScreen(false);
    await screen.findByText(/1 configured/);

    // The negative pill state is the muted surface, not the success surface.
    const papers = screen.getByText('0 paper sizes');
    expect(papers.getAttribute('style')).toContain('--mp-surface2');
    const trays = screen.getByText('0 trays');
    expect(trays.getAttribute('style')).toContain('--mp-surface2');
  });

  it('FR-044: a single-tray printer counts as a capability, not a gap', async () => {
    mockPrinters();
    renderScreen(false);
    await screen.findByText(/2 configured/);

    // One tray is a real, present capability — it used to render as "not ok".
    const trays = screen.getByText('1 tray');
    expect(trays.getAttribute('style')).toContain('--mp-successSoft');
  });

  it('smoke: shows an unavailable state outside the Tauri host (no IPC)', async () => {
    renderScreen(false);
    await waitFor(() =>
      expect(screen.getByText(/Printers unavailable/i)).toBeInTheDocument(),
    );
  });

  it('smoke: shows the empty state when no printers are installed', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_printers') return [];
      if (cmd === 'default_printer') return null;
      if (cmd === 'get_printer_prefs') return {};
      if (cmd === 'app_info') return fixtureAppInfo;
      return null;
    });
    renderScreen(false);
    await waitFor(() =>
      expect(screen.getByText(/No printers installed/i)).toBeInTheDocument(),
    );
  });

  // -- header actions -----------------------------------------------------

  it('FR-044: removes (soft-disables) a printer, then restores it', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText(/^Remove$/));
    // It is the system default, so removing it is confirmed first.
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove' }));
    // Persisted with the printer in the disabled list.
    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'set_printer_prefs');
      expect(save).toBeTruthy();
      expect((save!.args.prefs as { disabled: string[] }).disabled).toContain(
        'EPSON_L15160',
      );
    });
    // Header now offers Restore, and a success toast naming the real OS.
    expect(await screen.findByText(/^Restore$/)).toBeInTheDocument();
    expect(
      await screen.findByText(
        /Removed EPSON L15160 Series from Pluryprint \(still installed in macOS\)/,
      ),
    ).toBeInTheDocument();
  });

  it('NFR-032: confirms via an in-app dialog before sending a test page', async () => {
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

  it('NFR-033: the test-page toast does not claim more than the backend can verify', async () => {
    mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Test page'));
    fireEvent.click(
      await screen.findByRole('button', { name: 'Send test page' }),
    );

    // The Windows fallback path cannot observe whether the spooler accepted
    // the page, so the copy tells the user where to look instead.
    expect(
      await screen.findByText(/if nothing comes out, check the printer/i),
    ).toBeInTheDocument();
  });

  it("FR-092: a refused test page says why in the app's words, with the tool's own line under it", async () => {
    mockPrinters();
    // Override print_test_page to fail the way the CUPS backend reports it.
    mockIPC((cmd) => {
      if (cmd === 'print_test_page') {
        // A plain object is the command-returned error branch.
        throw {
          code: 'PrintFailed',
          reason: 'spooler_rejected',
          params: { detail: 'lp: The printer or class does not exist.' },
          message: 'lp: The printer or class does not exist.',
        };
      }
      if (cmd === 'app_info') return { ...fixtureAppInfo, os: 'macos' };
      if (cmd === 'list_printers') return fixturePrinters;
      if (cmd === 'default_printer') return 'EPSON_L15160';
      if (cmd === 'get_printer_prefs') return {};
      if (cmd === 'get_printer_policy') return { default_printer: null };
      return null;
    });
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Test page'));
    fireEvent.click(
      await screen.findByRole('button', { name: 'Send test page' }),
    );

    expect(
      await screen.findByText('The system print service rejected the job.'),
    ).toBeInTheDocument();
    expect(
      await screen.findByText('lp: The printer or class does not exist.'),
    ).toBeInTheDocument();
  });

  it('NFR-032: cancels the test page without printing', async () => {
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

  it('FR-044: sets a non-default printer as default via IPC', async () => {
    const calls = mockPrinters();
    renderScreen(false);
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

  it('NFR-033: surfaces an error toast when an action fails', async () => {
    mockPrinters({ fail: 'set_printer_prefs' });
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText(/^Remove$/));
    fireEvent.click(
      within(await screen.findByRole('dialog')).getByRole('button', {
        name: 'Remove',
      }),
    );
    expect(await screen.findByText(/boom/)).toBeInTheDocument();
  });

  it('FR-044: removing the printer jobs land on says where they go first', async () => {
    const calls = mockPrinters({
      prefs: { default_printer: 'HL_L2350', routes: { '.pdf': 'HL_L2350' } },
    });
    renderScreen();
    await screen.findByText(/2 configured/);
    fireEvent.click(screen.getByRole('button', { name: /Brother HL-L2350DW/ }));

    fireEvent.click(await screen.findByText(/^Remove$/));
    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent(
      'Remove Brother HL-L2350DW from Pluryprint?',
    );
    expect(dialog).toHaveTextContent(/Pluryprint's default printer/);
    expect(dialog).toHaveTextContent(/Files of type \.pdf are routed to it/);
    // Nothing is written until the user confirms.
    expect(calls.some((c) => c.cmd === 'set_printer_prefs')).toBe(false);

    fireEvent.click(within(dialog).getByRole('button', { name: 'Remove' }));
    await waitFor(() => {
      const saved = lastSaved(calls) as {
        disabled: string[];
        default_printer: string | null;
      };
      expect(saved.disabled).toContain('HL_L2350');
      expect(saved.default_printer).toBeNull();
    });
    // The routes stay, flagged as skipped while it is removed.
    expect(
      await screen.findByText(
        /These rules are skipped while the printer is removed/,
      ),
    ).toBeInTheDocument();
  });

  it('FR-044: a printer nothing depends on is removed without a question', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);
    fireEvent.click(screen.getByRole('button', { name: /Brother HL-L2350DW/ }));

    fireEvent.click(await screen.findByText(/^Remove$/));
    expect(screen.queryByRole('dialog')).toBeNull();
    await waitFor(() =>
      expect(lastSaved(calls)?.disabled).toContain('HL_L2350'),
    );
  });

  // -- per-printer defaults ----------------------------------------------

  it('FR-044: writes a per-printer tray override and can clear it again', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const tray = screen.getByLabelText('Default tray');
    // The clearing option exists and is what an un-overridden printer shows.
    expect(within(tray).getByText(/Printer default/)).toBeInTheDocument();
    expect((tray as HTMLSelectElement).value).toBe('');

    fireEvent.change(tray, { target: { value: 'Tray 1' } });
    await waitFor(() => {
      const overrides = lastSaved(calls)?.overrides as Record<
        string,
        { tray?: string }
      >;
      expect(overrides.EPSON_L15160?.tray).toBe('Tray 1');
    });

    // Back to "Printer default": the key must be REMOVED, not stored as "",
    // which the CUPS test page would turn into a bare `-o media=`.
    fireEvent.change(screen.getByLabelText('Default tray'), {
      target: { value: '' },
    });
    await waitFor(() => {
      const overrides = lastSaved(calls)?.overrides as Record<string, unknown>;
      expect(overrides.EPSON_L15160).toBeUndefined();
    });
    expect(
      await screen.findByText(/Default tray now follows the printer/),
    ).toBeInTheDocument();
  });

  it('FR-044: replaces the defaults select with a note when the driver lists nothing', async () => {
    mockPrinters({
      printers: [{ ...fixturePrinters[0]!, papers: [], trays: [] }],
    });
    renderScreen(false);
    await screen.findByText(/1 configured/);

    expect(screen.queryByLabelText('Default tray')).not.toBeInTheDocument();
    expect(
      screen.getByText('This driver reports no selectable trays.'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('This driver reports no paper sizes.'),
    ).toBeInTheDocument();
  });

  // -- file-type routing --------------------------------------------------

  it('FR-044: adds a file-type routing rule and persists it', async () => {
    const calls = mockPrinters();
    renderScreen(false);
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

  // -- add-printer modal --------------------------------------------------

  it('FR-044: opens OS printer settings from the add-printer modal', async () => {
    const calls = mockPrinters({ discovered: [] });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Add printer'));
    fireEvent.click(
      await screen.findByRole('button', { name: /open printer settings/i }),
    );

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'open_printer_settings')).toBe(true),
    );
  });

  it('NFR-041: the add-printer dialog takes focus, closes on Escape and gives focus back', async () => {
    mockPrinters({ discovered: [] });
    renderScreen(false);
    await screen.findByText(/2 configured/);
    const trigger = screen.getByText('Add printer').closest('button')!;
    trigger.focus();

    fireEvent.click(trigger);
    const dialog = await screen.findByRole('dialog', {
      name: 'Add a printer',
    });
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    expect(document.activeElement).toHaveAttribute('role', 'tab');

    fireEvent.keyDown(document, { key: 'Escape' });
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    );
    expect(trigger).toHaveFocus();
  });

  it('NFR-043/NFR-045: names the by-IP fields and marks the selected printer', async () => {
    mockPrinters({ discovered: [] });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    expect(
      screen
        .getAllByRole('button')
        .filter((b) => b.hasAttribute('aria-current')),
    ).toHaveLength(1);

    fireEvent.click(screen.getByText('Add printer'));
    fireEvent.click(await screen.findByRole('tab', { name: /ip/i }));
    for (const name of [/host/i, /queue/i, /name/i]) {
      expect(screen.getByRole('textbox', { name })).toBeInTheDocument();
    }
    expect(
      screen.getByRole('spinbutton', { name: /port/i }),
    ).toBeInTheDocument();
  });

  it('FR-044: discovers network printers in the add-printer modal', async () => {
    mockPrinters();
    renderScreen(false);
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Add printer'));
    expect(await screen.findByText('Canon TR8600')).toBeInTheDocument();
  });

  it('NFR-043: the add-printer methods are a named tablist wired to their panels', async () => {
    mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    // These controls swap the visible panel, so they are tabs — not radios.
    const tablist = within(dialog).getByRole('tablist', {
      name: 'How to add the printer',
    });
    expect(tablist).toBeInTheDocument();
    expect(within(dialog).queryAllByRole('radio')).toHaveLength(0);

    const tabs = within(tablist).getAllByRole('tab');
    expect(tabs.map((el) => el.textContent)).toEqual([
      'Scan network',
      'By IP address',
      'From system',
    ]);

    const selected = within(dialog).getByRole('tab', { selected: true });
    expect(selected).toHaveTextContent('Scan network');
    const panel = within(dialog).getByRole('tabpanel');
    expect(selected.getAttribute('aria-controls')).toBe(panel.id);
    expect(panel.getAttribute('aria-labelledby')).toBe(selected.id);
  });

  it('FR-044: switching tabs swaps the panel content', async () => {
    mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    // Scan network is the default tab (discovery running).
    expect(await within(dialog).findByText('Canon TR8600')).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole('tab', { name: 'By IP address' }));
    expect(
      within(dialog).getByPlaceholderText(/10\.0\.0\.41/),
    ).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole('tab', { name: 'From system' }));
    expect(within(dialog).getByText(/soft-disable/i)).toBeInTheDocument();
  });

  it('NFR-041: arrow keys move between the add-printer tabs', async () => {
    mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    const first = within(dialog).getByRole('tab', { name: 'Scan network' });
    // A tablist is one tab stop: only the selected tab is reachable by Tab.
    expect(first).toHaveAttribute('tabindex', '0');
    expect(
      within(dialog).getByRole('tab', { name: 'By IP address' }),
    ).toHaveAttribute('tabindex', '-1');

    fireEvent.keyDown(first, { key: 'ArrowRight' });
    expect(
      within(dialog).getByRole('tab', { selected: true }),
    ).toHaveTextContent('By IP address');

    fireEvent.keyDown(
      within(dialog).getByRole('tab', { name: 'By IP address' }),
      { key: 'ArrowLeft' },
    );
    expect(
      within(dialog).getByRole('tab', { selected: true }),
    ).toHaveTextContent('Scan network');
  });

  it('FR-044: adds a printer by IP address with a built device URI', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('tab', { name: 'By IP address' }));

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

  it('FR-042: keeps Connect disabled for an invalid host and never calls add_printer', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('tab', { name: 'By IP address' }));

    fireEvent.change(within(dialog).getByPlaceholderText(/10\.0\.0\.41/), {
      target: { value: 'bad host' },
    });

    const connect = within(dialog).getByRole('button', { name: 'Connect' });
    expect(connect).toBeDisabled();
    fireEvent.click(connect);
    expect(calls.some((c) => c.cmd === 'add_printer')).toBe(false);
  });

  it('NFR-043: an invalid host says why, and the fields announce it', async () => {
    mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('tab', { name: 'By IP address' }));
    const host = within(dialog).getByRole('textbox', {
      name: 'Host / IP address',
    });
    fireEvent.change(host, { target: { value: '999.1.1.1' } });

    expect(host).toHaveAttribute('aria-invalid', 'true');
    expect(host).toHaveAccessibleDescription(/Enter an IPv4 address/);

    const port = within(dialog).getByRole('spinbutton', { name: 'Port' });
    expect(port).toHaveAttribute('min', '1');
    expect(port).toHaveAttribute('max', '65535');
    fireEvent.change(port, { target: { value: '70000' } });
    expect(port).toHaveAccessibleDescription(/from 1 to 65535/);
  });

  it('FR-044: adds a discovered printer via add_printer, not open_printer_settings', async () => {
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

  it('FR-044: soft-disables and re-enables a queue from the system-spooler tab', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('tab', { name: 'From system' }));

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
    await waitFor(() =>
      expect(lastSaved(calls)?.disabled).not.toContain('HL_L2350'),
    );
  });

  // -- discovery honesty --------------------------------------------------

  it('FR-044 regression: a failed scan is reported as a failure, not an empty network', async () => {
    mockPrinters({ fail: 'discover_printers' });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    expect(
      await within(dialog).findByText(/Network scan failed: boom/),
    ).toBeInTheDocument();
    expect(
      within(dialog).queryByText('No new printers found on the network.'),
    ).not.toBeInTheDocument();
  });

  it('FR-044: on Windows the screen says discovery is unavailable instead of faking a scan', async () => {
    const calls = mockPrinters({ os: 'windows' });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    expect(
      await within(dialog).findByText(/Network discovery is not available/i),
    ).toBeInTheDocument();
    // No spinner, no "nothing found", and above all no pointless IPC round-trip.
    expect(
      within(dialog).queryByText(/Scanning the network/i),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).queryByText('No new printers found on the network.'),
    ).not.toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'discover_printers')).toBe(false);
  });

  it('FR-044: Windows offers IPP/IPPS only, from the backend-reported OS', async () => {
    mockPrinters({ os: 'windows' });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('tab', { name: 'By IP address' }));

    const proto = within(dialog).getByLabelText('Protocol');
    expect(
      within(proto)
        .getAllByRole('option')
        .map((o) => o.textContent),
    ).toEqual(['IPP', 'IPPS (secure)']);
    expect(
      within(dialog).getByText(/Windows installs IPP \/ IPPS queues only/),
    ).toBeInTheDocument();
  });

  it('FR-044: macOS offers the raw and LPD transports too', async () => {
    mockPrinters({ os: 'macos' });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('tab', { name: 'By IP address' }));

    const proto = within(dialog).getByLabelText('Protocol');
    expect(within(proto).getAllByRole('option')).toHaveLength(4);
    expect(
      within(dialog).queryByText(/Windows installs IPP/),
    ).not.toBeInTheDocument();
  });

  // -- missing-printer reconciliation -------------------------------------

  it('FR-044: reconciles a missing printer: reassign rewrites routes and drops overrides', async () => {
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

  it('FR-044 regression: the banner says jobs are paused, not failed', async () => {
    mockPrinters({ prefs: { routes: { '.pdf': 'GONE' } } });
    renderScreen(false);
    await screen.findByText(/2 configured/);
    await screen.findByText('GONE');

    // The dispatcher pauses on PrintError::PrinterMissing (not terminal), so
    // the copy must not promise a failure.
    expect(screen.getByText('paused, not failed,')).toBeInTheDocument();
    expect(screen.queryByText(/will\s+fail/i)).not.toBeInTheDocument();
  });

  it('FR-044: reconciles a missing printer: Forget removes every reference', async () => {
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

  it('FR-044: reconciles a missing printer: Keep paused hides the banner without saving', async () => {
    const calls = mockPrinters({ prefs: { routes: { '.pdf': 'GONE' } } });
    renderScreen(false);
    await screen.findByText(/2 configured/);
    await screen.findByText('GONE');

    fireEvent.click(screen.getByText('Keep paused'));
    await waitFor(() =>
      expect(screen.queryByText('GONE')).not.toBeInTheDocument(),
    );
    expect(calls.some((c) => c.cmd === 'set_printer_prefs')).toBe(false);
  });

  it('FR-044 regression: Keep paused lasts only until the next Rescan', async () => {
    mockPrinters({ prefs: { routes: { '.pdf': 'GONE' } } });
    renderScreen(false);
    await screen.findByText('GONE');

    fireEvent.click(screen.getByText('Keep paused'));
    await waitFor(() =>
      expect(screen.queryByText('GONE')).not.toBeInTheDocument(),
    );

    fireEvent.click(screen.getByRole('button', { name: /Rescan/ }));
    expect(await screen.findByText('GONE')).toBeInTheDocument();
  });

  it('NFR-033 regression: Set default does not claim success when saving the app default fails', async () => {
    const calls = mockPrinters({ fail: 'set_printer_prefs' });
    renderScreen();
    await screen.findByText(/2 configured/);

    fireEvent.click(screen.getByText('Brother HL-L2350DW'));
    fireEvent.click(screen.getByText('Set default'));

    expect(
      await screen.findByText(
        /could not save it as its own default/,
        {},
        { timeout: 3000 },
      ),
    ).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === 'set_default_printer')).toBe(true);
    expect(screen.queryByText(/is now the default$/)).not.toBeInTheDocument();
  });

  it('FR-044 regression: hides the Input trays block when the driver reports no trays', async () => {
    mockPrinters({
      printers: fixturePrinters.map((p) =>
        p.id === 'HL_L2350' ? { ...p, trays: [] } : p,
      ),
    });
    renderScreen(false);
    await screen.findByText(/2 configured/);
    expect(screen.getByText('Input trays')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Brother HL-L2350DW'));
    await waitFor(() =>
      expect(screen.queryByText('Input trays')).not.toBeInTheDocument(),
    );
  });

  it('FR-044 regression: rejects an out-of-range port before calling add_printer', async () => {
    const calls = mockPrinters();
    renderScreen();
    await screen.findByText(/2 configured/);

    const dialog = await openAddModal();
    fireEvent.click(within(dialog).getByRole('tab', { name: 'By IP address' }));
    fireEvent.change(within(dialog).getByPlaceholderText(/10\.0\.0\.41/), {
      target: { value: '10.0.0.5' },
    });
    const portField = within(dialog).getByDisplayValue('631');
    const connect = within(dialog).getByRole('button', { name: 'Connect' });

    for (const bad of ['-5', '0', '70000', '631.5']) {
      fireEvent.change(portField, { target: { value: bad } });
      expect(connect).toBeDisabled();
      expect(
        within(dialog).getByText(/whole number from 1 to 65535/),
      ).toBeInTheDocument();
    }
    fireEvent.click(connect);
    expect(calls.some((c) => c.cmd === 'add_printer')).toBe(false);

    fireEvent.change(portField, { target: { value: '8631' } });
    expect(connect).not.toBeDisabled();
  });

  it('NFR-043: the reassign dropdown carries an accessible name', async () => {
    mockPrinters({ prefs: { routes: { '.pdf': 'GONE' } } });
    renderScreen(false);
    await screen.findByText(/2 configured/);
    await screen.findByText('GONE');

    expect(
      screen.getByLabelText("Printer to take over the missing printer's rules"),
    ).toBeInTheDocument();
  });

  // -- policy + virtual printers ------------------------------------------

  it('FR-044: locks the default printer under org policy', async () => {
    mockPrinters({ policy: { default_printer: 'HL_L2350' } });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    // Org note references the locked printer by name.
    const note = await screen.findByText(/managed by your organization/i);
    expect(note).toHaveTextContent('Brother HL-L2350DW');
    expect(note).toHaveTextContent('policy.json');

    // Set default is read-only while EPSON (a non-default) is selected.
    expect(screen.getByText('Set default').closest('button')).toBeDisabled();

    // The effective default star follows policy: selecting Brother shows the
    // header default badge.
    fireEvent.click(screen.getByRole('button', { name: /Brother HL-L2350DW/ }));
    expect(await screen.findByText('default')).toBeInTheDocument();
  });

  it('FR-044: a policy.json that names the display name still locks that printer', async () => {
    mockPrinters({ policy: { default_printer: 'Brother HL-L2350DW' } });
    renderScreen(false);
    await screen.findByText(/2 configured/);

    const note = await screen.findByText(/managed by your organization/i);
    expect(note).toHaveTextContent('Brother HL-L2350DW');
    fireEvent.click(screen.getByRole('button', { name: /Brother HL-L2350DW/ }));
    expect(await screen.findByText('default')).toBeInTheDocument();
  });

  it('FR-044: hides Remove for a virtual printer but keeps Restore working', async () => {
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
    await waitFor(() =>
      expect(lastSaved(calls)?.disabled).not.toContain('SAVE_AS_PDF'),
    );
  });

  it('FR-041: a virtual printer reads in user language, not codes or repeats', async () => {
    mockPrinters({ printers: fixturePrintersWithVirtual });
    renderScreen();
    await screen.findByText(/3 configured/);

    fireEvent.click(screen.getByText('Save as PDF'));

    expect(
      await screen.findByText('Virtual · 0 jobs in its queue'),
    ).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/Virtual · Virtual/);
    expect(document.body.textContent).not.toMatch(/DuplexNotSupported/);
    expect(
      screen.getByText(/two-sided printing will pause on this printer/),
    ).toBeInTheDocument();
  });

  // -- i18n (FR-092) ------------------------------------------------------

  it('FR-092: renders the screen in Spanish when the locale is es-ES', async () => {
    mockPrinters();
    useSettingsStore.setState({
      settings: {
        ...fixtureAppSettings,
        app: { ...fixtureAppSettings.app, locale: 'es-ES' },
      },
    });
    renderScreen(false);

    await screen.findByText(/2 configuradas · 2 listas/);
    expect(screen.getByText('Impresoras')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Reexplorar' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Conexión')).toBeInTheDocument();
    expect(screen.getByText('Controlador')).toBeInTheDocument();
    expect(screen.getByText('Resolución máxima')).toBeInTheDocument();
    expect(screen.getByText('1200 ppp')).toBeInTheDocument();
    expect(
      screen.getByText('Valores por defecto de esta impresora'),
    ).toBeInTheDocument();
    expect(screen.getByText('Niveles de consumibles')).toBeInTheDocument();
    expect(screen.getByText('1 bandeja')).toBeInTheDocument();
    expect(screen.getByText('3 tamaños de papel')).toBeInTheDocument();
  });

  it('FR-092: translates the add-printer modal too', async () => {
    mockPrinters();
    useSettingsStore.setState({
      settings: {
        ...fixtureAppSettings,
        app: { ...fixtureAppSettings.app, locale: 'es-ES' },
      },
    });
    renderScreen(false);
    await screen.findByText(/2 configuradas/);

    fireEvent.click(screen.getByText('Añadir impresora'));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByRole('tablist', { name: 'Cómo añadir la impresora' }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole('tab', { name: 'Explorar la red' }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole('button', {
        name: 'Abrir ajustes de impresoras',
      }),
    ).toBeInTheDocument();
  });
});
