/**
 * Settings store — the UI's connection to the persisted app configuration
 * (`config.toml`, exposed by `get_app_settings` / `set_app_settings`).
 *
 * Writes are OPTIMISTIC: `update` applies `next` to local state immediately so
 * the controls feel instant, then persists it. The backend validates and
 * returns the saved (normalized) value, which we reconcile into state — so a
 * server clamp or serde default is reflected without a second round-trip. A
 * failed save ROLLS BACK to the last value the backend CONFIRMED and surfaces
 * an error toast, leaving the UI consistent with what's actually on disk.
 *
 * Writes are also SERIALIZED: at most one `set_app_settings` is in flight. An
 * update that arrives meanwhile is parked as the single pending value (a newer
 * one replaces it — every update carries the whole bundle, so the latest value
 * already contains the earlier changes) and is sent once the in-flight write
 * settles. Two concurrent IPC calls would each run on their own blocking task,
 * and `CONFIG_LOCK` orders the writes but not *which* lands last — so without
 * this queue an older bundle could win on disk while the UI shows the newer.
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
  'idle' | 'loading' | 'ready' | 'unavailable' | 'error';

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
   * Optimistically apply and persist `next`. Resolves `true` when the write
   * carrying it was saved (state reconciled to the server value) or `false`
   * when it failed (rolled back, toast shown). An update superseded by a newer
   * one before it was sent resolves with the outcome of that newer write,
   * which includes it.
   */
  update: (next: AppSettings) => Promise<boolean>;
  /**
   * Convenience over {@link update}: merge a per-section patch onto the current
   * settings and persist. Resolves `false` (a no-op) when nothing is loaded.
   */
  patch: (partial: SettingsPatch) => Promise<boolean>;
  /**
   * Record a `default_profile` the backend already persisted through another
   * command (`set_default_profile`). Updates the visible state, the confirmed
   * baseline a rollback returns to, and any queued write, so none of them can
   * later put the old default back.
   */
  syncDefaultProfile: (id: string) => void;
}

/** Turn an unknown thrown value into a user-facing message. */
function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * Apply a section-wise shallow patch onto the current settings. `base` is
 * spread first so server-computed fields the patch says nothing about — notably
 * `meta` (config health + engine limits) — survive the round-trip instead of
 * being dropped on the floor.
 */
function merge(base: AppSettings, p: SettingsPatch): AppSettings {
  return {
    ...base,
    app: { ...base.app, ...p.app },
    engine: { ...base.engine, ...p.engine },
    updater: { ...base.updater, ...p.updater },
    telemetry: { ...base.telemetry, ...p.telemetry },
  };
}

/**
 * The write queue's bookkeeping. Module-level rather than store state because
 * none of it is rendered; {@link resetSettingsWriteQueue} clears it for tests.
 */
interface WriteQueue {
  /**
   * The last bundle known to be on disk: the idle state when a burst of writes
   * starts, then each successful save's response.
   */
  confirmed: AppSettings | null;
  /** True while a `set_app_settings` call is in flight. */
  inFlight: boolean;
  /** The newest bundle waiting for the in-flight write to settle. */
  pending: AppSettings | null;
  /** Callers whose value is folded into `pending`, resolved with its outcome. */
  waiters: Array<(ok: boolean) => void>;
}

const queue: WriteQueue = {
  confirmed: null,
  inFlight: false,
  pending: null,
  waiters: [],
};

/** Test hook: forget the confirmed baseline and any queued write. */
export function resetSettingsWriteQueue(): void {
  queue.confirmed = null;
  queue.inFlight = false;
  queue.pending = null;
  queue.waiters = [];
}

export const useSettingsStore = create<SettingsState>()((set, get) => {
  /**
   * Drain the queue: send the pending bundle, settle its waiters, repeat until
   * nothing new arrived meanwhile. Only a write that leaves the queue empty
   * touches visible state — an older response must never overwrite a newer
   * optimistic value the user is already looking at.
   */
  async function drain(): Promise<void> {
    queue.inFlight = true;
    try {
      while (queue.pending) {
        const value = queue.pending;
        const waiters = queue.waiters;
        queue.pending = null;
        queue.waiters = [];
        let ok: boolean;
        try {
          const saved = await setAppSettings(value);
          queue.confirmed = saved;
          if (!queue.pending) set({ settings: saved });
          ok = true;
        } catch (e) {
          // Nothing queued behind us: unwind to what is really on disk. With a
          // newer bundle queued, leave the optimistic state alone — that write
          // decides the final value (and rolls back itself if it fails too).
          if (!queue.pending) set({ settings: queue.confirmed });
          toast.error(message(e));
          ok = false;
        }
        for (const w of waiters) w(ok);
      }
    } finally {
      queue.inFlight = false;
    }
  }

  return {
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

    update: (next) => {
      // With no write in flight the value on screen IS what is on disk (every
      // drain ends reconciled to a saved or rolled-back value), so it is the
      // baseline a failure of this burst of writes returns to.
      if (!queue.inFlight) queue.confirmed = get().settings;
      // Optimistic: reflect the change immediately so controls feel instant.
      set({ settings: next });
      const done = new Promise<boolean>((resolve) => {
        queue.pending = next;
        queue.waiters.push(resolve);
      });
      if (!queue.inFlight) void drain();
      return done;
    },

    patch: async (partial) => {
      const cur = get().settings;
      if (!cur) return false;
      return get().update(merge(cur, partial));
    },

    syncDefaultProfile: (id) => {
      const withDefault = (s: AppSettings): AppSettings =>
        s.engine.default_profile === id
          ? s
          : { ...s, engine: { ...s.engine, default_profile: id } };
      if (queue.confirmed) queue.confirmed = withDefault(queue.confirmed);
      if (queue.pending) queue.pending = withDefault(queue.pending);
      const cur = get().settings;
      if (cur) set({ settings: withDefault(cur) });
    },
  };
});
