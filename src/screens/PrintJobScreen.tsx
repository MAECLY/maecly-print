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
import { useEffect, useState, type Dispatch, type SetStateAction } from 'react';
import { toast } from 'sonner';
import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  findProfile,
  isValidCopies,
  isValidPageRange,
  type PrintJobOverrides,
} from '@/lib/profiles';
import { fmtKb, pluralize } from '@/lib/format';
import {
  getPrinterPrefs,
  isTauri,
  listPrinters,
  previewCommand,
} from '@/lib/ipc';
import { addPathsToJob } from '@/lib/jobFiles';
import type { Density } from '@/lib/theme';
import type { UiFile, UiProfile } from '@/types/domain';

// Re-exported for callers that still import the override shape from the screen
// (e.g. App). The canonical definition lives with the profile mapping.
export type { PrintJobOverrides };

/** Extensions the print engine accepts — mirrors the backend allowlist. */
const SUPPORTED = [
  'pdf',
  'jpg',
  'jpeg',
  'png',
  'tif',
  'tiff',
  'bmp',
  'webp',
] as const;

/** Turn an unknown thrown value into a user-facing message. */
function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

interface PrintJobScreenProps {
  files: UiFile[];
  setFiles: Dispatch<SetStateAction<UiFile[]>>;
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
  const profileObj = findProfile(profile);
  const merged = { ...profileObj, ...overrides };
  const tauri = isTauri();

