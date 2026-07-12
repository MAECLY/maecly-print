/**
 * Settings store — the UI's connection to the persisted app configuration
 * (`config.toml`, exposed by `get_app_settings` / `set_app_settings`).
 *
 * Writes are OPTIMISTIC: `update` applies `next` to local state immediately so
 * the controls feel instant, then persists it. The backend validates and
 * returns the saved (normalized) value, which we reconcile into state — so a
 * server clamp or serde default is reflected without a second round-trip. A
 * failed save ROLLS BACK to the previous value and surfaces an error toast,
 * leaving the UI consistent with what's actually on disk.
 *
 * The store is PURE: it never touches the DOM or localStorage. Appearance
 * (theme/density) also has a localStorage-backed instant-boot cache owned by
 * `src/lib/theme.ts`; the backend config here is the source of truth and wins
 * on divergence. The screen (and later App) reconciles the two after
 * init/update — see the appearance reconcile effect in `SettingsScreen`. That
 * split keeps this store free of side effects and independently testable.
 */

import { create } from 'zustand';
import { toast } from 'sonner';

import {
  getAppSettings,
  isTauri,
  setAppSettings,
  type AppCfg,
  type AppSettings,
  type EngineCfg,
  type TelemetryCfg,
  type UpdaterCfg,
} from '@/lib/ipc';

/** Lifecycle of the store's connection to the persisted config. */
export type SettingsStatus =
  | 'idle'
  | 'loading'
  | 'ready'
  | 'unavailable'
  | 'error';

/**
 * A shallow, per-section patch of the settings (see {@link patch}). Each
 * present section is merged onto the current value, so callers can change one
 * field (`{ app: { theme } }`) without restating the rest.
 */
export interface SettingsPatch {
  app?: Partial<AppCfg>;
  engine?: Partial<EngineCfg>;
  updater?: Partial<UpdaterCfg>;
  telemetry?: Partial<TelemetryCfg>;
}

interface SettingsState {
  settings: AppSettings | null;
  status: SettingsStatus;
  /** Explicit `| undefined` so `set({ error: undefined })` can clear it. */
  error?: string | undefined;

  /**
   * Load the persisted settings. Idempotent while loading/ready, and
   * re-runnable after a failure so one transient error doesn't leave the
   * Settings UI dead for the session. Reports `unavailable` outside Tauri.
   */
  init: () => Promise<void>;
  /**
   * Optimistically apply and persist `next`. Resolves `true` when saved (state
   * reconciled to the server value) or `false` on failure (rolled back, toast
   * shown).
   */
  update: (next: AppSettings) => Promise<boolean>;
  /**
   * Convenience over {@link update}: merge a per-section patch onto the current
   * settings and persist. Resolves `false` (a no-op) when nothing is loaded.
   */
  patch: (partial: SettingsPatch) => Promise<boolean>;
}

/** Turn an unknown thrown value into a user-facing message. */
function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Apply a section-wise shallow patch onto the current settings. */
function merge(base: AppSettings, p: SettingsPatch): AppSettings {
  return {
    app: { ...base.app, ...p.app },
    engine: { ...base.engine, ...p.engine },
    updater: { ...base.updater, ...p.updater },
    telemetry: { ...base.telemetry, ...p.telemetry },
  };
}

export const useSettingsStore = create<SettingsState>()((set, get) => ({
  settings: null,
  status: 'idle',

  init: async () => {
    const status = get().status;
    if (status !== 'idle' && status !== 'error') return;
    if (!isTauri()) {
      set({ status: 'unavailable' });
      return;
    }
    set({ status: 'loading', error: undefined });
    try {
      const settings = await getAppSettings();
      set({ settings, status: 'ready' });
    } catch (e) {
      set({ status: 'error', error: message(e) });
    }
  },

  update: async (next) => {
    const prev = get().settings;
    // Optimistic: reflect the change immediately so controls feel instant.
    set({ settings: next });
    try {
      // The backend validates + normalizes; reconcile to whatever it saved
      // (e.g. a clamped value) so the UI matches disk exactly.
      const saved = await setAppSettings(next);
      // Latest-wins: only reconcile if our optimistic value is still current.
      // If a newer overlapping update has superseded it, this stale response
      // must not clobber the newer state — leave it untouched.
      set((s) => (s.settings === next ? { settings: saved } : {}));
      return true;
    } catch (e) {
      // Roll back to what we captured — but only if ours is still the latest
      // optimistic value, so we never resurrect a snapshot a newer update
      // already replaced. The last overlapping failure unwinds to `prev`.
      set((s) => (s.settings === next ? { settings: prev } : {}));
      toast.error(message(e));
      return false;
    }
  },

  patch: async (partial) => {
    const cur = get().settings;
    if (!cur) return false;
    return get().update(merge(cur, partial));
  },
}));
