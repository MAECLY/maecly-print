import { useEffect, useState, version as REACT_VERSION } from 'react';
// The running @tauri-apps/api version, read from its own package.json (the
// package explicitly exports "./package.json"). This reflects the resolved
// dependency, not the caret range in our package.json — so it stays honest
// as the lockfile bumps. React's version comes from its runtime `version`
// export; the app's own version is authoritative from Rust (`app_info`).
import tauriPkg from '@tauri-apps/api/package.json';
import { toast } from 'sonner';
import { MPAppIcon } from '@/components/Brand';
import { Icon } from '@/components/Icon';
import { MPButton, MPCard, MPSectionLabel, MPTopBar } from '@/components/ui';
import {
  checkForUpdates,
  fetchAppInfo,
  isTauri,
  openExternal,
  openLogsFolder,
  reportIssue,
  type AppInfo,
  type UpdateCheck,
} from '@/lib/ipc';
import {
  DOCS_URL,
  LICENSE_URL,
  RELEASES_URL,
  REPO_URL,
  UNKNOWN_VERSION,
} from '@/lib/repo';
import { useSettingsStore } from '@/stores/settingsStore';

const TAURI_VERSION = tauriPkg.version;

/**
 * The update-check flow as an explicit state machine: no status card in `idle`,
 * a spinner while `checking`, and a real result (`done`) or the thrown,
 * user-facing message (`error`) afterwards.
 */
type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'done'; check: UpdateCheck }
  | { kind: 'error'; message: string };

