import { useState } from 'react';
import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import { ProfilesScreen, exportFileName } from './ProfilesScreen';
import { Toaster } from '@/components/AppShell/Toaster';
import { useProfilesStore } from '@/stores/profilesStore';
import { fixturePrinters, fixtureProfiles } from '@/test/fixtures';
import type { Profile } from '@/ipc/bindings';

// The Export/Import actions drive the OS file dialogs; the plugin is stubbed so
// the tests stay hermetic (no real picker, no real filesystem).
const openMock = vi.fn();
const saveDialogMock = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: (...a: unknown[]) => openMock(...a),
  save: (...a: unknown[]) => saveDialogMock(...a),
}));

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

const CUSTOM_ID = '01890a5e-7b2c-7c3d-8e4f-1a2b3c4d5e6f';

/** Stateful profiles backend (upsert/delete/reflag) so refetch sees mutations. */
function mockProfiles(
  opts: {
    fail?: string;
    list?: Profile[];
    printers?: unknown[];
    disabledPrinters?: string[];
    /**
     * What `get_profile` reads "from disk" for an id, when it differs from the
     * cached list (another window edited it). `null` = deleted on disk.
     */
    onDisk?: Record<string, Profile | null>;
  } = {},
): Call[] {
  const calls: Call[] = [];
  let list: Profile[] = opts.list ?? fixtureProfiles;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    switch (cmd) {
      case 'list_profiles':
        return list;
      case 'list_printers':
        return opts.printers ?? [];
      case 'get_printer_prefs':
        return { disabled: opts.disabledPrinters ?? [] };
      case 'get_profile': {
        const { id } = args as { id: string };
        if (opts.onDisk && id in opts.onDisk) {
          const disk = opts.onDisk[id];
          if (disk === null) {
            // Gone on disk: the list follows, as the real backend's would.
            list = list.filter((q) => q.id !== id);
            throw { code: 'ProfileNotFound', message: id };
          }
          if (disk) return disk;
        }
        const found = list.find((q) => q.id === id);
        if (!found) throw new Error(`Profile not found: ${id}`);
        return found;
      }
      case 'save_profile': {
        const p = (args as { profile: Profile }).profile;
        const id = p.id || 'srv-generated-id';
        const saved: Profile = { ...p, id };
        const idx = list.findIndex((q) => q.id === id);
        list =
          idx >= 0
            ? list.map((q, i) => (i === idx ? saved : q))
            : [...list, saved];
        return saved;
      }
      case 'delete_profile': {
        const { id } = args as { id: string };
        // Mirror real semantics: the backend refuses to delete the current
        // default profile (the UI must disable Delete for it).
        if (list.find((q) => q.id === id)?.default) {
          throw new Error('cannot delete the default profile');
        }
        list = list.filter((q) => q.id !== id);
        return null;
      }
      case 'set_default_profile': {
        const { id } = args as { id: string };
        list = list.map((q) => ({ ...q, default: q.id === id }));
        return null;
      }
      case 'export_profile':
        return null;
      case 'import_profiles': {
        // Mirror the backend: fresh ids, de-duplicated names, list grows.
        const imported: Profile[] = [
          {
            ...fixtureProfiles[3]!,
            id: 'imported-1',
            name: 'Contracts duplex 2',
          },
        ];
        list = [...list, ...imported];
        return imported;
      }
      default:
        return null;
    }
  });
  return calls;
}

/** Render the screen with App-owned selection state + a mounted Toaster. */
function Harness({ initialId = 'builtin-quick' }: { initialId?: string }) {
  const [selectedId, setSelectedId] = useState(initialId);
  return (
    <>
      <ProfilesScreen selectedId={selectedId} onSelect={setSelectedId} />
      <Toaster theme="light" />
    </>
  );
}

function resetStore() {
  useProfilesStore.setState({
    status: 'idle',
    profiles: [],
    error: undefined,
    draft: null,
    working: null,
  });
}

