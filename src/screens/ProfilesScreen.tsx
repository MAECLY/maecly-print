import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';

import { Icon } from '@/components/Icon';
import { CenteredNotice } from '@/components/CenteredNotice';
import { ConfirmDialog, type ConfirmState } from '@/components/ConfirmDialog';
import { MPBadge, MPButton, MPTopBar } from '@/components/ui';
import { useT, type TFunction } from '@/i18n/I18nProvider';
import { ProfileEditor } from '@/screens/profiles/ProfileEditor';
import {
  BUILT_IN_PROFILES,
  findProfileIn,
  nextUniqueName,
} from '@/lib/profiles';
import { isTauri, listPrinters } from '@/lib/ipc';
import type { PrinterChoice } from '@/lib/printerIdentity';
import { useProfilesStore } from '@/stores/profilesStore';
import type { UiProfile } from '@/types/domain';

type T = TFunction<'profiles'>;

interface ProfilesScreenProps {
  selectedId: string;
  onSelect?: (id: string) => void;
}

/** Build a fresh unsaved draft (id '') from a base profile and a name. */
function draftFrom(base: UiProfile, name: string): UiProfile {
  const { pageRange, ...rest } = base;
  const d: UiProfile = {
    ...rest,
    id: '',
    builtin: false,
    default: false,
    name,
  };
  if (pageRange) d.pageRange = pageRange;
  return d;
}

/**
 * A filesystem-safe suggestion for the export dialog, following the naming the
 * v1 spec fixes: `<name>.maecly-profile.toml`. Anything a path separator or a
 * Windows-reserved character could trip over collapses to a hyphen.
 */
export function exportFileName(name: string): string {
  const reserved = '<>:"/\\|?*';
  const safe = Array.from(name.trim())
    .map((ch) =>
      (ch.codePointAt(0) ?? 0) < 0x20 || reserved.includes(ch) ? '-' : ch,
    )
    .join('')
    .replace(/-{2,}/g, '-')
    .replace(/^[.\-\s]+/, '')
    .slice(0, 60)
    .trim();
  return `${safe === '' ? 'profile' : safe}.maecly-profile.toml`;
}

