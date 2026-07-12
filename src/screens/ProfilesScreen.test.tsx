import { useState } from 'react';
import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';

import { ProfilesScreen } from './ProfilesScreen';
import { Toaster } from '@/components/AppShell/Toaster';
import { useProfilesStore } from '@/stores/profilesStore';
import { fixtureProfiles } from '@/test/fixtures';
import type { Profile } from '@/ipc/bindings';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

const CUSTOM_ID = '01890a5e-7b2c-7c3d-8e4f-1a2b3c4d5e6f';

/** Stateful profiles backend (upsert/delete/reflag) so refetch sees mutations. */
function mockProfiles(opts: { fail?: string; list?: Profile[] } = {}): Call[] {
  const calls: Call[] = [];
  let list: Profile[] = opts.list ?? fixtureProfiles;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    switch (cmd) {
      case 'list_profiles':
        return list;
      case 'list_printers':
        return [];
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
  useProfilesStore.setState({ status: 'idle', profiles: [], error: undefined });
}

describe('ProfilesScreen', () => {
  beforeEach(() => resetStore());
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
    expect(screen.getByText('monochrome')).toBeInTheDocument();
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
