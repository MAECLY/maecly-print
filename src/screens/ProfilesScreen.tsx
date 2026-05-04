import { Icon } from '@/components/Icon';
import {
  MPBadge,
  MPButton,
  MPCard,
  MPSectionLabel,
  MPTopBar,
} from '@/components/ui';
import { MP_PROFILES } from '@/lib/mockData';

interface ProfilesScreenProps {
  selectedId: string;
  onSelect?: (id: string) => void;
}

export function ProfilesScreen({ selectedId, onSelect }: ProfilesScreenProps) {
  const sel = MP_PROFILES.find((p) => p.id === selectedId) ?? MP_PROFILES[0];

  return (
    <>
      <MPTopBar
        title="Profiles"
        subtitle="Saved combinations of printer + settings"
        right={
          <MPButton variant="primary" icon="plus">
            New profile
          </MPButton>
        }
      />
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        <div
          className="mp-scroll"
          style={{
            width: 280,
            flexShrink: 0,
            overflow: 'auto',
            borderRight: '1px solid var(--mp-border)',
            padding: '10px 8px',
            background: 'var(--mp-surface2)',
          }}
        >
          {MP_PROFILES.map((p) => {
            const a = p.id === sel.id;
            return (
              <button
                key={p.id}
                onClick={() => onSelect?.(p.id)}
                className="mp-focus"
                style={{
                  width: '100%',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: '9px 10px',
                  border: 'none',
                  background: a ? 'var(--mp-surface)' : 'transparent',
                  borderRadius: 6,
                  cursor: 'pointer',
                  textAlign: 'left',
                  marginBottom: 2,
                  boxShadow: a
                    ? '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px var(--mp-border)'
                    : 'none',
                }}
              >
                <div
                  style={{
                    width: 28,
                    height: 28,
                    borderRadius: 6,
                    background: p.builtin
                      ? 'var(--mp-surface2)'
                      : 'var(--mp-accentSoft)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    flexShrink: 0,
                  }}
                >
                  <Icon
                    name={p.builtin ? 'lock' : 'sliders'}
                    size={14}
                    color={
                      p.builtin ? 'var(--mp-textMuted)' : 'var(--mp-accent)'
                    }
                  />
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 6,
                    }}
                  >
                    <span style={{ fontSize: 12.5, fontWeight: 500 }}>
                      {p.name}
                    </span>
                    {p.default && <MPBadge tone="accent">default</MPBadge>}
                  </div>
                  <div
                    className="mp-mono"
                    style={{
                      fontSize: 10.5,
                      color: 'var(--mp-textDim)',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {p.color} · {p.duplex === 'off' ? 'simplex' : p.duplex} ·{' '}
                    {p.paper}
                  </div>
                </div>
              </button>
            );
          })}
        </div>
        <div
          className="mp-scroll"
          style={{ flex: 1, overflow: 'auto', padding: '18px 22px' }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              marginBottom: 18,
            }}
          >
            <div>
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                }}
              >
                <h2
                  style={{
                    margin: 0,
                    fontSize: 18,
                    fontWeight: 600,
                    letterSpacing: '-0.01em',
                  }}
                >
                  {sel.name}
                </h2>
                {sel.builtin && (
                  <MPBadge tone="neutral" icon="lock">
                    built-in
                  </MPBadge>
                )}
                {sel.default && <MPBadge tone="accent">default</MPBadge>}
              </div>
              <div
                className="mp-mono"
                style={{
                  fontSize: 11,
                  color: 'var(--mp-textDim)',
                  marginTop: 4,
                }}
              >
                id: {sel.id} · schema_version: 1
              </div>
            </div>
            <div style={{ display: 'flex', gap: 6 }}>
              <MPButton variant="ghost" icon="copy" size="sm">
                Duplicate
              </MPButton>
              <MPButton
                variant="ghost"
                icon="star"
                size="sm"
                disabled={sel.default}
              >
                Make default
              </MPButton>
              {!sel.builtin && (
                <MPButton variant="danger" icon="trash" size="sm">
                  Delete
                </MPButton>
              )}
            </div>
          </div>

          {(
            [
              { label: 'Printer', v: sel.printer, mono: true },
              { label: 'Color mode', v: sel.color },
              {
                label: 'Duplex',
                v: sel.duplex === 'off' ? 'Single-sided' : sel.duplex,
              },
              { label: 'Copies', v: sel.copies, mono: true },
              { label: 'Paper', v: sel.paper, mono: true },
              { label: 'Orientation', v: sel.orientation },
              { label: 'Scaling', v: sel.scaling },
              { label: 'Quality', v: sel.quality },
              { label: 'Tray', v: sel.tray },
            ] as const
          ).map((row) => (
            <div
              key={row.label}
              style={{
                display: 'grid',
                gridTemplateColumns: '140px 1fr',
                gap: 14,
                padding: '10px 0',
                borderBottom: '1px solid var(--mp-border)',
              }}
            >
              <span style={{ fontSize: 12, color: 'var(--mp-textMuted)' }}>
                {row.label}
              </span>
              <span
                className={'mono' in row && row.mono ? 'mp-mono' : ''}
                style={{ fontSize: 13, color: 'var(--mp-text)' }}
              >
                {row.v}
              </span>
            </div>
          ))}

          <div style={{ marginTop: 20 }}>
            <MPSectionLabel>profiles.toml entry</MPSectionLabel>
            <MPCard padding={0} style={{ background: 'var(--mp-surface2)' }}>
              <pre
                className="mp-mono"
                style={{
                  margin: 0,
                  padding: '12px 14px',
                  fontSize: 11,
                  lineHeight: 1.6,
                  color: 'var(--mp-textMuted)',
                  overflow: 'auto',
                }}
              >{`[[profile]]
id          = "${sel.id}"
name        = "${sel.name}"
printer     = "${sel.printer}"
color_mode  = "${sel.color}"
duplex      = "${sel.duplex}"
copies      = ${sel.copies}
paper_size  = "${sel.paper}"
orientation = "${sel.orientation}"
scaling     = "${sel.scaling}"
quality     = "${sel.quality}"
tray        = "${sel.tray}"`}</pre>
            </MPCard>
          </div>
        </div>
      </div>
    </>
  );
}