export function ProfilesScreen({ selectedId, onSelect }: ProfilesScreenProps) {
  const t = useT('profiles');
  const profiles = useProfilesStore((s) => s.profiles);
  const status = useProfilesStore((s) => s.status);
  const error = useProfilesStore((s) => s.error);
  const init = useProfilesStore((s) => s.init);
  const save = useProfilesStore((s) => s.save);
  const remove = useProfilesStore((s) => s.remove);
  const setDefault = useProfilesStore((s) => s.setDefault);
  const defaultProfile = useProfilesStore((s) => s.defaultProfile);
  const refreshProfile = useProfilesStore((s) => s.refreshProfile);
  const exportProfile = useProfilesStore((s) => s.exportProfile);
  const importProfiles = useProfilesStore((s) => s.importProfiles);

  // The editor's unsaved state lives in the store, not here: the whole screen
  // is unmounted the moment the user clicks another sidebar entry, and local
  // state would take the draft with it (silently, mid-edit).
  const draft = useProfilesStore((s) => s.draft);
  const working = useProfilesStore((s) => s.working);
  const setDraft = useProfilesStore((s) => s.setDraft);
  const setWorking = useProfilesStore((s) => s.setWorking);
  const clearEditor = useProfilesStore((s) => s.clearEditor);

  useEffect(() => {
    void init();
  }, [init]);

  // Installed printers (queue id + display name) for the editor's dropdown (best-effort;
  // empty outside Tauri or on failure — the dropdown still offers "default").
  const [printerChoices, setPrinterChoices] = useState<PrinterChoice[]>([]);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    listPrinters()
      .then((ps) => {
        if (alive)
          setPrinterChoices(ps.map((p) => ({ id: p.id, name: p.name })));
      })
      .catch(() => {
        /* non-fatal: the dropdown just stays on "System default". */
      });
    return () => {
      alive = false;
    };
  }, []);

  // Nonces that force the editor to remount (and so reset its working copy)
  // when the draft identity or the underlying profile changes out from under it.
  const [draftNonce, setDraftNonce] = useState(0);
  const [editorNonce, setEditorNonce] = useState(0);
  const [confirmState, setConfirmState] = useState<ConfirmState | null>(null);
  // Mirrors the open editor's dirty flag (reported via ProfileEditor's
  // onDirtyChange). Used to guard actions that would discard unsaved edits.
  const [editorDirty, setEditorDirty] = useState(false);

  const readOnly = status === 'unavailable';
  const loading = status === 'idle' || status === 'loading';
  const ready = status === 'ready';

  // Re-read the profile being edited straight from disk when the selection
  // lands on a saved custom: `profiles.toml` can change under us (a second
  // window, a hand edit) and an editor seeded from a stale row would write the
  // stale values back. Cheap, and the only consumer of `get_profile`.
  const selectedIsCustom =
    ready && profiles.some((p) => p.id === selectedId && !p.builtin);
  useEffect(() => {
    if (!selectedIsCustom) return;
    void refreshProfile(selectedId);
  }, [selectedIsCustom, selectedId, refreshProfile]);

  // Run `action`, but if the editor has unsaved edits, confirm the discard
  // first. The editor's own Cancel button stays an explicit (undialoged)
  // discard; this only guards implicit loss from switching/New/Clone/Import.
  const guardDirty = (action: () => void) => {
    if (editorDirty) {
      setConfirmState({
        title: t('confirm.discardTitle'),
        message: t('confirm.discardMessage'),
        confirmLabel: t('confirm.discardConfirm'),
        run: action,
      });
    } else {
      action();
    }
  };

  const startDraft = () => {
    const base = findProfileIn(BUILT_IN_PROFILES, 'builtin-quick');
    setWorking(null);
    setDraft(
      draftFrom(base, nextUniqueName(t('actions.newProfile'), profiles)),
    );
    setDraftNonce((n) => n + 1);
  };

  /** Pick a destination and write `p` there as a shareable TOML file (FR-024). */
  const doExport = async (p: UiProfile): Promise<void> => {
    const { save: saveDialog } = await import('@tauri-apps/plugin-dialog');
    const path = await saveDialog({
      title: t('dialog.exportTitle'),
      defaultPath: exportFileName(p.name),
      filters: [{ name: t('dialog.fileType'), extensions: ['toml'] }],
    });
    if (!path) return;
    if (await exportProfile(p.id, path)) {
      toast.success(t('toast.exported', { name: p.name }));
    }
  };

  /** Pick a TOML document and add whatever valid profiles it holds (FR-024). */
  const doImport = async (): Promise<void> => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({
      title: t('dialog.importTitle'),
      multiple: false,
      directory: false,
      filters: [{ name: t('dialog.fileType'), extensions: ['toml'] }],
    });
    if (typeof picked !== 'string') return;
    const imported = await importProfiles(picked);
    if (!imported) return;
    clearEditor();
    setEditorNonce((n) => n + 1);
    toast.success(t('toast.imported', { count: imported.length }));
    const first = imported[0];
    if (first) onSelect?.(first.id);
  };

  const topBar = (
    <MPTopBar
      title={t('title')}
      subtitle={t('subtitle')}
      right={
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <span style={{ display: 'inline-flex' }} title={t('tooltip.import')}>
            <MPButton
              variant="ghost"
              icon="download"
              size="sm"
              disabled={!ready}
              onClick={() => guardDirty(() => void doImport())}
            >
              {t('actions.import')}
            </MPButton>
          </span>
          <MPButton
            variant="primary"
            icon="plus"
            disabled={!ready}
            onClick={() => guardDirty(startDraft)}
          >
            {t('actions.newProfile')}
          </MPButton>
        </div>
      }
    />
  );

  if (loading || status === 'error') {
    return (
      <>
        {topBar}
        <CenteredNotice
          loading={loading}
          error={error ?? null}
          icon="sliders"
          loadingTitle={t('notice.loadingTitle')}
          errorTitle={t('notice.errorTitle')}
        />
      </>
    );
  }

  const showDraft = draft !== null;
  const sel = showDraft ? draft : findProfileIn(profiles, selectedId);
  const editing = showDraft || !sel.builtin;
  // Identity of the open editor, used both to key the React element and to tag
  // the working copy so it is never restored into a different form.
  const workingKey = showDraft ? 'draft' : `profile:${sel.id}`;
  const restore = working?.key === workingKey ? working : undefined;

  const select = (id: string) => {
    clearEditor();
    onSelect?.(id);
  };

  const startClone = (p: UiProfile) => {
    setWorking(null);
    setDraft(
      draftFrom(
        p,
        nextUniqueName(t('detail.copyOf', { name: p.name }), profiles),
      ),
    );
    setDraftNonce((n) => n + 1);
  };

  const handleSave = async (p: UiProfile): Promise<UiProfile | null> => {
    const saved = await save(p);
    if (saved) {
      toast.success(t('toast.saved', { name: saved.name }));
      clearEditor();
      setEditorNonce((n) => n + 1);
      onSelect?.(saved.id);
    }
    return saved;
  };

  const handleCancel = () => {
    clearEditor();
    // Bump so an in-place edit of a saved custom resets to the stored values.
    setEditorNonce((n) => n + 1);
  };

  const doDelete = async (p: UiProfile) => {
    const removed = await remove(p.id);
    if (!removed) return;
    clearEditor();
    if (selectedId === removed.id) onSelect?.(defaultProfile().id);
    toast(t('toast.deleted'), {
      duration: 5000,
      action: {
        label: t('toast.undo'),
        onClick: () => void save(removed),
      },
    });
  };

  const askDelete = (p: UiProfile) =>
    setConfirmState({
      title: t('confirm.deleteTitle'),
      message: t('confirm.deleteMessage', { name: p.name }),
      confirmLabel: t('confirm.deleteConfirm'),
      run: () => doDelete(p),
    });

  const detailRows = (
    [
      { label: t('field.printer'), v: sel.printer, mono: true },
      { label: t('field.colorMode'), v: colorLabel(t, sel.color) },
      { label: t('field.duplex'), v: duplexLabel(t, sel.duplex) },
      { label: t('field.copies'), v: sel.copies, mono: true },
      { label: t('field.paper'), v: sel.paper, mono: true },
      {
        label: t('field.orientation'),
        v: orientationLabel(t, sel.orientation),
      },
      { label: t('field.scaling'), v: scalingLabel(t, sel.scaling) },
      { label: t('field.quality'), v: qualityLabel(t, sel.quality) },
      { label: t('field.tray'), v: sel.tray },
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
  ));

  return (
    <>
      {topBar}
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        {/* Master list */}
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
          {profiles.map((p) => {
            const a = !showDraft && p.id === sel.id;
            return (
              <button
                key={p.id}
                onClick={() => guardDirty(() => select(p.id))}
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
                    {p.default && (
                      <MPBadge tone="accent">{t('badge.default')}</MPBadge>
                    )}
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
                    {colorLabel(t, p.color)} ·{' '}
                    {p.duplex === 'off'
                      ? t('detail.simplex')
                      : duplexLabel(t, p.duplex)}{' '}
                    · {p.paper}
                  </div>
                </div>
              </button>
            );
          })}
        </div>

        {/* Detail */}
        <div
          className="mp-scroll"
          style={{ flex: 1, overflow: 'auto', padding: '18px 22px' }}
        >
          {readOnly && (
            <div
              style={{
                display: 'flex',
                gap: 8,
                padding: '9px 12px',
                background: 'var(--mp-surface2)',
                border: '1px solid var(--mp-border)',
                borderRadius: 6,
                marginBottom: 18,
                alignItems: 'center',
              }}
            >
              <Icon name="lock" size={13} color="var(--mp-textMuted)" />
              <span style={{ fontSize: 11.5, color: 'var(--mp-textMuted)' }}>
                {t('notice.readOnly')}
              </span>
            </div>
          )}

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
                {showDraft && (
                  <MPBadge tone="neutral">{t('badge.unsaved')}</MPBadge>
                )}
                {!showDraft && sel.builtin && (
                  <MPBadge tone="neutral" icon="lock">
                    {t('badge.builtin')}
                  </MPBadge>
                )}
                {!showDraft && sel.default && (
                  <MPBadge tone="accent">{t('badge.default')}</MPBadge>
                )}
              </div>
              <div
                className="mp-mono"
                style={{
                  fontSize: 11,
                  color: 'var(--mp-textDim)',
                  marginTop: 4,
                }}
              >
                {t('detail.id', {
                  id: showDraft ? t('detail.unsavedId') : sel.id,
                })}
              </div>
            </div>

            {!showDraft && (
              <div style={{ display: 'flex', gap: 6 }}>
                {sel.builtin ? (
                  <MPButton
                    variant="ghost"
                    icon="copy"
                    size="sm"
                    disabled={readOnly}
                    onClick={() => guardDirty(() => startClone(sel))}
                  >
                    {t('actions.clone')}
                  </MPButton>
                ) : null}
                <span
                  style={{ display: 'inline-flex' }}
                  title={t('tooltip.export')}
                >
                  <MPButton
                    variant="ghost"
                    icon="upload"
                    size="sm"
                    disabled={!ready}
                    onClick={() => void doExport(sel)}
                  >
                    {t('actions.export')}
                  </MPButton>
                </span>
                <MPButton
                  variant="ghost"
                  icon="star"
                  size="sm"
                  disabled={readOnly || sel.default}
                  onClick={() => void setDefault(sel.id)}
                >
                  {t('actions.makeDefault')}
                </MPButton>
                {!sel.builtin && (
                  <span
                    style={{ display: 'inline-flex' }}
                    title={
                      sel.default ? t('tooltip.deleteIsDefault') : undefined
                    }
                  >
                    <MPButton
                      variant="danger"
                      icon="trash"
                      size="sm"
                      disabled={sel.default}
                      onClick={() => askDelete(sel)}
                    >
                      {t('actions.delete')}
                    </MPButton>
                  </span>
                )}
              </div>
            )}
          </div>

          {editing ? (
            <EditorSlot
              editorKey={
                showDraft
                  ? `draft-${draftNonce}`
                  : `sel-${sel.id}-${editorNonce}`
              }
              workingKey={workingKey}
              initial={sel}
              existing={profiles}
              printers={printerChoices}
              restore={restore}
              setWorking={setWorking}
              onSave={handleSave}
              onCancel={handleCancel}
              onDirtyChange={setEditorDirty}
            />
          ) : (
            detailRows
          )}
        </div>
      </div>

      {confirmState && (
        <ConfirmDialog
          state={confirmState}
          onClose={() => setConfirmState(null)}
        />
      )}
    </>
  );
}

