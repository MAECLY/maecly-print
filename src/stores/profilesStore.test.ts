import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { toast } from 'sonner';

import type { Profile } from '@/ipc/bindings';
import type { UiProfile } from '@/types/domain';
import {
  fixtureAppSettings,
  fixtureProfiles,
  mkProfile,
} from '@/test/fixtures';
import { BUILT_IN_PROFILES } from '@/lib/profiles';
import { useProfilesStore } from '@/stores/profilesStore';
import { useSettingsStore } from '@/stores/settingsStore';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

const CUSTOM_ID = '01890a5e-7b2c-7c3d-8e4f-1a2b3c4d5e6f';

/**
 * A stateful profiles backend: `list_profiles` returns a mutable list that the
 * write commands upsert/delete/reflag, so the store's reload-after-write sees a
 * realistic post-mutation snapshot. `fail` makes the named command throw.
 */
function mockProfiles(opts: { list?: Profile[]; fail?: string } = {}): Call[] {
  const calls: Call[] = [];
  let list: Profile[] = opts.list ?? fixtureProfiles;
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    switch (cmd) {
      case 'list_profiles':
        return list;
      case 'get_profile': {
        const { id } = args as { id: string };
        const found = list.find((q) => q.id === id);
        if (!found) throw new Error(`Profile not found: ${id}`);
        return found;
      }
      case 'export_profile':
        return null;
      case 'import_profiles': {
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
        // default profile.
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

/** A custom UiProfile ready to save (empty id → create). */
function draft(over: Partial<UiProfile> = {}): UiProfile {
  return {
    id: '',
    name: 'Draft profile',
    builtin: false,
    printer: '<default>',
    color: 'color',
    duplex: 'off',
    copies: 1,
    paper: 'A4',
    orientation: 'auto',
    scaling: 'fit',
    quality: 'normal',
    tray: 'auto',
    ...over,
  };
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

describe('profilesStore', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('adapts the wire list into UiProfiles on init', async () => {
    mockProfiles({
      list: [
        ...fixtureProfiles,
        mkProfile({
          id: 'c2',
          name: 'Ranged',
          page_range: '2-3',
          paper_size: 'legal',
        }),
      ],
    });
    await useProfilesStore.getState().init();

    const s = useProfilesStore.getState();
    expect(s.status).toBe('ready');
    expect(s.profiles).toHaveLength(5);
    // Paper is re-cased and page_range is renamed to pageRange.
    const quick = s.profiles.find((p) => p.id === 'builtin-quick')!;
    expect(quick.paper).toBe('A4');
    const ranged = s.profiles.find((p) => p.id === 'c2')!;
    expect(ranged.paper).toBe('Legal');
    expect(ranged.pageRange).toBe('2-3');
  });

  it('is idempotent — a second init does not re-fetch', async () => {
    const calls = mockProfiles();
    await useProfilesStore.getState().init();
    await useProfilesStore.getState().init();
    expect(calls.filter((c) => c.cmd === 'list_profiles')).toHaveLength(1);
  });

  it('reports unavailable and falls back to the built-ins outside Tauri', async () => {
    const w = window as unknown as {
      __TAURI_INTERNALS__?: unknown;
      __TAURI__?: unknown;
    };
    delete w.__TAURI_INTERNALS__;
    delete w.__TAURI__;

    await useProfilesStore.getState().init();
    const s = useProfilesStore.getState();
    expect(s.status).toBe('unavailable');
    expect(s.profiles).toEqual(BUILT_IN_PROFILES);
  });

  it('marks status error on failure and is retryable', async () => {
    mockProfiles({ fail: 'list_profiles' });
    await useProfilesStore.getState().init();
    expect(useProfilesStore.getState().status).toBe('error');
    expect(useProfilesStore.getState().error).toBe('boom');

    // A fresh mock + re-init recovers (init re-runs after an error).
    mockProfiles();
    await useProfilesStore.getState().init();
    expect(useProfilesStore.getState().status).toBe('ready');
    expect(useProfilesStore.getState().profiles).toHaveLength(4);
  });

  it('saves via save_profile with an empty id for create, then refetches', async () => {
    const calls = mockProfiles();
    await useProfilesStore.getState().init();

    const saved = await useProfilesStore
      .getState()
      .save(draft({ name: 'Marketing' }));

    const save = calls.find((c) => c.cmd === 'save_profile');
    expect((save!.args.profile as Profile).id).toBe('');
    expect((save!.args.profile as Profile).name).toBe('Marketing');
    // Reload-after-write: list_profiles ran again and the store grew.
    expect(calls.filter((c) => c.cmd === 'list_profiles')).toHaveLength(2);
    expect(saved?.id).toBe('srv-generated-id');
    expect(useProfilesStore.getState().profiles).toHaveLength(5);
  });

  it('re-saves an unknown non-empty id preserving it (undo path)', async () => {
    mockProfiles();
    await useProfilesStore.getState().init();

    const removed = await useProfilesStore.getState().remove(CUSTOM_ID);
    expect(
      useProfilesStore.getState().profiles.some((p) => p.id === CUSTOM_ID),
    ).toBe(false);

    const restored = await useProfilesStore.getState().save(removed!);
    expect(restored?.id).toBe(CUSTOM_ID);
    expect(
      useProfilesStore.getState().profiles.some((p) => p.id === CUSTOM_ID),
    ).toBe(true);
  });

  it('removes via delete_profile and resolves the removed snapshot', async () => {
    const calls = mockProfiles();
    await useProfilesStore.getState().init();

    const removed = await useProfilesStore.getState().remove(CUSTOM_ID);
    expect(
      calls.some((c) => c.cmd === 'delete_profile' && c.args.id === CUSTOM_ID),
    ).toBe(true);
    expect(removed?.id).toBe(CUSTOM_ID);
    expect(removed?.name).toBe('Contracts duplex');
    expect(
      useProfilesStore.getState().profiles.some((p) => p.id === CUSTOM_ID),
    ).toBe(false);
  });

  it('FR-022: the default flag moves after set_default_profile', async () => {
    const calls = mockProfiles();
    await useProfilesStore.getState().init();
    expect(useProfilesStore.getState().defaultProfile().id).toBe(
      'builtin-quick',
    );

    const ok = await useProfilesStore
      .getState()
      .setDefault('builtin-duplex-bw');
    expect(ok).toBe(true);
    expect(
      calls.some(
        (c) =>
          c.cmd === 'set_default_profile' && c.args.id === 'builtin-duplex-bw',
      ),
    ).toBe(true);

    const s = useProfilesStore.getState();
    expect(s.profiles.find((p) => p.id === 'builtin-duplex-bw')!.default).toBe(
      true,
    );
    expect(s.profiles.find((p) => p.id === 'builtin-quick')!.default).toBe(
      false,
    );
    expect(s.defaultProfile().id).toBe('builtin-duplex-bw');
  });

  it('regression: setDefault syncs the settings store cache so a later engine edit cannot revert the default', async () => {
    mockProfiles();
    await useProfilesStore.getState().init();
    // The settings store holds a loaded config whose engine section still
    // carries the OLD default; without the sync, the next whole-section
    // engine update would write it back.
    useSettingsStore.setState({
      status: 'ready',
      settings: {
        ...fixtureAppSettings,
        engine: { ...fixtureAppSettings.engine, default_profile: null },
      },
    });

    await useProfilesStore.getState().setDefault('builtin-duplex-bw');

    expect(useSettingsStore.getState().settings?.engine.default_profile).toBe(
      'builtin-duplex-bw',
    );
  });

  it('regression: setDefault syncs the settings cache even if the post-set refetch fails', async () => {
    // list_profiles succeeds for init, then throws on the post-set refetch. The
    // cache sync must therefore happen BEFORE the refetch to survive it.
    let listCalls = 0;
    mockIPC((cmd) => {
      if (cmd === 'list_profiles') {
        listCalls += 1;
        if (listCalls >= 2) throw new Error('boom');
        return fixtureProfiles;
      }
      if (cmd === 'set_default_profile') return null;
      return null;
    });
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useProfilesStore.getState().init();
    useSettingsStore.setState({
      status: 'ready',
      settings: {
        ...fixtureAppSettings,
        engine: { ...fixtureAppSettings.engine, default_profile: null },
      },
    });

    const ok = await useProfilesStore
      .getState()
      .setDefault('builtin-duplex-bw');
    // The refetch failed, so the call reports failure and toasts…
    expect(ok).toBe(false);
    expect(spy).toHaveBeenCalledWith('boom');
    // …but the settings cache was already synced before the refetch threw, so
    // a later whole-section engine patch cannot revert the on-disk default.
    expect(useSettingsStore.getState().settings?.engine.default_profile).toBe(
      'builtin-duplex-bw',
    );
    spy.mockRestore();
  });

  it('surfaces a server rejection as a toast and leaves state unchanged', async () => {
    mockProfiles({ fail: 'save_profile' });
    await useProfilesStore.getState().init();
    const before = useProfilesStore.getState().profiles;

    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    const res = await useProfilesStore.getState().save(draft());
    expect(res).toBeNull();
    expect(spy).toHaveBeenCalledWith('boom');
    // No refetch on failure: the same array reference is retained.
    expect(useProfilesStore.getState().profiles).toBe(before);
    spy.mockRestore();
  });

  it('returns false and toasts when set_default_profile is rejected', async () => {
    mockProfiles({ fail: 'set_default_profile' });
    await useProfilesStore.getState().init();

    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    const ok = await useProfilesStore
      .getState()
      .setDefault('builtin-photo-color');
    expect(ok).toBe(false);
    expect(spy).toHaveBeenCalledWith('boom');
    spy.mockRestore();
  });

  it('returns null and toasts when delete_profile is rejected', async () => {
    mockProfiles({ fail: 'delete_profile' });
    await useProfilesStore.getState().init();

    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    const removed = await useProfilesStore.getState().remove(CUSTOM_ID);
    expect(removed).toBeNull();
    expect(spy).toHaveBeenCalledWith('boom');
    // The profile is still present (delete never took effect).
    expect(
      useProfilesStore.getState().profiles.some((p) => p.id === CUSTOM_ID),
    ).toBe(true);
    spy.mockRestore();
  });
  // ---- get_profile: the single-profile refresh ---------------------------

  it('refreshProfile patches a stale row from get_profile', async () => {
    const calls = mockProfiles();
    await useProfilesStore.getState().init();
    // Simulate the list going stale (another window renamed the profile).
    useProfilesStore.setState({
      profiles: useProfilesStore
        .getState()
        .profiles.map((p) =>
          p.id === CUSTOM_ID ? { ...p, name: 'Stale name', copies: 99 } : p,
        ),
    });

    await useProfilesStore.getState().refreshProfile(CUSTOM_ID);

    expect(
      calls.some((c) => c.cmd === 'get_profile' && c.args.id === CUSTOM_ID),
    ).toBe(true);
    const fresh = useProfilesStore
      .getState()
      .profiles.find((p) => p.id === CUSTOM_ID)!;
    expect(fresh.name).toBe('Contracts duplex');
    expect(fresh.copies).not.toBe(99);
  });

  it('refreshProfile keeps the array identity when nothing changed', async () => {
    mockProfiles();
    await useProfilesStore.getState().init();
    const before = useProfilesStore.getState().profiles;

    await useProfilesStore.getState().refreshProfile(CUSTOM_ID);
    expect(useProfilesStore.getState().profiles).toBe(before);
  });

  it('refreshProfile stays silent when the backend rejects', async () => {
    mockProfiles({ fail: 'get_profile' });
    await useProfilesStore.getState().init();
    const before = useProfilesStore.getState().profiles;
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');

    await useProfilesStore.getState().refreshProfile(CUSTOM_ID);

    // Best-effort: no toast, no state churn — the cached row still renders.
    expect(spy).not.toHaveBeenCalled();
    expect(useProfilesStore.getState().profiles).toBe(before);
    spy.mockRestore();
  });

  // ---- FR-024: import / export -------------------------------------------

  it('FR-024: exportProfile forwards the id and the destination path', async () => {
    const calls = mockProfiles();
    await useProfilesStore.getState().init();

    const ok = await useProfilesStore
      .getState()
      .exportProfile(CUSTOM_ID, '/tmp/out.maecly-profile.toml');

    expect(ok).toBe(true);
    const call = calls.find((c) => c.cmd === 'export_profile');
    expect(call?.args).toEqual({
      id: CUSTOM_ID,
      path: '/tmp/out.maecly-profile.toml',
    });
  });

  it('FR-024: a rejected export toasts and reports failure', async () => {
    mockProfiles({ fail: 'export_profile' });
    await useProfilesStore.getState().init();
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');

    const ok = await useProfilesStore.getState().exportProfile(CUSTOM_ID, '/x');
    expect(ok).toBe(false);
    expect(spy).toHaveBeenCalledWith('boom');
    spy.mockRestore();
  });

  it('FR-024: importProfiles adapts the result and reloads the list', async () => {
    const calls = mockProfiles();
    await useProfilesStore.getState().init();

    const imported = await useProfilesStore
      .getState()
      .importProfiles('/tmp/shared.toml');

    expect(imported).toHaveLength(1);
    // Adapted like every other profile (wire `paper_size` → display `paper`).
    expect(imported![0]!.paper).toBe('Letter');
    expect(imported![0]!.id).toBe('imported-1');
    // Reload-after-write: the list grew.
    expect(calls.filter((c) => c.cmd === 'list_profiles')).toHaveLength(2);
    expect(useProfilesStore.getState().profiles).toHaveLength(5);
  });

  it('FR-024: a rejected import toasts, returns null and does not refetch', async () => {
    const calls = mockProfiles({ fail: 'import_profiles' });
    await useProfilesStore.getState().init();
    const before = useProfilesStore.getState().profiles;
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');

    const imported = await useProfilesStore
      .getState()
      .importProfiles('/tmp/corrupt.toml');

    expect(imported).toBeNull();
    expect(spy).toHaveBeenCalledWith('boom');
    expect(useProfilesStore.getState().profiles).toBe(before);
    expect(calls.filter((c) => c.cmd === 'list_profiles')).toHaveLength(1);
    spy.mockRestore();
  });

  it('FR-024: an IpcError object from an unbound command becomes a readable message', async () => {
    mockIPC((cmd) => {
      if (cmd === 'list_profiles') return fixtureProfiles;
      // What a command-returned IpcError looks like on the raw invoke path.
      throw { code: 'InvalidRequest', message: 'this is not a profile file' };
    });
    await useProfilesStore.getState().init();
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');

    expect(await useProfilesStore.getState().importProfiles('/x')).toBeNull();
    expect(spy).toHaveBeenCalledWith('this is not a profile file');
    spy.mockRestore();
  });

  // ---- Editor state kept across screen unmounts ---------------------------

  it('regression(profiles-dirty-editor-lost-on-navigation): the draft and working copy outlive the screen', () => {
    const p = draft({ name: 'Half typed' });
    useProfilesStore.getState().setDraft(p);
    useProfilesStore
      .getState()
      .setWorking({ key: 'draft', profile: p, copiesText: '3' });

    // Unmounting the screen touches nothing here — that is the whole point.
    expect(useProfilesStore.getState().draft).toEqual(p);
    expect(useProfilesStore.getState().working?.copiesText).toBe('3');

    useProfilesStore.getState().clearEditor();
    expect(useProfilesStore.getState().draft).toBeNull();
    expect(useProfilesStore.getState().working).toBeNull();
  });
});
