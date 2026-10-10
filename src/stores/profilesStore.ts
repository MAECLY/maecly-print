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
 *
 * The store also owns the **editor draft** (`draft` / `working`). That state is
 * conceptually screen-local, but the Profiles screen is unmounted the moment
 * the user clicks another sidebar entry, which used to throw the unsaved work
 * away without a word. Keeping it here means navigating away and back restores
 * exactly what was typed, so no confirmation prompt is needed on app-level
 * navigation at all (the in-screen discard guard still covers switching
 * profiles, where the working copy genuinely has to go).
 */

import { create } from 'zustand';
import { toast } from 'sonner';

import { commands } from '@/ipc/bindings';
import {
  deleteProfile as deleteProfileCmd,
  isTauri,
  listProfiles,
  saveProfile as saveProfileCmd,
  setDefaultProfile as setDefaultProfileCmd,
  toUiProfile,
  unwrap,
} from '@/lib/ipc';
import { builtInProfiles, findProfileIn } from '@/lib/profiles';
import { useSettingsStore } from '@/stores/settingsStore';
import type { UiProfile } from '@/types/domain';

/** Lifecycle of the store's connection to the backend profile list. */
export type ProfilesLoadStatus =
  | 'idle'
  | 'loading'
  | 'ready'
  | 'unavailable'
  | 'error';

/**
 * The profile editor's live working copy, kept across screen unmounts.
 *
 * `key` identifies the editor instance it belongs to — `'draft'` for the
 * unsaved new/clone draft, `profile:<id>` for an in-place edit — so a copy is
 * never restored into the wrong form.
 */
export interface EditorWorking {
  key: string;
  profile: UiProfile;
  /** Raw copies text, buffered so an out-of-range keystroke survives too. */
  copiesText: string;
}

/** Outcome of {@link ProfilesState.refreshProfile}. */
export type RefreshResult =
  | { kind: 'unchanged' }
  | { kind: 'changed'; before: UiProfile }
  | { kind: 'missing' }
  | { kind: 'failed' };

interface ProfilesState {
  profiles: UiProfile[];
  status: ProfilesLoadStatus;
  /** Explicit `| undefined` so `set({ error: undefined })` can clear it. */
  error?: string | undefined;

  /** The unsaved new/clone draft (id `''`), or null when none is open. */
  draft: UiProfile | null;
  /** The editor's live working copy, or null when nothing is being edited. */
  working: EditorWorking | null;

  init: () => Promise<void>;
  /**
   * Re-read the whole list from disk, whatever the current status, and
   * resolve the fresh list. `profiles.toml` is written by more than this
   * window — the CLI (`profiles create`), a second window, a hand edit — and
   * the store has no file watcher, so a caller that is about to act on a
   * profile reference it did not pick from this list (a context-menu
   * selection, FR-001) must reload first. Rejects when the backend cannot be
   * reached; the cached list is left untouched in that case.
   */
  reload: () => Promise<UiProfile[]>;
  /** Upsert a profile; resolves the saved profile, or null on rejection. */
  save: (p: UiProfile) => Promise<UiProfile | null>;
  /** Delete a profile; resolves the removed snapshot (for undo), or null. */
  remove: (id: string) => Promise<UiProfile | null>;
  /** Set the default profile; resolves true on success, false on rejection. */
  setDefault: (id: string) => Promise<boolean>;
  /** The current default profile (or the fallback chain), derived on demand. */
  defaultProfile: () => UiProfile;

  /**
   * Re-read one profile by id and refresh its row if the backend disagrees.
   * Says what happened, so the screen can re-seed an editor built from the
   * old row (`changed`, with the row as it was) or move off a profile that no
   * longer exists (`missing`, after the list was reloaded).
   */
  refreshProfile: (id: string) => Promise<RefreshResult>;
  /** Write one profile to `path` as a TOML document (FR-024). */
  exportProfile: (id: string, path: string) => Promise<boolean>;
  /** Import profiles from the TOML document at `path` (FR-024). */
  importProfiles: (path: string) => Promise<UiProfile[] | null>;

  setDraft: (p: UiProfile | null) => void;
  setWorking: (w: EditorWorking | null) => void;
  /** Drop both the draft and the working copy (save / cancel / delete). */
  clearEditor: () => void;
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
    draft: null,
    working: null,

    init: async () => {
      // Idempotent while loading/ready; re-runnable after a failure so one
      // transient error doesn't leave the editor dead for the session.
      const status = get().status;
      if (status !== 'idle' && status !== 'error') return;
      if (!isTauri()) {
        set({ status: 'unavailable', profiles: builtInProfiles() });
        return;
      }
      set({ status: 'loading', error: undefined });
      try {
        await refetch();
      } catch (e) {
        set({ status: 'error', error: message(e) });
      }
    },

    reload: async () => {
      if (!isTauri()) {
        if (get().status === 'idle') await get().init();
        return get().profiles;
      }
      await refetch();
      return get().profiles;
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
        useSettingsStore.getState().syncDefaultProfile(id);
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

    /**
     * Re-read a single profile straight from disk and patch its row when it
     * differs from what the cached list says.
     *
     * The Profiles screen calls this as it opens the editor for a saved custom:
     * `profiles.toml` can change under us (a second window, a hand edit), and
     * an editor seeded from a stale row would silently write the stale values
     * back on the next save. Best-effort by design — a failure just leaves the
     * cached row in place, which is what the screen would have shown anyway.
     */
    refreshProfile: async (id) => {
      if (!isTauri()) return { kind: 'failed' };
      let fresh: UiProfile;
      try {
        const r = await commands.getProfile(id);
        if (r.status === 'error') {
          if (r.error?.code === 'ProfileNotFound') {
            // Deleted behind our back (another window, the CLI). Keeping the
            // row would let a later Save re-create it through the upsert's
            // "unknown id → create" path, so reload the list instead.
            try {
              await refetch();
            } catch {
              /* the caller still moves the selection off the dead id */
            }
            return { kind: 'missing' };
          }
          return { kind: 'failed' };
        }
        fresh = toUiProfile(r.data);
      } catch {
        return { kind: 'failed' };
      }
      const before = get().profiles.find((p) => p.id === fresh.id);
      if (!before) return { kind: 'unchanged' };
      if (JSON.stringify(before) === JSON.stringify(fresh)) {
        return { kind: 'unchanged' };
      }
      set((s) => ({
        profiles: s.profiles.map((p) => (p.id === fresh.id ? fresh : p)),
      }));
      return { kind: 'changed', before };
    },

    exportProfile: async (id, path) => {
      try {
        await unwrap(commands.exportProfile(id, path));
        return true;
      } catch (e) {
        toast.error(message(e));
        return false;
      }
    },

    importProfiles: async (path) => {
      try {
        const imported = await unwrap(commands.importProfiles(path));
        await refetch();
        return imported.map(toUiProfile);
      } catch (e) {
        toast.error(message(e));
        return null;
      }
    },

    setDraft: (draft) => set({ draft }),
    setWorking: (working) => set({ working }),
    clearEditor: () => set({ draft: null, working: null }),
  };
});
