import {
  MPButton,
  MPCard,
  MPIconButton,
  MPInput,
  MPKbd,
  MPSectionLabel,
  MPSegmented,
  MPSelect,
  MPTopBar,
  FileGlyph,
} from '@/components/ui';
import { MP_PRINTERS, MP_PROFILES } from '@/lib/mockData';
import { fmtKb, pluralize } from '@/lib/format';
import type { Density } from '@/lib/theme';
import type { UiFile, UiProfile } from '@/types/domain';

export interface PrintJobOverrides {
  printer?: string;
  duplex?: UiProfile['duplex'];
  color?: UiProfile['color'];
  copies?: number;
  paper?: string;
  orientation?: UiProfile['orientation'];
  scaling?: UiProfile['scaling'];
  range?: string;
}

interface PrintJobScreenProps {
  files: UiFile[];
  setFiles: (files: UiFile[]) => void;
  profile: string;
  setProfile: (id: string) => void;
  overrides: PrintJobOverrides;
  setOverrides: (o: PrintJobOverrides) => void;
  density?: Density;
  onPrint?: () => void;
}

export function PrintJobScreen({
  files,
  setFiles,
  profile,
  setProfile,
  overrides,
  setOverrides,
  density = 'comfortable',
  onPrint,
}: PrintJobScreenProps) {
  const profileObj =
    MP_PROFILES.find((p) => p.id === profile) ?? MP_PROFILES[0];
  const merged = { ...profileObj, ...overrides };

  const totalPages = files.reduce((s, f) => s + (f.pages ?? 1), 0);
  const totalKb = files.reduce((s, f) => s + f.kb, 0);

  const remove = (id: string) => setFiles(files.filter((f) => f.id !== id));
  const move = (id: string, dir: -1 | 1) => {
    const i = files.findIndex((f) => f.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= files.length) return;
    const next = files.slice();
    [next[i], next[j]] = [next[j], next[i]];
    setFiles(next);
  };

  return (
    <>
      <MPTopBar
        title="Print Job"
        subtitle={`${files.length} ${pluralize(files.length, 'file')} · ${totalPages} pages · ${fmtKb(totalKb)}`}
        right={
          <>
            <MPButton variant="ghost" icon="plus" size="sm">
              Add files
            </MPButton>
            <MPButton variant="primary" icon="printer" onClick={onPrint}>
              Print {files.length}
            </MPButton>
          </>
        }
      />
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        {/* File list */}
        <div
          className="mp-scroll"
          style={{
            flex: 1,
            overflow: 'auto',
            padding: '14px 16px',
            minWidth: 0,
          }}
        >
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 8,
            }}
          >
            <MPSectionLabel>Files in this job</MPSectionLabel>
            <div
              style={{
                display: 'flex',
                gap: 6,
                alignItems: 'center',
              }}
            >
              <MPKbd>↑↓</MPKbd>
              <span style={{ fontSize: 10.5, color: 'var(--mp-textDim)' }}>
                reorder
              </span>
            </div>
          </div>
          <MPCard padding={0}>
            {files.map((f, i) => (
              <div
                key={f.id}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: density === 'compact' ? '6px 12px' : '10px 12px',
                  borderBottom:
                    i < files.length - 1
                      ? '1px solid var(--mp-border)'
                      : 'none',
                }}
              >
                <span
                  className="mp-mono"
                  style={{
                    fontSize: 10.5,
                    color: 'var(--mp-textDim)',
                    width: 18,
                    textAlign: 'right',
                  }}
                >
                  {(i + 1).toString().padStart(2, '0')}
                </span>
                <FileGlyph ext={f.ext} size={density === 'compact' ? 18 : 24} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div
                    style={{
                      fontSize: 12.5,
                      fontWeight: 500,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {f.name}.{f.ext}
                  </div>
                  {density !== 'compact' && (
                    <div
                      className="mp-mono"
                      style={{
                        fontSize: 10.5,
                        color: 'var(--mp-textDim)',
                        marginTop: 1,
                      }}
                    >
                      {f.pages
                        ? `${f.pages} ${pluralize(f.pages, 'page')}`
                        : 'image'}{' '}
                      · {fmtKb(f.kb)}
                    </div>
                  )}
                </div>
                <MPIconButton
                  icon="chevu"
                  size={22}
                  label="Move up"
                  onClick={() => move(f.id, -1)}
                />
                <MPIconButton
                  icon="chevd"
                  size={22}
                  label="Move down"
                  onClick={() => move(f.id, 1)}
                />
                <MPIconButton
                  icon="x"
                  size={22}
                  label="Remove"
                  onClick={() => remove(f.id)}
                />
              </div>
            ))}
            {files.length === 0 && (
              <div
                style={{
                  padding: '30px 16px',
                  textAlign: 'center',
                  color: 'var(--mp-textMuted)',
                  fontSize: 12.5,
                }}
              >
                No files. Drag from Explorer or click <strong>Add files</strong>
                .
              </div>
            )}
          </MPCard>
        </div>

        {/* Settings panel */}
        <div
          className="mp-scroll"
          style={{
            width: 280,
            flexShrink: 0,
            overflow: 'auto',
            background: 'var(--mp-surface2)',
            borderLeft: '1px solid var(--mp-border)',
            padding: '14px 16px',
          }}
        >
          <MPSectionLabel>Profile</MPSectionLabel>
          <MPSelect
            fullWidth
            value={profile}
            onChange={(e) => setProfile(e.target.value)}
            options={MP_PROFILES.map((p) => ({
              value: p.id,
              label: p.name + (p.builtin ? ' · built-in' : ''),
            }))}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>Printer</MPSectionLabel>
          <MPSelect
            fullWidth
            value={overrides.printer ?? merged.printer}
            onChange={(e) =>
              setOverrides({ ...overrides, printer: e.target.value })
            }
            options={[
              { value: '<default>', label: 'System default' },
              ...MP_PRINTERS.map((p) => ({ value: p.name, label: p.name })),
            ]}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>Sides</MPSectionLabel>
          <MPSegmented
            value={merged.duplex}
            onChange={(v) => setOverrides({ ...overrides, duplex: v })}
            options={[
              { value: 'off', label: 'Single' },
              { value: 'long-edge', label: 'Long' },
              { value: 'short-edge', label: 'Short' },
            ]}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>Color</MPSectionLabel>
          <MPSegmented
            value={merged.color}
            onChange={(v) => setOverrides({ ...overrides, color: v })}
            options={[
              { value: 'color', label: 'Color' },
              { value: 'monochrome', label: 'B&W' },
            ]}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>Copies</MPSectionLabel>
          <MPInput
            fullWidth
            mono
            value={String(merged.copies)}
            onChange={(e) =>
              setOverrides({
                ...overrides,
                copies: parseInt(e.target.value) || 1,
              })
            }
            type="number"
            suffix="× per file"
          />
          <div style={{ height: 14 }} />

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: 10,
            }}
          >
            <div>
              <MPSectionLabel>Paper</MPSectionLabel>
              <MPSelect
                fullWidth
                value={merged.paper}
                onChange={(e) =>
                  setOverrides({ ...overrides, paper: e.target.value })
                }
                options={['A4', 'Letter', 'Legal', 'A3'].map((v) => ({
                  value: v,
                  label: v,
                }))}
              />
            </div>
            <div>
              <MPSectionLabel>Orient.</MPSectionLabel>
              <MPSelect
                fullWidth
                value={merged.orientation}
                onChange={(e) =>
                  setOverrides({
                    ...overrides,
                    orientation: e.target.value as UiProfile['orientation'],
                  })
                }
                options={[
                  { value: 'auto', label: 'Auto' },
                  { value: 'portrait', label: 'Portrait' },
                  { value: 'landscape', label: 'Landscape' },
                ]}
              />
            </div>
          </div>
          <div style={{ height: 14 }} />

          <MPSectionLabel>Scaling</MPSectionLabel>
          <MPSegmented
            value={merged.scaling}
            onChange={(v) => setOverrides({ ...overrides, scaling: v })}
            options={[
              { value: 'fit', label: 'Fit' },
              { value: 'shrink', label: 'Shrink' },
              { value: 'none', label: '100%' },
            ]}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>Page range (PDFs)</MPSectionLabel>
          <MPInput
            fullWidth
            mono
            placeholder="e.g. 1-5,10"
            value={overrides.range ?? ''}
            onChange={(e) =>
              setOverrides({ ...overrides, range: e.target.value })
            }
          />

          <div style={{ height: 18 }} />
          <div
            style={{
              padding: '10px 12px',
              background: 'var(--mp-surface)',
              border: '1px solid var(--mp-border)',
              borderRadius: 6,
            }}
          >
            <div
              className="mp-mono"
              style={{
                fontSize: 10.5,
                color: 'var(--mp-textDim)',
                marginBottom: 6,
                letterSpacing: '0.06em',
              }}
            >
              RESOLVED COMMAND
            </div>
            <div
              className="mp-mono"
              style={{
                fontSize: 10.5,
                color: 'var(--mp-textMuted)',
                lineHeight: 1.5,
                wordBreak: 'break-all',
              }}
            >
              SumatraPDF -print-to "
              {merged.printer === '<default>' ? '<default>' : merged.printer}"
              -print-settings "
              {merged.duplex === 'long-edge'
                ? 'duplex,'
                : merged.duplex === 'short-edge'
                  ? 'duplexshort,'
                  : ''}
              {merged.color === 'monochrome' ? 'monochrome,' : 'color,'}
              {merged.scaling},paper={merged.paper},copies={merged.copies}"
              -silent
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
