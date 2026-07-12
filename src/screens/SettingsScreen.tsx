import { useEffect, useState, type ReactNode } from 'react';
import {
  MPInput,
  MPSectionLabel,
  MPSegmented,
  MPSelect,
  MPSwitch,
  MPTopBar,
} from '@/components/ui';
import type { Density, ResolvedTheme, ThemeMode } from '@/lib/theme';
import type { UpdateChannel } from '@/lib/ipc';
import { useSettingsStore } from '@/stores/settingsStore';
import { useProfilesStore } from '@/stores/profilesStore';

interface SettingsScreenProps {
  /** Resolved (light/dark) theme — always present for the 2-way fallback. */
  theme: ResolvedTheme;
  setTheme: (t: ResolvedTheme) => void;
  density: Density;
  setDensity: (d: Density) => void;
  /**
   * Optional 3-way theme mode. When `mode`/`setMode` are supplied the theme
   * control renders Light/Dark/System and drives `setMode`; otherwise it falls
   * back to the 2-way Light/Dark control on `setTheme`.
   */
  mode?: ThemeMode;
  setMode?: (m: ThemeMode) => void;
}

interface RowProps {
  label: string;
  hint?: string;
  /** Dim + block interaction on the control (backend not reachable). */
  disabled?: boolean;
  children: ReactNode;
}

function Row({ label, hint, disabled, children }: RowProps) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '1fr auto',
        gap: 20,
        padding: '14px 0',
        borderBottom: '1px solid var(--mp-border)',
        alignItems: 'center',
      }}
    >
      <div>
        <div style={{ fontSize: 13, fontWeight: 500 }}>{label}</div>
        {hint && (
          <div
            style={{
              fontSize: 11.5,
              color: 'var(--mp-textMuted)',
              marginTop: 2,
            }}
          >
            {hint}
          </div>
        )}
      </div>
      <div
        style={disabled ? { opacity: 0.45, pointerEvents: 'none' } : undefined}
      >
        {children}
      </div>
    </div>
  );
}

/** A full-width muted note (milestone caveats, consent copy, load states). */
function Note({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        fontSize: 11.5,
        color: 'var(--mp-textMuted)',
        lineHeight: 1.5,
        padding: '12px 0',
      }}
    >
      {children}
    </div>
  );
}

interface EngineFieldProps {
  label: string;
  hint: string;
  suffix: string;
  value: number;
  /** Whole-number bounds check; commit is skipped (error shown) when it fails. */
  validate: (n: number) => boolean;
  errorHint: string;
  onCommit: (n: number) => void;
  disabled: boolean;
}

/**
 * A numeric engine setting with LOCAL text state. Typing never persists; the
 * value is committed on blur or Enter, and only when it parses to an in-bounds
 * whole number — an out-of-range entry shows an inline error and is NOT sent to
 * the backend. The field re-syncs to `value` whenever the persisted setting
 * changes (initial load, or a server clamp echoed back).
 */
function EngineField({
  label,
  hint,
  suffix,
  value,
  validate,
  errorHint,
  onCommit,
  disabled,
}: EngineFieldProps) {
  const [text, setText] = useState(String(value));
  const [error, setError] = useState(false);

  useEffect(() => {
    setText(String(value));
    setError(false);
  }, [value]);

  const commit = () => {
    const s = text.trim();
    const n = Number(s);
    if (!/^\d+$/.test(s) || !validate(n)) {
      setError(true);
      return;
    }
    setError(false);
    if (n !== value) onCommit(n);
  };

  return (
    <Row label={label} hint={hint} disabled={disabled}>
      {/* onBlur bubbles from the inner input (React focusout), so it fires
          when the field loses focus even though MPInput owns no blur prop. */}
      <div
        onBlur={commit}
        style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'flex-end',
          gap: 4,
        }}
      >
        <MPInput
          mono
          value={text}
          suffix={suffix}
          error={error}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit();
          }}
        />
        {error && (
          <div style={{ fontSize: 11, color: 'var(--mp-error)' }} role="alert">
            {errorHint}
          </div>
        )}
      </div>
    </Row>
  );
}