  // Real installed-printer names for the override dropdown (empty outside
  // Tauri), excluding printers disabled in MaeclyPrint.
  const [printerNames, setPrinterNames] = useState<string[]>([]);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    Promise.all([
      listPrinters(),
      getPrinterPrefs().catch(() => ({ disabled: [] as string[] })),
    ])
      .then(([ps, prefs]) => {
        if (!alive) return;
        const disabled = new Set(prefs.disabled ?? []);
        setPrinterNames(
          ps.filter((p) => !disabled.has(p.id)).map((p) => p.name),
        );
      })
      .catch((e) => {
        // Non-fatal (the dropdown just stays on "Default printer"), but not
        // silent: a real enumeration failure is worth a trace in dev tools.
        console.error('printer list load failed', e);
      });
    return () => {
      alive = false;
    };
  }, []);
  const currentPrinter = overrides.printer ?? merged.printer;
  const printerOptions = Array.from(
    new Set([
      ...printerNames,
      ...(currentPrinter && currentPrinter !== '<default>'
        ? [currentPrinter]
        : []),
    ]),
  );

  const totalPages = files.reduce((s, f) => s + (f.pages ?? 1), 0);
  const totalKb = files.reduce((s, f) => s + f.kb, 0);

  /**
   * Inspect the given paths and append the printable ones. Shared with the
   * app-level drop handler; dedup happens inside the functional state update,
   * so concurrent adds can't lose files or duplicate them.
   */
  async function addPaths(paths: string[]): Promise<void> {
    await addPathsToJob(paths, setFiles);
  }

  /** Open the OS file picker (Tauri only) and add the chosen files. */
  async function onAddFiles(): Promise<void> {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({
        multiple: true,
        filters: [{ name: 'Printable files', extensions: [...SUPPORTED] }],
      });
      if (!picked) return;
      await addPaths(Array.isArray(picked) ? picked : [picked]);
    } catch (e) {
      toast.error(message(e));
    }
  }

  // Native OS drag & drop of files onto the window (FR-007). Registered once;
  // reads the latest files via filesRef for dedup.
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (event.payload.type === 'over') {
            setDragging(true);
          } else if (event.payload.type === 'drop') {
            setDragging(false);
            void addPaths(event.payload.paths);
          } else {
            setDragging(false);
          }
        }),
      )
      .then((un) => {
        if (cancelled) un();
        else unlisten = un;
      })
      .catch((e) => {
        console.error('drag-drop listener failed', e);
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // addPaths dedupes inside a functional update, so a stable listener is
    // intentional.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Copies is buffered locally so an out-of-range keystroke can show an error
  // instead of being silently clamped; valid values flow into the overrides.
  const [copiesText, setCopiesText] = useState(String(merged.copies));
  const copiesError = !isValidCopies(copiesText);
  const rangeError = !isValidPageRange(overrides.range ?? '');

  function onProfileChange(id: string): void {
    setProfile(id);
    setCopiesText(String(overrides.copies ?? findProfile(id).copies));
  }

  // Honesty preview: the exact command the backend would run, debounced so it
  // does not fire on every keystroke. Hidden outside Tauri, with no files, or
  // while a field is invalid; a failed preview shows nothing (it is advisory).
  const [preview, setPreview] = useState('');
  const canPreview = tauri && files.length > 0 && !copiesError && !rangeError;
  useEffect(() => {
    if (!canPreview) {
      setPreview('');
      return;
    }
    const req = buildPrintRequest(
      files.map((f) => f.path),
      profile,
      overrides,
    );
    // `stale` guards the in-flight promise: without it a slow response for
    // superseded settings could render over the current ones.
    let stale = false;
    const t = setTimeout(() => {
      previewCommand(req)
        .then((s) => {
          if (!stale) setPreview(s);
        })
        .catch((e) => {
          if (!stale) setPreview('');
          console.debug('preview_command failed', e);
        });
    }, 300);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [canPreview, files, profile, overrides]);

  const canPrint = files.length > 0 && !copiesError && !rangeError;

  const remove = (id: string) => setFiles(files.filter((f) => f.id !== id));
  const move = (id: string, dir: -1 | 1) => {
    const i = files.findIndex((f) => f.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= files.length) return;
    const next = files.slice();
    [next[i], next[j]] = [next[j]!, next[i]!];
    setFiles(next);
  };

  return (
    <>
      <MPTopBar
        title="Print Job"
        subtitle={`${files.length} ${pluralize(files.length, 'file')} · ${totalPages} pages · ${fmtKb(totalKb)}`}
        right={
          <>
            <MPButton
              variant="ghost"
              icon="plus"
              size="sm"
              onClick={onAddFiles}
              disabled={!tauri}
            >
              Add files
            </MPButton>
            <MPButton
              variant="primary"
              icon="printer"
              onClick={onPrint}
              disabled={!canPrint}
            >
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
            outline: dragging ? '2px dashed var(--mp-accent)' : 'none',
            outlineOffset: -4,
            background: dragging ? 'var(--mp-accentSoft)' : 'transparent',
            transition: 'background .12s',
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
                {tauri ? (
                  <>
                    No files. Drag from Explorer or Finder, or click{' '}
                    <strong>Add files</strong>.
                  </>
                ) : (
                  <>Adding files requires the desktop app.</>
                )}
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
            onChange={(e) => onProfileChange(e.target.value)}
            options={BUILT_IN_PROFILES.map((p) => ({
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
              ...printerOptions.map((n) => ({ value: n, label: n })),
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
            value={copiesText}
            onChange={(e) => {
              const raw = e.target.value;
              setCopiesText(raw);
              if (isValidCopies(raw)) {
                setOverrides({ ...overrides, copies: Number(raw.trim()) });
              }
            }}
            type="number"
            suffix="× per file"
            error={copiesError}
          />
          {copiesError && (
            <div
              style={{
                fontSize: 11,
                color: 'var(--mp-error)',
                marginTop: 4,
              }}
            >
              Enter a whole number from 1 to 999.
            </div>
          )}
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

          <MPSectionLabel>Page range</MPSectionLabel>
          <MPInput
            fullWidth
            mono
            placeholder="e.g. 1-5,10"
            value={overrides.range ?? ''}
            onChange={(e) =>
              setOverrides({ ...overrides, range: e.target.value })
            }
            error={rangeError}
          />
          <div
            style={{
              fontSize: 11,
              color: rangeError ? 'var(--mp-error)' : 'var(--mp-textDim)',
              marginTop: 4,
            }}
          >
            {rangeError
              ? 'Use page numbers like 1-5,10 (ranges must ascend).'
              : 'Applies to PDFs only — images ignore the range.'}
          </div>

          {tauri && files.length > 0 && preview && (
            <>
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
                  {preview}
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </>
  );
}
