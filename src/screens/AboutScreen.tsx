import { useEffect, useState } from 'react';
import { MPAppIcon } from '@/components/Brand';
import { Icon } from '@/components/Icon';
import { MPButton, MPCard, MPSectionLabel, MPTopBar } from '@/components/ui';
import { fetchAppInfo, type AppInfoLike } from '@/lib/ipc';

const DEPS = [
  { name: 'Tauri', v: '2.1.0', license: 'MIT/Apache-2.0' },
  { name: 'SumatraPDF', v: '3.5.2', license: 'GPLv3' },
  { name: 'LibreOffice', v: 'auto-detect', license: 'MPL-2.0' },
  { name: 'ImageMagick', v: '7.1.x', license: 'ImageMagick' },
  { name: 'Pandoc', v: 'auto-detect', license: 'GPLv2+' },
  { name: 'qpdf', v: '11.x', license: 'Apache-2.0' },
];

export function AboutScreen() {
  const [info, setInfo] = useState<AppInfoLike | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    fetchAppInfo()
      .then((i) => {
        if (alive) setInfo(i);
      })
      .catch((e: unknown) => {
        if (alive) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      alive = false;
    };
  }, []);

  const versionLine = info
    ? `${info.version} · build ${info.build_date} · stable channel`
    : '0.0.1-dev · standalone web preview';

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
              Open source batch printing for Windows and macOS. Right-click →
              print everything.
            </div>
          </div>
        </div>

        <div style={{ display: 'flex', gap: 8, marginBottom: 24 }}>
          <MPButton variant="primary" icon="rotate" size="sm">
            Check for updates
          </MPButton>
          <MPButton variant="ghost" icon="page" size="sm">
            Release notes
          </MPButton>
          <MPButton variant="ghost" icon="shield" size="sm">
            License
          </MPButton>
        </div>

        <MPSectionLabel>Up to date</MPSectionLabel>
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
                You&apos;re running the latest version.
              </div>
              <div
                className="mp-mono"
                style={{
                  fontSize: 11,
                  color: 'var(--mp-textDim)',
                  marginTop: 2,
                }}
              >
                last checked 12 minutes ago
              </div>
            </div>
          </div>
        </MPCard>

        {info && (
          <>
            <MPSectionLabel>Filesystem paths (live from Rust)</MPSectionLabel>
            <MPCard padding={14} style={{ marginBottom: 24 }}>
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

        {error && (
          <MPCard
            padding={14}
            style={{
              marginBottom: 24,
              background: 'var(--mp-errorSoft)',
              borderColor: 'var(--mp-error)',
            }}
          >
            <div style={{ fontSize: 12, color: 'var(--mp-error)' }}>
              IPC unavailable: {error}. This is expected when running the
              standalone Vite dev server (no Tauri host). Run{' '}
              <span className="mp-mono">pnpm tauri dev</span> to test the real
              bridge.
            </div>
          </MPCard>
        )}

        <MPSectionLabel>Bundled dependencies</MPSectionLabel>
        <MPCard padding={0}>
          {DEPS.map((d, i) => (
            <div
              key={d.name}
              style={{
                display: 'grid',
                gridTemplateColumns: '1fr 100px 140px',
                padding: '10px 14px',
                gap: 12,
                borderBottom:
                  i < DEPS.length - 1 ? '1px solid var(--mp-border)' : 'none',
                fontSize: 12.5,
              }}
            >
              <span style={{ fontWeight: 500 }}>{d.name}</span>
              <span
                className="mp-mono"
                style={{ color: 'var(--mp-textMuted)' }}
              >
                {d.v}
              </span>
              <span className="mp-mono" style={{ color: 'var(--mp-textDim)' }}>
                {d.license}
              </span>
            </div>
          ))}
        </MPCard>

        <div
          style={{
            marginTop: 24,
            fontSize: 11.5,
            color: 'var(--mp-textDim)',
            lineHeight: 1.7,
          }}
        >
          MaeclyPrint is free software licensed under{' '}
          <span className="mp-mono">GPLv3</span>. No telemetry, no cloud, no
          account. Source code at{' '}
          <span className="mp-mono">github.com/maecly/maeclyprint</span>.
        </div>
      </div>
    </>
  );
}