describe('ProfilesScreen', () => {
  beforeEach(() => {
    resetStore();
    openMock.mockReset();
    saveDialogMock.mockReset();
  });
  afterEach(() => clearMocks());

  it('renders the profile list and switches the detail on select', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    // All four profiles are in the master list.
    expect(screen.getByText('Duplex BW')).toBeInTheDocument();
    expect(screen.getByText('Contracts duplex')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Duplex BW'));
    // Now selected: appears in both the list row and the detail header.
    expect(screen.getAllByText('Duplex BW').length).toBeGreaterThanOrEqual(2);
    // The read-only grid reflects the selected built-in's values.
    expect(screen.getAllByText('Monochrome').length).toBeGreaterThan(0);
  });

  it('NFR-045: exposes the selected profile and shows sentinels in words', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    const current = screen
      .getAllByRole('button')
      .filter((b) => b.getAttribute('aria-current') === 'true');
    expect(current).toHaveLength(1);
    expect(current[0]).toHaveTextContent('Quick');

    // The built-in's printer and tray are the wire sentinels, never shown raw.
    expect(screen.queryByText('<default>')).not.toBeInTheDocument();
    expect(screen.getByText('System default')).toBeInTheDocument();
    expect(screen.getByText('Automatic')).toBeInTheDocument();
  });

  it('NFR-043: every editor field is named by its caption and described by its error', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    const copies = screen.getByRole('spinbutton', { name: 'Copies' });
    expect(copies).toHaveAttribute('min', '1');
    expect(copies).toHaveAttribute('max', '999');
    fireEvent.change(copies, { target: { value: '0' } });
    expect(copies).toHaveAttribute('aria-invalid', 'true');
    expect(copies).toHaveAccessibleDescription(/whole number from 1 to 999/i);
    for (const name of ['Name', 'Tray', 'Page range']) {
      expect(screen.getByRole('textbox', { name })).toBeInTheDocument();
    }
    for (const name of ['Printer', 'Color', 'Duplex', 'Paper']) {
      expect(screen.getByRole('combobox', { name })).toBeInTheDocument();
    }
  });

  it('marks the default profile with a badge', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');
    // Quick is the default (list row + selected header both badge it).
    expect(screen.getAllByText('default').length).toBeGreaterThan(0);
  });

  it('FR-021: built-in profiles are read-only (no editor, no delete, clone offered)', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    // Quick is selected by default — a built-in.
    expect(screen.getByRole('button', { name: 'Clone' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull();
    expect(
      screen.queryByRole('button', { name: /create profile|save changes/i }),
    ).toBeNull();
    // No editable name field for a built-in.
    expect(screen.queryByDisplayValue('Quick')).toBeNull();
  });

  it('clones a built-in into a prefilled "Copy of …" draft', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'Clone' }));
    // The editor opens on an unsaved draft named "Copy of Quick".
    expect(screen.getByDisplayValue('Copy of Quick')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Create profile' }),
    ).toBeInTheDocument();
    expect(screen.getByText('unsaved')).toBeInTheDocument();
  });

  it('opens a new draft with a unique auto-name', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    expect(screen.getByDisplayValue('New profile')).toBeInTheDocument();
  });

  it('saves a new profile via save_profile with an empty id (create)', async () => {
    const calls = mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    fireEvent.click(screen.getByRole('button', { name: 'Create profile' }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'save_profile')).toBe(true),
    );
    const save = calls.find((c) => c.cmd === 'save_profile')!;
    expect((save.args.profile as Profile).id).toBe('');
    expect((save.args.profile as Profile).name).toBe('New profile');
  });

  it('FR-044: the printer dropdown stores the queue id, not the display name', async () => {
    const calls = mockProfiles({ printers: fixturePrinters });
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    const option = await screen.findByRole('option', {
      name: 'EPSON L15160 Series',
    });
    expect(option).toHaveValue('EPSON_L15160');
    const select = option.closest('select')!;
    fireEvent.change(select, { target: { value: 'EPSON_L15160' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create profile' }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'save_profile')).toBe(true),
    );
    const save = calls.find((c) => c.cmd === 'save_profile')!;
    expect((save.args.profile as Profile).printer).toBe('EPSON_L15160');
  });

  it('shows inline errors and disables Save for invalid input', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    const createBtn = () =>
      screen.getByRole('button', { name: 'Create profile' });
    expect(createBtn()).not.toBeDisabled();

    // Out-of-range copies.
    const copies = screen.getByDisplayValue('1');
    fireEvent.change(copies, { target: { value: '0' } });
    expect(screen.getByText(/whole number from 1 to 999/i)).toBeInTheDocument();
    expect(createBtn()).toBeDisabled();
    fireEvent.change(copies, { target: { value: '2' } });
    expect(createBtn()).not.toBeDisabled();

    // Inverted page range.
    const range = screen.getByPlaceholderText('e.g. 1-5,10');
    fireEvent.change(range, { target: { value: '5-1' } });
    expect(screen.getByText(/ranges must ascend/i)).toBeInTheDocument();
    expect(createBtn()).toBeDisabled();
    fireEvent.change(range, { target: { value: '' } });
    expect(createBtn()).not.toBeDisabled();

    // Duplicate name (case-insensitive) against an existing profile.
    const name = screen.getByDisplayValue('New profile');
    fireEvent.change(name, { target: { value: 'Duplex BW' } });
    expect(screen.getByText(/already exists/i)).toBeInTheDocument();
    expect(createBtn()).toBeDisabled();

    // An empty name and an over-long one each get their own message.
    fireEvent.change(name, { target: { value: '   ' } });
    expect(screen.getByText('Enter a profile name.')).toBeInTheDocument();
    fireEvent.change(name, { target: { value: 'x'.repeat(101) } });
    expect(
      screen.getByText('Name must be 100 characters or fewer.'),
    ).toBeInTheDocument();
  });

  it('discards a draft on Cancel', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    expect(screen.getByDisplayValue('New profile')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByDisplayValue('New profile')).toBeNull();
    // Back to the read-only built-in detail.
    expect(screen.getByRole('button', { name: 'Clone' })).toBeInTheDocument();
  });

  it('sets a non-default profile as default via set_default_profile', async () => {
    const calls = mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByText('Photo Color'));
    fireEvent.click(screen.getByRole('button', { name: 'Make default' }));

    await waitFor(() =>
      expect(
        calls.some(
          (c) =>
            c.cmd === 'set_default_profile' &&
            c.args.id === 'builtin-photo-color',
        ),
      ).toBe(true),
    );
  });

  it('NFR-032: delete asks for confirmation, and Undo re-saves the same id', async () => {
    const calls = mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    // Select the custom profile, then delete it.
    fireEvent.click(screen.getByText('Contracts duplex'));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));

    // An in-app confirmation appears; confirm it.
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }));

    await waitFor(() =>
      expect(
        calls.some(
          (c) => c.cmd === 'delete_profile' && c.args.id === CUSTOM_ID,
        ),
      ).toBe(true),
    );

    // The undo toast re-saves the removed profile, preserving its id.
    fireEvent.click(await screen.findByRole('button', { name: 'Undo' }));
    await waitFor(() => {
      const save = calls.find((c) => c.cmd === 'save_profile');
      expect(save).toBeTruthy();
      expect((save!.args.profile as Profile).id).toBe(CUSTOM_ID);
    });
  });

  it('shows the loading state while the list is in flight', () => {
    useProfilesStore.setState({ status: 'loading', profiles: [] });
    render(<Harness />);
    expect(screen.getByText(/Loading profiles…/i)).toBeInTheDocument();
  });

  it('renders read-only built-ins with a desktop-app notice outside Tauri', async () => {
    const w = window as unknown as {
      __TAURI_INTERNALS__?: unknown;
      __TAURI__?: unknown;
    };
    delete w.__TAURI_INTERNALS__;
    delete w.__TAURI__;

    render(<Harness />);
    expect(
      await screen.findByText(/Editing profiles requires the desktop app/i),
    ).toBeInTheDocument();
    // The built-in list is still shown (static fallback).
    expect(screen.getAllByText('Quick').length).toBeGreaterThan(0);
    expect(screen.getByText('Duplex BW')).toBeInTheDocument();
  });

  // ---- Dirty-draft discard guard ----------------------------------------

  it('D: warns before switching away from a dirty draft, and Cancel keeps it', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    // A new draft is dirty by definition.
    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    expect(screen.getByDisplayValue('New profile')).toBeInTheDocument();

    // Switching to another profile prompts to discard.
    fireEvent.click(screen.getByText('Duplex BW'));
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(/discard unsaved changes/i),
    ).toBeInTheDocument();

    // Cancel keeps the editor and its content intact.
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.getByDisplayValue('New profile')).toBeInTheDocument();
  });

  it('D: confirming Discard switches away and drops the draft', async () => {
    mockProfiles();
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    expect(screen.getByDisplayValue('New profile')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Duplex BW'));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Discard' }));

    // The draft is gone; the selected built-in's read-only detail is shown.
    await waitFor(() =>
      expect(screen.queryByDisplayValue('New profile')).toBeNull(),
    );
    expect(screen.getByRole('button', { name: 'Clone' })).toBeInTheDocument();
  });

  it('D: switching away from a clean editor does not prompt', async () => {
    mockProfiles();
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByText('Photo Color');

    // The custom profile opens the editor (clean — no edits yet).
    expect(screen.getByDisplayValue('Contracts duplex')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Duplex BW'));
    // No discard dialog: a clean editor switches immediately.
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  // ---- Unsaved work survives leaving the screen ---------------------------

  it('regression(profiles-dirty-editor-lost-on-navigation): a new draft survives unmounting the screen', async () => {
    mockProfiles();
    const view = render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    const name = screen.getByDisplayValue('New profile');
    fireEvent.change(name, { target: { value: 'Half typed' } });

    // The sidebar simply unmounts the screen — there is no navigation hook.
    view.unmount();
    render(<Harness />);
    await screen.findByText('Photo Color');

    expect(await screen.findByDisplayValue('Half typed')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Create profile' }),
    ).toBeInTheDocument();
    expect(screen.getByText('unsaved')).toBeInTheDocument();
  });

  it('regression(profiles-dirty-editor-lost-on-navigation): in-place edits survive unmounting, and say so', async () => {
    mockProfiles();
    const view = render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    fireEvent.change(screen.getByDisplayValue('Contracts duplex'), {
      target: { value: 'Contracts duplex v2' },
    });

    view.unmount();
    render(<Harness initialId={CUSTOM_ID} />);

    expect(
      await screen.findByDisplayValue('Contracts duplex v2'),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Unsaved changes from your last visit were restored/i),
    ).toBeInTheDocument();
  });

  it('a saved edit does not linger as a restored draft', async () => {
    mockProfiles();
    const view = render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    fireEvent.change(screen.getByDisplayValue('Contracts duplex'), {
      target: { value: 'Contracts duplex v2' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await screen.findByText('Saved "Contracts duplex v2"');

    view.unmount();
    render(<Harness initialId={CUSTOM_ID} />);
    expect(
      screen.queryByText(/Unsaved changes from your last visit/i),
    ).toBeNull();
  });

  // ---- get_profile is a live consumer ------------------------------------

  it('regression(get-profile-command-never-called): re-reads the selected custom through get_profile', async () => {
    const calls = mockProfiles();
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    await waitFor(() =>
      expect(
        calls.some((c) => c.cmd === 'get_profile' && c.args.id === CUSTOM_ID),
      ).toBe(true),
    );
    // A built-in is served from the list, so it costs no round-trip.
    const before = calls.filter((c) => c.cmd === 'get_profile').length;
    fireEvent.click(screen.getByText('Duplex BW'));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Clone' })).toBeInTheDocument(),
    );
    expect(calls.filter((c) => c.cmd === 'get_profile')).toHaveLength(before);
  });

  it('FR-020: a profile changed on disk re-seeds a clean editor, so Save cannot write stale values back', async () => {
    const fresh: Profile = { ...fixtureProfiles[3]!, copies: 7 };
    mockProfiles({ onDisk: { [CUSTOM_ID]: fresh } });
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    // The editor now shows the on-disk copies and is not dirty.
    expect(await screen.findByDisplayValue('7')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();
  });

  it('FR-020: a profile deleted on disk moves the selection to the default', async () => {
    mockProfiles({ onDisk: { [CUSTOM_ID]: null } });
    render(<Harness initialId={CUSTOM_ID} />);

    expect(
      await screen.findByText('This profile was deleted outside the app.'),
    ).toBeInTheDocument();
    // The default (Quick) is a built-in: read-only detail, Clone offered.
    expect(
      await screen.findByRole('button', { name: 'Clone' }),
    ).toBeInTheDocument();
    expect(screen.queryByText('Contracts duplex')).toBeNull();
  });

  it('FR-024: Export is disabled while the editor holds unsaved edits', async () => {
    mockProfiles();
    render(<Harness initialId={CUSTOM_ID} />);
    const name = await screen.findByDisplayValue('Contracts duplex');
    expect(screen.getByRole('button', { name: 'Export' })).toBeEnabled();

    fireEvent.change(name, { target: { value: 'Contracts duplex v2' } });
    expect(screen.getByRole('button', { name: 'Export' })).toBeDisabled();
  });

  it('FR-024: a file dialog that fails to open says so', async () => {
    mockProfiles();
    saveDialogMock.mockRejectedValue(new Error('dialog denied'));
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    fireEvent.click(screen.getByRole('button', { name: 'Export' }));
    expect(
      await screen.findByText("Couldn't open the file dialog."),
    ).toBeInTheDocument();
    err.mockRestore();
  });

  it('offers Retry when the list fails to load, and a retry recovers', async () => {
    let fail = true;
    mockIPC((cmd) => {
      if (cmd === 'list_profiles') {
        if (fail) throw new Error('boom');
        return fixtureProfiles;
      }
      if (cmd === 'list_printers') return [];
      if (cmd === 'get_printer_prefs') return { disabled: [] };
      return null;
    });
    render(<Harness />);
    const retry = await screen.findByRole('button', { name: 'Retry' });
    fail = false;
    fireEvent.click(retry);
    expect(await screen.findByText('Photo Color')).toBeInTheDocument();
  });

  // ---- FR-023: the stored printer is the queue id ------------------------

  /** The editor's printer <select> (the one offering "System default"). */
  function printerSelect(): HTMLSelectElement {
    const sel = screen.getAllByRole('combobox').find((s) =>
      within(s)
        .queryAllByRole('option')
        .some((o) => o.textContent === 'System default'),
    );
    if (!sel) throw new Error('no printer select');
    return sel as HTMLSelectElement;
  }

  it('FR-023: the editor stores the printer id (queue name), not its display name', async () => {
    const calls = mockProfiles({
      printers: fixturePrinters,
      disabledPrinters: ['HL_L2350'],
    });
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    await waitFor(() =>
      expect(
        within(printerSelect()).getByRole('option', {
          name: 'EPSON L15160 Series',
        }),
      ).toBeInTheDocument(),
    );
    // A printer disabled in MaeclyPrint is not offered, as on Print Job.
    expect(
      within(printerSelect()).queryByRole('option', {
        name: 'Brother HL-L2350DW',
      }),
    ).toBeNull();

    fireEvent.change(printerSelect(), { target: { value: 'EPSON_L15160' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create profile' }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'save_profile')).toBe(true),
    );
    const save = calls.find((c) => c.cmd === 'save_profile')!;
    expect((save.args.profile as Profile).printer).toBe('EPSON_L15160');
  });

  it('FR-023: a legacy display-name printer is shown as that printer and saved as its id', async () => {
    const legacy: Profile = {
      ...fixtureProfiles[3]!,
      printer: 'EPSON L15160 Series',
    };
    const calls = mockProfiles({
      list: [...fixtureProfiles.slice(0, 3), legacy],
      printers: fixturePrinters,
    });
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');
    await waitFor(() => expect(printerSelect().value).toBe('EPSON_L15160'));
    // Mapping the label to its id is not an edit the user made.
    expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();

    fireEvent.change(screen.getByDisplayValue('Contracts duplex'), {
      target: { value: 'Contracts duplex v2' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'save_profile')).toBe(true),
    );
    const save = calls.find((c) => c.cmd === 'save_profile')!;
    expect((save.args.profile as Profile).printer).toBe('EPSON_L15160');
  });

  // ---- Quality / tray honesty --------------------------------------------

  it('FR-023: the editor says tray and quality are sent with every job', async () => {
    mockProfiles();
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    expect(
      screen.getByText(/Tray and quality are sent with every job/i),
    ).toBeInTheDocument();
    // The old copy claimed the engine ignored them, which was false.
    expect(screen.queryByText(/does not send them yet/i)).toBeNull();
  });

  // ---- FR-024: import / export -------------------------------------------

  it('FR-024: exports the selected profile through the save dialog', async () => {
    const calls = mockProfiles();
    saveDialogMock.mockResolvedValue(
      '/tmp/Contracts-duplex.maecly-profile.toml',
    );
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    fireEvent.click(screen.getByRole('button', { name: 'Export' }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'export_profile')).toBe(true),
    );
    const call = calls.find((c) => c.cmd === 'export_profile')!;
    expect(call.args.id).toBe(CUSTOM_ID);
    expect(call.args.path).toBe('/tmp/Contracts-duplex.maecly-profile.toml');
    // The dialog is seeded with the `<name>.maecly-profile.toml` convention.
    expect(saveDialogMock.mock.calls[0]?.[0]).toMatchObject({
      defaultPath: 'Contracts duplex.maecly-profile.toml',
    });
    expect(
      await screen.findByText('Exported "Contracts duplex"'),
    ).toBeInTheDocument();
  });

  it('FR-024: cancelling the export dialog calls nothing', async () => {
    const calls = mockProfiles();
    saveDialogMock.mockResolvedValue(null);
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    fireEvent.click(screen.getByRole('button', { name: 'Export' }));
    await waitFor(() => expect(saveDialogMock).toHaveBeenCalled());
    expect(calls.some((c) => c.cmd === 'export_profile')).toBe(false);
  });

  it('FR-024: imports profiles, reloads the list and selects the first one', async () => {
    const calls = mockProfiles();
    openMock.mockResolvedValue('/tmp/shared.maecly-profile.toml');
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'Import' }));

    await waitFor(() =>
      expect(calls.some((c) => c.cmd === 'import_profiles')).toBe(true),
    );
    expect(calls.find((c) => c.cmd === 'import_profiles')!.args.path).toBe(
      '/tmp/shared.maecly-profile.toml',
    );
    // The renamed copy is in the list and selected, and the toast counts it.
    expect(await screen.findAllByText('Contracts duplex 2')).not.toHaveLength(
      0,
    );
    expect(await screen.findByText('Imported 1 profile')).toBeInTheDocument();
  });

  it('FR-024: a rejected import surfaces the backend reason and changes nothing', async () => {
    const calls = mockProfiles({ fail: 'import_profiles' });
    openMock.mockResolvedValue('/tmp/corrupt.toml');
    render(<Harness />);
    await screen.findByText('Photo Color');
    const before = useProfilesStore.getState().profiles;

    fireEvent.click(screen.getByRole('button', { name: 'Import' }));

    expect(await screen.findByText('boom')).toBeInTheDocument();
    expect(useProfilesStore.getState().profiles).toBe(before);
    expect(calls.filter((c) => c.cmd === 'list_profiles')).toHaveLength(1);
  });

  it('FR-024: cancelling the import dialog calls nothing', async () => {
    const calls = mockProfiles();
    openMock.mockResolvedValue(null);
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'Import' }));
    await waitFor(() => expect(openMock).toHaveBeenCalled());
    expect(calls.some((c) => c.cmd === 'import_profiles')).toBe(false);
  });

  it('FR-024: Import is guarded by the same discard prompt as the rest', async () => {
    mockProfiles();
    openMock.mockResolvedValue('/tmp/shared.toml');
    render(<Harness />);
    await screen.findByText('Photo Color');

    fireEvent.click(screen.getByRole('button', { name: 'New profile' }));
    fireEvent.click(screen.getByRole('button', { name: 'Import' }));

    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(/discard unsaved changes/i),
    ).toBeInTheDocument();
    expect(openMock).not.toHaveBeenCalled();
  });

  // ---- Delete disabled on the current default custom profile -------------

  it('F: disables Delete on the current default custom profile', async () => {
    // Make the custom profile the default so Delete must be blocked. initialId
    // selects it, so the editor opens on it (its name input is unique).
    mockProfiles({
      list: fixtureProfiles.map((p) => ({ ...p, default: p.id === CUSTOM_ID })),
    });
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    const del = screen.getByRole('button', { name: 'Delete' });
    expect(del).toBeDisabled();
    // The reason is surfaced as a tooltip on the wrapping span.
    expect(del.parentElement).toHaveAttribute(
      'title',
      'Set another profile as default first',
    );
  });

  it('F: allows deleting a non-default custom profile', async () => {
    mockProfiles();
    render(<Harness initialId={CUSTOM_ID} />);
    await screen.findByDisplayValue('Contracts duplex');

    expect(screen.getByRole('button', { name: 'Delete' })).not.toBeDisabled();
  });
});

describe('exportFileName', () => {
  it('FR-024: follows the <name>.maecly-profile.toml convention', () => {
    expect(exportFileName('Press A')).toBe('Press A.maecly-profile.toml');
  });

  it('FR-024: strips path separators and reserved characters', () => {
    expect(exportFileName('a/b\\c:d*e?f"g<h>i|j')).toBe(
      'a-b-c-d-e-f-g-h-i-j.maecly-profile.toml',
    );
    expect(exportFileName('  ...leading')).toBe('leading.maecly-profile.toml');
    expect(exportFileName('   ')).toBe('profile.maecly-profile.toml');
  });

  it('FR-024: caps the length so no OS rejects the path', () => {
    const name = exportFileName('x'.repeat(200));
    expect(name.length).toBeLessThanOrEqual(60 + '.maecly-profile.toml'.length);
  });
});