/** RFC 3339 UTC → a local `HH:MM` clock; unparseable → `—`. */
function formatClock(rfc3339: string): string {
  const d = new Date(rfc3339);
  if (Number.isNaN(d.getTime())) return '—';
  return d.toLocaleTimeString([], {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
  });
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function AboutScreen() {
  const tauri = isTauri();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [infoError, setInfoError] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateState>({ kind: 'idle' });

  const settings = useSettingsStore((s) => s.settings);
  const initSettings = useSettingsStore((s) => s.init);

  useEffect(() => {
    // Idempotent — App may also init the store; a second call is a no-op.
    initSettings();
  }, [initSettings]);

  useEffect(() => {
    let alive = true;
    fetchAppInfo()
      .then((i) => {
        if (alive) setInfo(i);
      })
      .catch((e: unknown) => {
        if (alive) setInfoError(errText(e));
      });
    return () => {
      alive = false;
    };
  }, []);

  async function runCheck() {
    setUpdate({ kind: 'checking' });
    try {
      const check = await checkForUpdates();
      setUpdate({ kind: 'done', check });
    } catch (e) {
      setUpdate({ kind: 'error', message: errText(e) });
    }
  }

  async function open(url: string) {
    try {
      await openExternal(url);
    } catch (e) {
      toast.error(errText(e));
    }
  }

  async function revealLogs() {
    try {
      await openLogsFolder();
    } catch (e) {
      toast.error(errText(e));
    }
  }

  async function fileIssue() {
    try {
      // Empty user text is fine: the backend prefills version/os into the body.
      await reportIssue('');
    } catch (e) {
      toast.error(errText(e));
    }
  }

  const channel = settings?.updater.channel;
  // Without a backend there is no version to show — say so rather than repeat a
  // frontend literal that goes stale on the first release bump.
  const versionLine = info
    ? `${info.version} · build ${info.build_date}${channel ? ` · ${channel} channel` : ''}`
    : `${UNKNOWN_VERSION} · standalone web preview`;

  const components: { name: string; value: string; license: string }[] = [
    {
      name: 'MaeclyPrint',
      value: info?.version ?? UNKNOWN_VERSION,
      license: 'GPL-3.0',
    },
    { name: 'React', value: REACT_VERSION, license: 'MIT' },
    { name: 'Tauri', value: TAURI_VERSION, license: 'MIT/Apache-2.0' },
  ];

  return (
    <>
      <MPTopBar title="About" />
      <div
        className="mp-scroll"
        style={{ flex: 1, overflow: 'auto', padding: '24px 28px' }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 16,
            marginBottom: 18,
          }}
        >
          <MPAppIcon size={56} />
          <div>
            <div
              style={{
                fontSize: 18,
                fontWeight: 600,
                letterSpacing: '-0.01em',
              }}
            >
              MaeclyPrint
            </div>
            <div
              className="mp-mono"
              style={{
                fontSize: 11.5,
                color: 'var(--mp-textMuted)',
                marginTop: 2,
              }}
            >
              {versionLine}
            </div>
            <div
              style={{
                fontSize: 12,
                color: 'var(--mp-textMuted)',
                marginTop: 6,
                maxWidth: 420,
              }}
            >
              Open source batch printing for Windows and Mac. No 15-file limit,
              no Adobe required, no $149 license fee.
            </div>
          </div>
        </div>

        <div style={{ display: 'flex', gap: 8, marginBottom: 24 }}>
          {update.kind === 'checking' ? (
            <MPButton variant="primary" size="sm" disabled>
              <span
                style={{
                  width: 12,
                  height: 12,
                  border: '2px solid rgba(255,255,255,0.4)',
                  borderTopColor: '#fff',
                  borderRadius: '50%',
                  animation: 'mp-spin 0.9s linear infinite',
                  display: 'inline-block',
                }}
              />
              Checking…
            </MPButton>
          ) : (
            <MPButton
              variant="primary"
              icon="rotate"
              size="sm"
              disabled={!tauri}
              onClick={() => void runCheck()}
            >
              Check for updates
            </MPButton>
          )}
        </div>

        {update.kind === 'done' && !update.check.available && (
          <>
            <MPSectionLabel>Updates</MPSectionLabel>
            <MPCard padding={14} style={{ marginBottom: 24 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <div
                  style={{
                    width: 32,
                    height: 32,
                    borderRadius: '50%',
                    background: 'var(--mp-successSoft)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                  }}
                >
                  <Icon name="check" size={16} color="var(--mp-success)" />
                </div>
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 13, fontWeight: 500 }}>
                    You&apos;re up to date
                  </div>
                  <div
                    className="mp-mono"
                    style={{
                      fontSize: 11,
                      color: 'var(--mp-textDim)',
                      marginTop: 2,
                    }}
                  >
                    version {update.check.current} · checked{' '}
                    {formatClock(update.check.checked_at)}
                  </div>
                </div>
              </div>
            </MPCard>
          </>
        )}

        {update.kind === 'done' && update.check.available && (
          <>
            <MPSectionLabel>Updates</MPSectionLabel>
            <MPCard
              padding={14}
              style={{
                marginBottom: 24,
                background: 'var(--mp-accentSoft)',
                borderColor: 'var(--mp-accent)',
              }}
            >
              <div
                role="group"
                aria-label="Update available"
                style={{ display: 'flex', alignItems: 'center', gap: 12 }}
              >
                <div
                  style={{
                    width: 32,
                    height: 32,
                    borderRadius: '50%',
                    background: 'var(--mp-surface)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                  }}
                >
                  <Icon name="download" size={16} color="var(--mp-accent)" />
                </div>
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 13, fontWeight: 600 }}>
                    Version {update.check.latest} is available
                  </div>
                  <div
                    style={{
                      fontSize: 11.5,
                      color: 'var(--mp-textMuted)',
                      marginTop: 2,
                    }}
                  >
                    You&apos;re on {update.check.current}. In-app install
                    arrives with the auto-updater (M4); for now, download it
                    from the release page.
                  </div>
                </div>
                <MPButton
                  variant="primary"
                  size="sm"
                  icon="page"
                  onClick={() => void open(update.check.url ?? RELEASES_URL)}
                >
                  Release notes
                </MPButton>
              </div>
            </MPCard>
          </>
        )}

        {update.kind === 'error' && (
          <>
            <MPSectionLabel>Updates</MPSectionLabel>
            <MPCard
              padding={14}
              style={{
                marginBottom: 24,
                background: 'var(--mp-errorSoft)',
                borderColor: 'var(--mp-error)',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
                <Icon name="alert" size={16} color="var(--mp-error)" />
                <div
                  style={{ flex: 1, fontSize: 12.5, color: 'var(--mp-error)' }}
                >
                  {update.message}
                </div>
                <MPButton
                  variant="secondary"
                  size="sm"
                  icon="rotate"
                  onClick={() => void runCheck()}
                >
                  Retry
                </MPButton>
              </div>
            </MPCard>
          </>
        )}

        <MPSectionLabel>Components</MPSectionLabel>
        <MPCard padding={0} style={{ marginBottom: 12 }}>
          {components.map((c, i) => (
            <div
              key={c.name}
              style={{
                display: 'grid',
                gridTemplateColumns: '1fr 120px 140px',
                padding: '10px 14px',
                gap: 12,
                borderBottom:
                  i < components.length - 1
                    ? '1px solid var(--mp-border)'
                    : 'none',
                fontSize: 12.5,
              }}
            >
              <span style={{ fontWeight: 500 }}>{c.name}</span>
              <span
                className="mp-mono"
                style={{ color: 'var(--mp-textMuted)' }}
              >
                {c.value}
              </span>
              <span className="mp-mono" style={{ color: 'var(--mp-textDim)' }}>
                {c.license}
              </span>
            </div>
          ))}
          <div
            style={{
              padding: '10px 14px',
              borderTop: '1px solid var(--mp-border)',
              fontSize: 11.5,
              color: 'var(--mp-textMuted)',
            }}
          >
            Platform:{' '}
            <span className="mp-mono">
              {info ? `${info.os} · ${info.arch}` : '—'}
            </span>
          </div>
        </MPCard>
        <div
          style={{
            fontSize: 11.5,
            color: 'var(--mp-textDim)',
            marginBottom: 24,
            lineHeight: 1.6,
          }}
        >
          Converter tooling versions will appear here once bundled (M3+).
        </div>

        {info && (
          <>
            <MPSectionLabel>Filesystem paths (live from Rust)</MPSectionLabel>
            <MPCard padding={14} style={{ marginBottom: 10 }}>
              <div
                className="mp-mono"
                style={{
                  fontSize: 11,
                  color: 'var(--mp-textMuted)',
                  lineHeight: 1.7,
                }}
              >
                <div>
                  <strong style={{ color: 'var(--mp-text)' }}>config:</strong>{' '}
                  {info.config_dir}
                </div>
                <div>
                  <strong style={{ color: 'var(--mp-text)' }}>data:</strong>{' '}
                  {info.data_dir}
                </div>
                <div>
                  <strong style={{ color: 'var(--mp-text)' }}>logs:</strong>{' '}
                  {info.log_dir}
                </div>
              </div>
            </MPCard>
          </>
        )}
        <div style={{ marginBottom: 24 }}>
          <MPButton
            variant="ghost"
            icon="folder"
            size="sm"
            disabled={!tauri}
            onClick={() => void revealLogs()}
          >
            Open logs folder
          </MPButton>
        </div>

        {infoError && (
          <MPCard
            padding={14}
            style={{
              marginBottom: 24,
              background: 'var(--mp-errorSoft)',
              borderColor: 'var(--mp-error)',
            }}
          >
            <div style={{ fontSize: 12, color: 'var(--mp-error)' }}>
              IPC unavailable: {infoError}. This is expected when running the
              standalone Vite dev server (no Tauri host). Run{' '}
              <span className="mp-mono">pnpm tauri dev</span> to test the real
              bridge.
            </div>
          </MPCard>
        )}

        <MPSectionLabel>Links</MPSectionLabel>
        <div
          style={{
            display: 'flex',
            flexWrap: 'wrap',
            gap: 8,
            marginBottom: 24,
          }}
        >
          <MPButton
            variant="ghost"
            icon="page"
            size="sm"
            disabled={!tauri}
            onClick={() => void open(RELEASES_URL)}
          >
            Release notes
          </MPButton>
          <MPButton
            variant="ghost"
            icon="shield"
            size="sm"
            disabled={!tauri}
            onClick={() => void open(LICENSE_URL)}
          >
            License GPL-3.0
          </MPButton>
          <MPButton
            variant="ghost"
            icon="help"
            size="sm"
            disabled={!tauri}
            onClick={() => void open(DOCS_URL)}
          >
            Documentation
          </MPButton>
          <MPButton
            variant="ghost"
            icon="alert"
            size="sm"
            disabled={!tauri}
            onClick={() => void fileIssue()}
          >
            Report an issue
          </MPButton>
        </div>

        <div
          style={{
            fontSize: 11.5,
            color: 'var(--mp-textDim)',
            lineHeight: 1.7,
          }}
        >
          MaeclyPrint is free software licensed under{' '}
          <span className="mp-mono">GPL-3.0</span>. No telemetry, no cloud, no
          account. Source at{' '}
          <button
            type="button"
            className="mp-mono"
            disabled={!tauri}
            onClick={() => void open(REPO_URL)}
            style={{
              background: 'none',
              border: 'none',
              padding: 0,
              font: 'inherit',
              color: 'var(--mp-accent)',
              cursor: tauri ? 'pointer' : 'default',
              textDecoration: 'underline',
            }}
          >
            {REPO_URL.replace('https://', '')}
          </button>
          .
        </div>
      </div>
    </>
  );
}