/**
 * Mounts the editor with a referentially stable `onWorkingChange`. The callback
 * has to be stable — the editor publishes its working copy from an effect, and
 * a fresh identity on every store update would loop.
 */
function EditorSlot({
  editorKey,
  workingKey,
  setWorking,
  ...rest
}: {
  editorKey: string;
  workingKey: string;
  setWorking: (w: {
    key: string;
    profile: UiProfile;
    copiesText: string;
  }) => void;
  initial: UiProfile;
  existing: UiProfile[];
  printers: PrinterChoice[];
  restore: { key: string; profile: UiProfile; copiesText: string } | undefined;
  onSave: (p: UiProfile) => Promise<UiProfile | null>;
  onCancel: () => void;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const onWorkingChange = useCallback(
    (profile: UiProfile, copiesText: string) =>
      setWorking({ key: workingKey, profile, copiesText }),
    [setWorking, workingKey],
  );
  return (
    <ProfileEditor
      key={editorKey}
      onWorkingChange={onWorkingChange}
      {...rest}
    />
  );
}

const colorLabel = (t: T, c: UiProfile['color']) =>
  t(c === 'color' ? 'option.color' : 'option.monochrome');

const duplexLabel = (t: T, d: UiProfile['duplex']) =>
  t(
    d === 'off'
      ? 'option.duplexOff'
      : d === 'long-edge'
        ? 'option.duplexLongEdge'
        : 'option.duplexShortEdge',
  );

const orientationLabel = (t: T, o: UiProfile['orientation']) =>
  t(
    o === 'portrait'
      ? 'option.orientationPortrait'
      : o === 'landscape'
        ? 'option.orientationLandscape'
        : 'option.orientationAuto',
  );

const scalingLabel = (t: T, s: UiProfile['scaling']) =>
  t(
    s === 'fit'
      ? 'option.scalingFit'
      : s === 'shrink'
        ? 'option.scalingShrink'
        : 'option.scalingNone',
  );

const qualityLabel = (t: T, q: UiProfile['quality']) =>
  t(
    q === 'draft'
      ? 'option.qualityDraft'
      : q === 'high'
        ? 'option.qualityHigh'
        : 'option.qualityNormal',
  );
