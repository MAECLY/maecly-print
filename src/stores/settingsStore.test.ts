import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { toast } from 'sonner';

import type { AppSettings } from '@/ipc/bindings';
import { fixtureAppSettings } from '@/test/fixtures';
import { useSettingsStore } from '@/stores/settingsStore';

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

interface MockOpts {
  /** What `get_app_settings` returns (defaults to the fixture). */
  initial?: AppSettings;
  /**
   * What `set_app_settings` returns. Omitted → echoes the sent settings (the
   * common case); set it to simulate server normalization/clamping.
   */
  saved?: AppSettings;
  /** When set, that command throws `boom`. */
  fail?: string;
}

/** Install settings IPC mocks and return the list of commands invoked. */
function mockSettings(opts: MockOpts = {}): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === opts.fail) throw new Error('boom');
    if (cmd === 'get_app_settings') return opts.initial ?? fixtureAppSettings;
    if (cmd === 'set_app_settings')
      return opts.saved ?? (args as { settings: AppSettings }).settings;
    return null;
  });
  return calls;
}

function resetStore() {
  useSettingsStore.setState({
    settings: null,
    status: 'idle',
    error: undefined,
  });
}

/** A raw wire payload that omits the serde-default fields (density, telemetry). */
const sparseSettings = {
  app: { locale: 'en-US', theme: 'system', launch_minimized: false },
  engine: {
    max_concurrent_conversions: 2,
    conversion_timeout_seconds: 60,
    max_file_size_mb: 200,
    default_profile: null,
  },
  updater: { channel: 'stable', check_on_startup: true },
  telemetry: {},
} as unknown as AppSettings;

