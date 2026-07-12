/**
 * Profiles store — the UI's view of the print-profile list.
 *
 * `init()` fetches the list once (idempotent, re-runnable after an error).
 * Profiles have no live event stream, so mutations follow a reload-after-write
 * model: each action calls its IPC wrapper, then re-fetches the whole list and
 * replaces state wholesale, so the server's computed `builtin`/`default` flags
 * are always authoritative and we never drift from a mis-guessed optimistic
 * update. Outside the Tauri host the store falls back to the static built-ins
 * so the Print Job dropdown still works in the standalone Vite preview.
 */

import { create } from 'zustand';
import { toast } from 'sonner';

import {
  deleteProfile as deleteProfileCmd,
  isTauri,
  listProfiles,
  saveProfile as saveProfileCmd,
  setDefaultProfile as setDefaultProfileCmd,
} from '@/lib/ipc';
import { BUILT_IN_PROFILES, findProfileIn } from '@/lib/profiles';
import { useSettingsStore } from '@/stores/settingsStore';
import type { UiProfile } from '@/types/domain';

/** Lifecycle of the store's connection to the backend profile list. */
export type ProfilesLoadStatus =
  | 'idle'
  | 'loading'
  | 'ready'
  | 'unavailable'
  | 'error';

interface ProfilesState {
  profiles: UiProfile[];
  status: ProfilesLoadStatus;
  /** Explicit `| undefined` so `set({ error: undefined })` can clear it. */
  error?: string | undefined;

  init: () => Promise<void>;
  /** Upsert a profile; resolves the saved profile, or null on rejection. */
  save: (p: UiProfile) => Promise<UiProfile | null>;
  /** Delete a profile; resolves the removed snapshot (for undo), or null. */
  remove: (id: string) => Promise<UiProfile | null>;
  /** Set the default profile; resolves true on success, false on rejection. */
  setDefault: (id: string) => Promise<boolean>;
  /** The current default profile (or the fallback chain), derived on demand. */
  defaultProfile: () => UiProfile;
}

/** Turn an unknown thrown value into a user-facing message. */
function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useProfilesStore = create<ProfilesState>()((set, get) => {
  /** Re-read the whole list and replace state (reload-after-write). */
  async function refetch(): Promise<void> {
    const profiles = await listProfiles();
    set({ profiles, status: 'ready', error: undefined });
  }

  return {
    profiles: [],
    status: 'idle',

    init: async () => {
      // Idempotent while loading/ready; re-runnable after a failure so one
      // transient error doesn't leave the editor dead for the session.
      const status = get().status;
      if (status !== 'idle' && status !== 'error') return;
      if (!isTauri()) {
        set({ status: 'unavailable', profiles: BUILT_IN_PROFILES });
        return;
      }
      set({ status: 'loading', error: undefined });
      try {
        await refetch();
      } catch (e) {
        set({ status: 'error', error: message(e) });
      }
    },

    save: async (p) => {
      try {
        const saved = await saveProfileCmd(p);
        await refetch();
        return saved;
      } catch (e) {
        toast.error(message(e));
        return null;
      }
    },

    remove: async (id) => {
      // Snapshot before the delete so the caller can offer an undo that
      // re-saves the exact same id (the backend upsert preserves it).
      const removed = get().profiles.find((p) => p.id === id) ?? null;
      try {
        await deleteProfileCmd(id);
        await refetch();
        return removed;
      } catch (e) {
        toast.error(message(e));
        return null;
      }
    },

    setDefault: async (id) => {
      try {
        await setDefaultProfileCmd(id);
        // set_default_profile writes config.toml's engine.default_profile
        // behind the settings store's back; sync its cached copy IMMEDIATELY
        // (before the refetch, which may throw) or the next whole-section
        // engine update would silently revert the default from a stale cache.
        const settings = useSettingsStore.getState().settings;
        if (settings && settings.engine.default_profile !== id) {
          useSettingsStore.setState({
            settings: {
              ...settings,
              engine: { ...settings.engine, default_profile: id },
            },
          });
        }
        // Reload the list last: a refetch failure only affects list freshness,
        // not the already-synced default.
        await refetch();
        return true;
      } catch (e) {
        toast.error(message(e));
        return false;
      }
    },

    defaultProfile: () => findProfileIn(get().profiles, null),
  };
});
