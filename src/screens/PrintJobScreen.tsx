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
import {
  useEffect,
  useState,
  useSyncExternalStore,
  type Dispatch,
  type KeyboardEvent,
  type SetStateAction,
} from 'react';
import { toast } from 'sonner';
import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  DEFAULT_PRINTER,
  findProfileIn,
  findPageRangeOverflow,
  isValidCopies,
  isValidPageRange,
  type PrintJobOverrides,
} from '@/lib/profiles';
import { fmtKb } from '@/lib/format';
import {
  getPrinterPrefs,
  isTauri,
  listPrinters,
  previewCommand,
} from '@/lib/ipc';
import { useT } from '@/i18n/I18nProvider';
import { useProfilesStore } from '@/stores/profilesStore';
import {
  addPathsToJob,
  expandFolderIntoJob,
  getPendingFolders,
  resolvePendingFolder,
  subscribePendingFolders,
} from '@/lib/jobFiles';
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
  const t = useT('printJob');

  // Profiles come from the shared store (built-ins + customs). Init is
  // idempotent; the static built-ins are the fallback until it resolves (and
  // the store's own fallback outside Tauri), so the dropdown is never empty.
  const storeProfiles = useProfilesStore((s) => s.profiles);
  const initProfiles = useProfilesStore((s) => s.init);
  useEffect(() => {
    void initProfiles();
  }, [initProfiles]);
  const profileList =
    storeProfiles.length > 0 ? storeProfiles : BUILT_IN_PROFILES;

  const profileObj = findProfileIn(profileList, profile);
  const merged = { ...profileObj, ...overrides };
  const tauri = isTauri();

  // Real installed printers for the override dropdown (empty outside Tauri),
  // excluding printers disabled in MaeclyPrint. The *id* is the queue name the
  // spooler needs (`lp -d`, SumatraPDF `-print-to`); the display name only
  // labels it, and on CUPS the two genuinely differ (`printer-info` vs the
  // queue name). Sending the label would produce a job for a printer that does
  // not exist.
  const [printers, setPrinters] = useState<{ id: string; name: string }[]>([]);
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
        setPrinters(
          ps
            .filter((p) => !disabled.has(p.id))
            .map((p) => ({ id: p.id, name: p.name })),
        );
      })
      .catch((e) => {
        // Non-fatal (the dropdown just stays on "System default"), but not
        // silent: a real enumeration failure is worth a trace in dev tools.
        console.error('printer list load failed', e);
      });
    return () => {
      alive = false;
    };
  }, []);
  const currentPrinter = merged.printer;
  // A profile saved before this screen used ids — or one naming a printer that
  // is currently offline — would otherwise silently snap to "System default".
  // Keep the stored value selectable, labelled with itself.
  const unknownPrinter =
    currentPrinter &&
    currentPrinter !== DEFAULT_PRINTER &&
    !printers.some((p) => p.id === currentPrinter)
      ? currentPrinter
      : null;
  const printerOptions = [
    { value: DEFAULT_PRINTER, label: t('settings.systemDefault') },
    ...printers.map((p) => ({ value: p.id, label: p.name })),
    ...(unknownPrinter
      ? [{ value: unknownPrinter, label: unknownPrinter }]
      : []),
  ];

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

  /**
   * Open the OS file picker (Tauri only) and add the chosen files.
   *
   * Two pickers, because the OS dialogs are mutually exclusive: files with the
   * printable-extension filter, or a directory (FR-007). The folder path lands
   * in the same `addPaths` pipeline, so it gets the same FR-004 question a drop
   * would.
   */
  async function onPick(directory: boolean): Promise<void> {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = directory
        ? await open({ multiple: true, directory: true })
        : await open({
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
  // addPaths dedupes inside a functional update, so a stable listener is safe.
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Folders waiting on the FR-004 recurse-or-not question. The store lives in
  // `jobFiles` so a folder dropped from any screen (the app-level listener then
  // navigates here) surfaces the same prompt.
  const pendingFolders = useSyncExternalStore(
    subscribePendingFolders,
    getPendingFolders,
  );

  // Copies is buffered locally so an out-of-range keystroke can show an error
  // instead of being silently clamped; valid values flow into the overrides.
  const [copiesText, setCopiesText] = useState(String(merged.copies));
  const copiesError = !isValidCopies(copiesText);

  // The page range is DERIVED, never buffered: what the field shows has to be
  // exactly what `toJobSettings` sends (FR-041 wants the profile's value
  // pre-filled). `undefined` inherits the profile's saved range; `''` is an
  // explicit override back to "all pages" — the only way to defeat a profile
  // range, so clearing the field must keep meaning that.
  const rangeText = overrides.range ?? profileObj.pageRange ?? '';
  const rangeSyntaxError = !isValidPageRange(rangeText);
  // FR-042/FR-043: bounds are checked inline against the real page counts, so
  // "500-600" on an 8-page report fails here instead of spooling an empty job.
  const rangeOverflow = rangeSyntaxError
    ? null
    : findPageRangeOverflow(
        rangeText,
        files.map((f) => ({
          name: `${f.name}.${f.ext}`,
          ext: f.ext,
          pages: f.pages,
        })),
      );
  const rangeError = rangeSyntaxError || rangeOverflow !== null;

  function onProfileChange(id: string): void {
    setProfile(id);
    setCopiesText(
      String(overrides.copies ?? findProfileIn(profileList, id).copies),
    );
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
      profileObj,
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
        .catch(() => {
          if (!stale) setPreview('');
        });
    }, 300);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [canPreview, files, profileObj, overrides]);

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

  /**
   * Keyboard reordering for a focused row. Alt is the modifier because the bare
   * arrows belong to the scroll container, and React keeps the same DOM node
   * for a moved key — so focus follows the row without being re-applied.
   */
  const onRowKeyDown = (e: KeyboardEvent<HTMLDivElement>, id: string) => {
    if (!e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return;
    e.preventDefault();
    move(id, e.key === 'ArrowUp' ? -1 : 1);
  };

  const summary = t('subtitle', {
    files: t('units.file', { count: files.length }),
    pages: t('units.page', { count: totalPages }),
    size: fmtKb(totalKb),
  });

  return (
    <>
      <MPTopBar
        title={t('title')}
        subtitle={summary}
        right={
          <>
            <MPButton
              variant="ghost"
              icon="plus"
              size="sm"
              onClick={() => void onPick(false)}
              disabled={!tauri}
            >
              {t('actions.addFiles')}
            </MPButton>
            <MPButton
              variant="ghost"
              icon="folder"
              size="sm"
              onClick={() => void onPick(true)}
              disabled={!tauri}
            >
              {t('actions.addFolder')}
            </MPButton>
            <MPButton
              variant="primary"
              icon="printer"
              onClick={onPrint}
              disabled={!canPrint}
            >
              {t('actions.print', { count: files.length })}
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
          {pendingFolders.length > 0 && (
            <section
              aria-label={t('folders.title', {
                count: pendingFolders.length,
              })}
              style={{
                border: '1px solid var(--mp-accent)',
                background: 'var(--mp-accentSoft)',
                borderRadius: 8,
                padding: '12px 14px',
                marginBottom: 12,
              }}
            >
              <MPSectionLabel>
                {t('folders.title', { count: pendingFolders.length })}
              </MPSectionLabel>
              {pendingFolders.map((folder) => (
                <div key={folder.path} style={{ marginTop: 8 }}>
                  <div style={{ fontSize: 12.5, marginBottom: 8 }}>
                    {t('folders.question', { name: folder.name })}
                  </div>
                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                    <MPButton
                      variant="primary"
                      size="sm"
                      onClick={() =>
                        void expandFolderIntoJob(folder, 'recursive', setFiles)
                      }
                    >
                      {t('folders.recursive')}
                    </MPButton>
                    <MPButton
                      size="sm"
                      onClick={() =>
                        void expandFolderIntoJob(folder, 'top_level', setFiles)
                      }
                    >
                      {t('folders.topLevel')}
                    </MPButton>
                    <MPButton
                      variant="ghost"
                      size="sm"
                      onClick={() => resolvePendingFolder(folder.path)}
                    >
                      {t('folders.skip')}
                    </MPButton>
                  </div>
                </div>
              ))}
            </section>
          )}
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 8,
            }}
          >
            <MPSectionLabel>{t('list.heading')}</MPSectionLabel>
            <div
              style={{
                display: 'flex',
                gap: 6,
                alignItems: 'center',
              }}
            >
              <MPKbd>{t('list.reorderKeys')}</MPKbd>
              <span style={{ fontSize: 10.5, color: 'var(--mp-textDim)' }}>
                {t('list.reorderHint')}
              </span>
            </div>
          </div>
          <MPCard padding={0}>
            <div role="list" aria-label={t('list.label')}>
              {files.map((f, i) => (
                // A focusable list item carrying its own arrow-key handler is
                // the standard reorderable-list pattern: the row is the thing
                // being moved, so it is the thing that must hold focus. The
                // rule only knows that `listitem` is not natively interactive.
                /* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */
                <div
                  key={f.id}
                  role="listitem"
                  // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
                  tabIndex={0}
                  aria-label={t('list.row', {
                    position: i + 1,
                    total: files.length,
                    name: `${f.name}.${f.ext}`,
                  })}
                  onKeyDown={(e) => onRowKeyDown(e, f.id)}
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
                  <FileGlyph
                    ext={f.ext}
                    size={density === 'compact' ? 18 : 24}
                  />
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
                          ? t('units.page', { count: f.pages })
                          : t('list.image')}{' '}
                        · {fmtKb(f.kb)}
                      </div>
                    )}
                  </div>
                  <MPIconButton
                    icon="chevu"
                    size={22}
                    label={t('list.moveUp', { name: `${f.name}.${f.ext}` })}
                    onClick={() => move(f.id, -1)}
                  />
                  <MPIconButton
                    icon="chevd"
                    size={22}
                    label={t('list.moveDown', { name: `${f.name}.${f.ext}` })}
                    onClick={() => move(f.id, 1)}
                  />
                  <MPIconButton
                    icon="x"
                    size={22}
                    label={t('list.remove', { name: `${f.name}.${f.ext}` })}
                    onClick={() => remove(f.id)}
                  />
                </div>
              ))}
            </div>
            {files.length === 0 && (
              <div
                style={{
                  padding: '30px 16px',
                  textAlign: 'center',
                  color: 'var(--mp-textMuted)',
                  fontSize: 12.5,
                }}
              >
                {tauri ? t('empty.desktop') : t('empty.web')}
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
          <MPSectionLabel>{t('settings.profile')}</MPSectionLabel>
          <MPSelect
            fullWidth
            aria-label={t('settings.profile')}
            value={profile}
            onChange={(e) => onProfileChange(e.target.value)}
            options={profileList.map((p) => ({
              value: p.id,
              label: p.name + (p.builtin ? t('settings.builtinSuffix') : ''),
            }))}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>{t('settings.printer')}</MPSectionLabel>
          <MPSelect
            fullWidth
            aria-label={t('settings.printer')}
            value={currentPrinter}
            onChange={(e) =>
              setOverrides({ ...overrides, printer: e.target.value })
            }
            options={printerOptions}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>{t('settings.sides')}</MPSectionLabel>
          <MPSegmented
            value={merged.duplex}
            onChange={(v) => setOverrides({ ...overrides, duplex: v })}
            options={[
              { value: 'off', label: t('sides.single') },
              { value: 'long-edge', label: t('sides.long') },
              { value: 'short-edge', label: t('sides.short') },
            ]}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>{t('settings.color')}</MPSectionLabel>
          <MPSegmented
            value={merged.color}
            onChange={(v) => setOverrides({ ...overrides, color: v })}
            options={[
              { value: 'color', label: t('color.color') },
              { value: 'monochrome', label: t('color.monochrome') },
            ]}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>{t('settings.copies')}</MPSectionLabel>
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
            suffix={t('settings.copiesSuffix')}
            error={copiesError}
          />
          {copiesError && (
            <div
              role="alert"
              style={{
                fontSize: 11,
                color: 'var(--mp-error)',
                marginTop: 4,
              }}
            >
              {t('copies.error')}
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
              <MPSectionLabel>{t('settings.paper')}</MPSectionLabel>
              <MPSelect
                fullWidth
                aria-label={t('settings.paper')}
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
              <MPSectionLabel>{t('settings.orientation')}</MPSectionLabel>
              <MPSelect
                fullWidth
                aria-label={t('settings.orientationLabel')}
                value={merged.orientation}
                onChange={(e) =>
                  setOverrides({
                    ...overrides,
                    orientation: e.target.value as UiProfile['orientation'],
                  })
                }
                options={[
                  { value: 'auto', label: t('orientation.auto') },
                  { value: 'portrait', label: t('orientation.portrait') },
                  { value: 'landscape', label: t('orientation.landscape') },
                ]}
              />
            </div>
          </div>
          <div style={{ height: 14 }} />

          <MPSectionLabel>{t('settings.scaling')}</MPSectionLabel>
          <MPSegmented
            value={merged.scaling}
            onChange={(v) => setOverrides({ ...overrides, scaling: v })}
            options={[
              { value: 'fit', label: t('scaling.fit') },
              { value: 'shrink', label: t('scaling.shrink') },
              { value: 'none', label: t('scaling.none') },
            ]}
          />
          <div style={{ height: 14 }} />

          <MPSectionLabel>{t('settings.pageRange')}</MPSectionLabel>
          <MPInput
            fullWidth
            mono
            placeholder={t('pageRange.placeholder')}
            value={rangeText}
            onChange={(e) =>
              setOverrides({ ...overrides, range: e.target.value })
            }
            error={rangeError}
          />
          <div
            {...(rangeError ? { role: 'alert' as const } : {})}
            style={{
              fontSize: 11,
              color: rangeError ? 'var(--mp-error)' : 'var(--mp-textDim)',
              marginTop: 4,
            }}
          >
            {rangeSyntaxError
              ? t('pageRange.syntaxError')
              : rangeOverflow
                ? t('pageRange.outOfBounds', {
                    name: rangeOverflow.name,
                    pages: rangeOverflow.pages,
                  })
                : t('pageRange.hint')}
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
                    textTransform: 'uppercase',
                  }}
                >
                  {t('preview.title')}
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