describe('settingsStore', () => {
  beforeEach(() => resetStore());
  afterEach(() => clearMocks());

  it('loads and normalizes the settings on init', async () => {
    mockSettings({ initial: sparseSettings });
    await useSettingsStore.getState().init();

    const s = useSettingsStore.getState();
    expect(s.status).toBe('ready');
    // The optional serde-default fields are filled in by the IPC adapter.
    expect(s.settings?.app.density).toBe('comfortable');
    expect(s.settings?.telemetry).toEqual({
      crash_reports: false,
      anonymous_usage_metrics: false,
    });
  });

  it('reports unavailable outside the Tauri host', async () => {
    // clearMocks() leaves __TAURI_INTERNALS__ around; drop it for a true no-host.
    const w = window as unknown as {
      __TAURI_INTERNALS__?: unknown;
      __TAURI__?: unknown;
    };
    delete w.__TAURI_INTERNALS__;
    delete w.__TAURI__;

    await useSettingsStore.getState().init();
    expect(useSettingsStore.getState().status).toBe('unavailable');
  });

  it('is idempotent — a second init does not re-fetch', async () => {
    const calls = mockSettings();
    await useSettingsStore.getState().init();
    await useSettingsStore.getState().init();
    expect(calls.filter((c) => c.cmd === 'get_app_settings')).toHaveLength(1);
  });

  it('re-runs init after an error (retryable)', async () => {
    mockSettings({ fail: 'get_app_settings' });
    await useSettingsStore.getState().init();
    expect(useSettingsStore.getState().status).toBe('error');
    expect(useSettingsStore.getState().error).toBe('boom');

    // A fresh, healthy mock lets the retry succeed.
    clearMocks();
    mockSettings();
    await useSettingsStore.getState().init();
    expect(useSettingsStore.getState().status).toBe('ready');
    expect(useSettingsStore.getState().settings).toEqual(fixtureAppSettings);
  });

  it('persists an update and sends the full settings payload', async () => {
    const calls = mockSettings();
    await useSettingsStore.getState().init();

    const next: AppSettings = {
      ...fixtureAppSettings,
      updater: { ...fixtureAppSettings.updater, channel: 'beta' },
    };
    const ok = await useSettingsStore.getState().update(next);

    expect(ok).toBe(true);
    const save = calls.find((c) => c.cmd === 'set_app_settings');
    expect(save?.args.settings).toEqual(next);
    expect(useSettingsStore.getState().settings?.updater.channel).toBe('beta');
  });

  it('applies the change optimistically before the save resolves', async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    mockIPC((cmd, args) => {
      if (cmd === 'get_app_settings') return fixtureAppSettings;
      if (cmd === 'set_app_settings')
        return gate.then(() => (args as { settings: AppSettings }).settings);
      return null;
    });
    await useSettingsStore.getState().init();

    const next: AppSettings = {
      ...fixtureAppSettings,
      updater: { ...fixtureAppSettings.updater, check_on_startup: false },
    };
    const p = useSettingsStore.getState().update(next);
    // Optimistic: state reflects `next` while the save is still in flight.
    expect(useSettingsStore.getState().settings?.updater.check_on_startup).toBe(
      false,
    );

    release();
    await p;
    expect(useSettingsStore.getState().settings?.updater.check_on_startup).toBe(
      false,
    );
  });

  it('rolls back and toasts when the save fails', async () => {
    mockSettings({ fail: 'set_app_settings' });
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    await useSettingsStore.getState().init();

    const before = useSettingsStore.getState().settings;
    const next: AppSettings = {
      ...fixtureAppSettings,
      updater: { ...fixtureAppSettings.updater, channel: 'nightly' },
    };
    const ok = await useSettingsStore.getState().update(next);

    expect(ok).toBe(false);
    expect(useSettingsStore.getState().settings).toEqual(before);
    expect(spy).toHaveBeenCalledWith('boom');
    spy.mockRestore();
  });

  it('reconciles state to the server-returned (clamped) value', async () => {
    const clamped: AppSettings = {
      ...fixtureAppSettings,
      engine: { ...fixtureAppSettings.engine, max_concurrent_conversions: 8 },
    };
    const calls = mockSettings({ saved: clamped });
    await useSettingsStore.getState().init();

    // Caller asks for an out-of-range value; the server clamps and echoes it.
    const next: AppSettings = {
      ...fixtureAppSettings,
      engine: { ...fixtureAppSettings.engine, max_concurrent_conversions: 999 },
    };
    await useSettingsStore.getState().update(next);

    const sent = calls.find((c) => c.cmd === 'set_app_settings')!.args
      .settings as AppSettings;
    expect(sent.engine.max_concurrent_conversions).toBe(999);
    // State holds the server's clamped value, not what we sent.
    expect(
      useSettingsStore.getState().settings?.engine.max_concurrent_conversions,
    ).toBe(8);
  });

  it('patch merges one section and persists the whole settings', async () => {
    const calls = mockSettings();
    await useSettingsStore.getState().init();

    const ok = await useSettingsStore
      .getState()
      .patch({ telemetry: { crash_reports: true } });

    expect(ok).toBe(true);
    const sent = calls.find((c) => c.cmd === 'set_app_settings')!.args
      .settings as AppSettings;
    // Only crash_reports changed; the rest of the section and others intact.
    expect(sent.telemetry).toEqual({
      crash_reports: true,
      anonymous_usage_metrics: false,
    });
    expect(sent.app).toEqual(fixtureAppSettings.app);
    expect(useSettingsStore.getState().settings?.telemetry.crash_reports).toBe(
      true,
    );
  });

  it('patch of app.theme sends theme in the app section, preserving the rest', async () => {
    const calls = mockSettings();
    await useSettingsStore.getState().init();

    await useSettingsStore.getState().patch({ app: { theme: 'dark' } });

    const sent = calls.find((c) => c.cmd === 'set_app_settings')!.args
      .settings as AppSettings;
    expect(sent.app.theme).toBe('dark');
    expect(sent.app.density).toBe('comfortable');
    expect(sent.app.locale).toBe('en-US');
  });

  it('regression: patch preserves the server-computed meta (limits + config health)', async () => {
    // `merge` used to rebuild the bundle from its four sections, dropping
    // `meta` — which would blank the engine limits and hide the corrupt-config
    // warning after the very first change the user makes.
    const withMeta: AppSettings = {
      ...fixtureAppSettings,
      meta: {
        config_unreadable: true,
        config_path: '/tmp/config.toml',
        limits: {
          max_concurrent_conversions: {
            min: 1,
            max: 8,
            default: 2,
            zero: 'clamp',
          },
          conversion_timeout_seconds: {
            min: 10,
            max: 3600,
            default: 60,
            zero: 'disables',
          },
          max_file_size_mb: {
            min: 1,
            max: 4096,
            default: 200,
            zero: 'default',
          },
        },
      },
    };
    const calls = mockSettings({ initial: withMeta });
    await useSettingsStore.getState().init();

    await useSettingsStore.getState().patch({ app: { theme: 'dark' } });

    const sent = calls.find((c) => c.cmd === 'set_app_settings')!.args
      .settings as AppSettings;
    expect(sent.meta).toEqual(withMeta.meta);
    expect(useSettingsStore.getState().settings?.meta?.limits).toEqual(
      withMeta.meta?.limits,
    );
  });

  it('patch is a no-op before settings load', async () => {
    const calls = mockSettings();
    const ok = await useSettingsStore
      .getState()
      .patch({ app: { theme: 'dark' } });
    expect(ok).toBe(false);
    expect(calls.some((c) => c.cmd === 'set_app_settings')).toBe(false);
  });

  // ---- Overlapping updates (latest-wins guard) --------------------------
  // Each set_app_settings gets a deferred promise the test resolves by hand,
  // so we can drive the resolution order of two in-flight updates.

  interface Deferred {
    resolve: (v: AppSettings) => void;
    reject: (e: unknown) => void;
    settings: AppSettings;
  }

  /** Install mocks whose set_app_settings resolution is externally controlled. */
  function mockDeferredSaves(): Deferred[] {
    const deferreds: Deferred[] = [];
    mockIPC((cmd, args) => {
      if (cmd === 'get_app_settings') return fixtureAppSettings;
      if (cmd === 'set_app_settings') {
        const settings = (args as { settings: AppSettings }).settings;
        return new Promise<AppSettings>((resolve, reject) => {
          deferreds.push({ resolve, reject, settings });
        });
      }
      return null;
    });
    return deferreds;
  }

  it('latest-wins: a slow success for #1 does not clobber a newer #2', async () => {
    const deferreds = mockDeferredSaves();
    await useSettingsStore.getState().init();
    const S0 = useSettingsStore.getState().settings!;

    const next1: AppSettings = {
      ...S0,
      updater: { ...S0.updater, channel: 'beta' },
    };
    const next2: AppSettings = {
      ...S0,
      updater: { ...S0.updater, channel: 'nightly' },
    };

    const p1 = useSettingsStore.getState().update(next1);
    const p2 = useSettingsStore.getState().update(next2);
    // Both optimistic writes have landed; #2 is the newest.
    expect(useSettingsStore.getState().settings?.updater.channel).toBe(
      'nightly',
    );

    // #2 resolves first (fast), echoing what it was sent.
    deferreds[1]!.resolve(deferreds[1]!.settings);
    expect(await p2).toBe(true);
    expect(useSettingsStore.getState().settings?.updater.channel).toBe(
      'nightly',
    );

    // #1 resolves last (slow); its stale success must NOT clobber #2.
    deferreds[0]!.resolve(deferreds[0]!.settings);
    expect(await p1).toBe(true);
    expect(useSettingsStore.getState().settings?.updater.channel).toBe(
      'nightly',
    );
  });

  it('latest-wins: two overlapping failures roll back to the original, not #1', async () => {
    const spy = vi.spyOn(toast, 'error').mockReturnValue('id');
    const deferreds = mockDeferredSaves();
    await useSettingsStore.getState().init();
    const S0 = useSettingsStore.getState().settings!;

    const next1: AppSettings = {
      ...S0,
      app: { ...S0.app, theme: 'dark' },
    };
    const next2: AppSettings = {
      ...S0,
      app: { ...S0.app, theme: 'light' },
    };

    const p1 = useSettingsStore.getState().update(next1);
    const p2 = useSettingsStore.getState().update(next2);
    expect(useSettingsStore.getState().settings).toEqual(next2);

    // #2 (newest) fails first → unwinds to #1's optimistic value…
    deferreds[1]!.reject(new Error('boom'));
    expect(await p2).toBe(false);
    // …then #1 fails → unwinds to the ORIGINAL persisted S0, not next1.
    deferreds[0]!.reject(new Error('boom'));
    expect(await p1).toBe(false);

    expect(useSettingsStore.getState().settings).toEqual(S0);
    expect(useSettingsStore.getState().settings).not.toEqual(next1);
    spy.mockRestore();
  });
});
