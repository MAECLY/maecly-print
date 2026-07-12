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
  useProfilesStore.setState({ status: 'idle', profiles: [], error: undefined });
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
});
