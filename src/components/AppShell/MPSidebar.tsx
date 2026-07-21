import { useEffect, useState } from 'react';
import { Icon, type IconName } from '@/components/Icon';
import { MPWordmark } from '@/components/Brand';
import { MPBadge, MPKbd } from '@/components/ui';
import { fetchAppInfo } from '@/lib/ipc';
import { UNKNOWN_VERSION } from '@/lib/repo';

export type ScreenId =
  | 'home'
  | 'job'
  | 'queue'
  | 'printers'
  | 'profiles'
  | 'settings'
  | 'about';

interface NavItem {
  id: ScreenId;
  icon: IconName;
  label: string;
  count?: number;
  kbd?: string;
}

interface MPSidebarProps {
  active: ScreenId;
  onChange?: (id: ScreenId) => void;
  queueCount?: number;
}

/**
 * What the footer knows about the Rust host: still asking, answered (with the
 * authoritative running version), or no host at all — the last being the normal
 * state in the standalone Vite preview, not an error worth surfacing.
 */
type HostState =
  | { kind: 'connecting' }
  | { kind: 'connected'; version: string }
  | { kind: 'no-host' };

/** Ask `app_info` once on mount for the running version and host liveness. */
function useHostState(): HostState {
  const [state, setState] = useState<HostState>({ kind: 'connecting' });
  useEffect(() => {
    let alive = true;
    fetchAppInfo()
      .then((info) => {
        if (alive) setState({ kind: 'connected', version: info.version });
      })
      .catch(() => {
        if (alive) setState({ kind: 'no-host' });
      });
    return () => {
      alive = false;
    };
  }, []);
  return state;
}

export function MPSidebar({
  active,
  onChange,
  queueCount = 4,
}: MPSidebarProps) {
  const host = useHostState();
  const items: NavItem[] = [
    { id: 'home', icon: 'home', label: 'Home' },
    { id: 'job', icon: 'file', label: 'Print Job', kbd: 'Ctrl+P' },
    { id: 'queue', icon: 'queue', label: 'Queue', count: queueCount },
    { id: 'printers', icon: 'printer', label: 'Printers' },
    { id: 'profiles', icon: 'sliders', label: 'Profiles' },
    { id: 'settings', icon: 'settings', label: 'Settings' },
    { id: 'about', icon: 'info', label: 'About' },
  ];
  return (
    <div
      style={{
        width: 200,
        flexShrink: 0,
        background: 'var(--mp-surface2)',
        borderRight: '1px solid var(--mp-border)',
        display: 'flex',
        flexDirection: 'column',
        padding: '10px 8px',
      }}
    >
      <div style={{ padding: '6px 8px 12px' }}>
        <MPWordmark size={13} />
      </div>
      <nav style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
        {items.map((it) => {
          const a = it.id === active;
          return (
            <button
              key={it.id}
              onClick={() => onChange?.(it.id)}
              className="mp-focus"
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 9,
                padding: '6px 9px',
                height: 30,
                background: a ? 'var(--mp-surface)' : 'transparent',
                color: a ? 'var(--mp-text)' : 'var(--mp-textMuted)',
                border: 'none',
                borderRadius: 6,
                cursor: 'pointer',
                fontSize: 13,
                fontWeight: a ? 500 : 400,
                textAlign: 'left',
                boxShadow: a
                  ? '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px var(--mp-border)'
                  : 'none',
                transition: 'background .12s, color .12s',
              }}
            >
              <Icon name={it.icon} size={15} />
              <span style={{ flex: 1 }}>{it.label}</span>
              {it.count != null && <MPBadge tone="neutral">{it.count}</MPBadge>}
              {it.kbd && <MPKbd>{it.kbd}</MPKbd>}
            </button>
          );
        })}
      </nav>
      <div style={{ flex: 1 }} />
      <div
        style={{
          padding: '8px',
          borderTop: '1px solid var(--mp-border)',
          marginTop: 8,
        }}
      >
        <div
          className="mp-mono"
          style={{
            fontSize: 10.5,
            color: 'var(--mp-textDim)',
            display: 'flex',
            justifyContent: 'space-between',
          }}
        >
          <span>
            {host.kind === 'connected' ? `v${host.version}` : UNKNOWN_VERSION}
          </span>
          <span
            style={{
              color:
                host.kind === 'connected'
                  ? 'var(--mp-success)'
                  : 'var(--mp-textDim)',
            }}
          >
            {host.kind === 'connected'
              ? '● connected'
              : host.kind === 'connecting'
                ? '○ connecting…'
                : '○ no backend'}
          </span>
        </div>
      </div>
    </div>
  );
}