export function SettingsScreen({
  theme,
  setTheme,
  density,
  setDensity,
  mode,
  setMode,
}: SettingsScreenProps) {
  const settings = useSettingsStore((s) => s.settings);
  const status = useSettingsStore((s) => s.status);
  const initSettings = useSettingsStore((s) => s.init);
  const patch = useSettingsStore((s) => s.patch);

  const profiles = useProfilesStore((s) => s.profiles);
  const setDefaultProfile = useProfilesStore((s) => s.setDefault);
  const initProfiles = useProfilesStore((s) => s.init);

  useEffect(() => {
    void initSettings();
  }, [initSettings]);
  useEffect(() => {
    void initProfiles();
  }, [initProfiles]);

  // Reconcile appearance once the backend config loads: it's the source of
  // truth, so on divergence it overrides the localStorage instant-boot cache.
  // Runs on the settings identity change (load, and the update reconcile);
  // idempotent, so it settles without fighting a fresh user change.
  useEffect(() => {
    if (status !== 'ready' || !settings) return;
    const backendTheme = settings.app.theme;
    if (setMode) {
      if (backendTheme !== mode) setMode(backendTheme);
    } else if (backendTheme !== 'system' && backendTheme !== theme) {
      // The 2-way control can't represent 'system'; leave the resolved theme.
      setTheme(backendTheme);
    }
    const backendDensity = settings.app.density ?? 'comfortable';
    if (backendDensity !== density) setDensity(backendDensity);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status, settings]);

  const backendReady = status === 'ready' && settings !== null;
  const backendDisabled = !backendReady;

  // While the backend config is still loading, an appearance change would apply
  // locally and then be silently reverted by the arriving snapshot — so disable
  // the controls until it settles. 'unavailable'/'error' keep them enabled:
  // there is no backend to revert or persist, so appearance stays local-only.
  const appearanceDisabled = status === 'loading';

  const engine = settings?.engine;
  const updater = settings?.updater;
  const telemetry = settings?.telemetry;

  const defaultProfileId =
    profiles.find((p) => p.default)?.id ?? profiles[0]?.id ?? '';

  // Dual-write: the setter props update the localStorage cache + in-memory
  // theme now; persisting the same value to the backend (when ready) keeps the
  // source of truth in step. In the standalone preview (not ready) appearance
  // stays local-only — no backend write.
  const onThemeMode = (m: ThemeMode) => {
    setMode?.(m);
    if (backendReady) void patch({ app: { theme: m } });
  };
  const onThemeResolved = (t: ResolvedTheme) => {
    setTheme(t);
    if (backendReady) void patch({ app: { theme: t } });
  };
  const onDensity = (d: Density) => {
    setDensity(d);
    if (backendReady) void patch({ app: { density: d } });
  };

  return (
    <>
      <MPTopBar title="Settings" />
      <div
        className="mp-scroll"
        style={{
          flex: 1,
          overflow: 'auto',
          padding: '18px 24px',
          maxWidth: 760,
        }}
      >
        <MPSectionLabel>Appearance</MPSectionLabel>
        <Row
          label="Theme"
          hint="Follows your OS by default."
          disabled={appearanceDisabled}
        >
          {mode !== undefined && setMode ? (
            <MPSegmented<ThemeMode>
              value={mode}
              onChange={onThemeMode}
              options={[
                { value: 'light', icon: 'sun', label: 'Light' },
                { value: 'dark', icon: 'moon', label: 'Dark' },
                { value: 'system', label: 'System' },
              ]}
            />
          ) : (
            <MPSegmented<ResolvedTheme>
              value={theme}
              onChange={onThemeResolved}
              options={[
                { value: 'light', icon: 'sun', label: 'Light' },
                { value: 'dark', icon: 'moon', label: 'Dark' },
              ]}
            />
          )}
        </Row>
        <Row
          label="Density"
          hint="Comfortable for reading, compact for big batches."
          disabled={appearanceDisabled}
        >
          <MPSegmented<Density>
            value={density}
            onChange={onDensity}
            options={[
              { value: 'comfortable', label: 'Comfortable' },
              { value: 'compact', label: 'Compact' },
            ]}
          />
        </Row>

        {status === 'unavailable' && (
          <Note>
            Settings below are read from the desktop app. In this preview only
            appearance is adjustable.
          </Note>
        )}
        {status === 'loading' && <Note>Loading settings…</Note>}
        {status === 'error' && (
          <Note>
            Couldn&rsquo;t load settings.{' '}
            <button
              type="button"
              onClick={() => void initSettings()}
              style={{
                background: 'none',
                border: 'none',
                padding: 0,
                color: 'var(--mp-accent)',
                cursor: 'pointer',
                font: 'inherit',
                textDecoration: 'underline',
              }}
            >
              Retry
            </button>
          </Note>
        )}

        <div style={{ height: 24 }} />
        <MPSectionLabel>Printing</MPSectionLabel>
        <Row
          label="Default profile"
          hint="Applied to a job when no other profile is chosen."
          disabled={profiles.length === 0}
        >
          <MPSelect
            value={defaultProfileId}
            onChange={(e) => void setDefaultProfile(e.target.value)}
            options={profiles.map((p) => ({ value: p.id, label: p.name }))}
          />
        </Row>
        <Note>
          Right-click printing arrives with the shell integration milestone
          (M2).
        </Note>

        <div style={{ height: 24 }} />
        <MPSectionLabel>Engine</MPSectionLabel>
        <EngineField
          label="Concurrent conversions"
          hint="Applies after restart."
          suffix="jobs"
          value={engine?.max_concurrent_conversions ?? 2}
          validate={(n) => n >= 1 && n <= 8}
          errorHint="Enter a whole number from 1 to 8."
          onCommit={(n) =>
            void patch({ engine: { max_concurrent_conversions: n } })
          }
          disabled={backendDisabled}
        />
        <EngineField
          label="Conversion timeout"
          hint="0 disables the timeout."
          suffix="seconds"
          value={engine?.conversion_timeout_seconds ?? 60}
          validate={(n) => n === 0 || (n >= 10 && n <= 3600)}
          errorHint="Enter 0, or a whole number from 10 to 3600."
          onCommit={(n) =>
            void patch({ engine: { conversion_timeout_seconds: n } })
          }
          disabled={backendDisabled}
        />
        <EngineField
          label="Max file size"
          hint="Largest input accepted for printing."
          suffix="MB"
          value={engine?.max_file_size_mb ?? 200}
          validate={(n) => n >= 1 && n <= 4096}
          errorHint="Enter a whole number from 1 to 4096."
          onCommit={(n) => void patch({ engine: { max_file_size_mb: n } })}
          disabled={backendDisabled}
        />

        <div style={{ height: 24 }} />
        <MPSectionLabel>Updates</MPSectionLabel>
        <Row label="Channel" disabled={backendDisabled}>
          <MPSegmented<UpdateChannel>
            value={updater?.channel ?? 'stable'}
            onChange={(c) => void patch({ updater: { channel: c } })}
            options={[
              { value: 'stable', label: 'Stable' },
              { value: 'beta', label: 'Beta' },
              { value: 'nightly', label: 'Nightly' },
            ]}
          />
        </Row>
        <Note>
          Channel selection takes effect when the auto-updater ships (M4).
        </Note>
        <Row
          label="Check on startup"
          hint="Delayed a few seconds to keep launch fast."
          disabled={backendDisabled}
        >
          <MPSwitch
            checked={updater?.check_on_startup ?? true}
            onChange={() =>
              void patch({
                updater: {
                  check_on_startup: !(updater?.check_on_startup ?? true),
                },
              })
            }
          />
        </Row>

        <div style={{ height: 24 }} />
        <MPSectionLabel>Privacy</MPSectionLabel>
        <Row
          label="Crash reports"
          hint="Off by default. Sent only when toggled on."
          disabled={backendDisabled}
        >
          <MPSwitch
            checked={telemetry?.crash_reports ?? false}
            onChange={() =>
              void patch({
                telemetry: {
                  crash_reports: !(telemetry?.crash_reports ?? false),
                },
              })
            }
          />
        </Row>
        <Row
          label="Anonymous usage metrics"
          hint="MaeclyPrint never enables this without consent."
          disabled={backendDisabled}
        >
          <MPSwitch
            checked={telemetry?.anonymous_usage_metrics ?? false}
            onChange={() =>
              void patch({
                telemetry: {
                  anonymous_usage_metrics: !(
                    telemetry?.anonymous_usage_metrics ?? false
                  ),
                },
              })
            }
          />
        </Row>
        <Note>
          No data is sent in this version — these switches record consent only.
        </Note>
      </div>
    </>
  );
}
