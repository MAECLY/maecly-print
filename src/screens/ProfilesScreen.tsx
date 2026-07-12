import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import { Icon } from '@/components/Icon';
import { CenteredNotice } from '@/components/CenteredNotice';
import { ConfirmDialog, type ConfirmState } from '@/components/ConfirmDialog';
import { MPBadge, MPButton, MPTopBar } from '@/components/ui';
import { ProfileEditor } from '@/screens/profiles/ProfileEditor';
import {
  BUILT_IN_PROFILES,
  findProfileIn,
  nextUniqueName,
} from '@/lib/profiles';
import { isTauri, listPrinters } from '@/lib/ipc';
import { useProfilesStore } from '@/stores/profilesStore';
import type { UiProfile } from '@/types/domain';

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

export function ProfilesScreen({ selectedId, onSelect }: ProfilesScreenProps) {
  const profiles = useProfilesStore((s) => s.profiles);
  const status = useProfilesStore((s) => s.status);
  const error = useProfilesStore((s) => s.error);
  const init = useProfilesStore((s) => s.init);
  const save = useProfilesStore((s) => s.save);
  const remove = useProfilesStore((s) => s.remove);
  const setDefault = useProfilesStore((s) => s.setDefault);
  const defaultProfile = useProfilesStore((s) => s.defaultProfile);

  useEffect(() => {
    void init();
  }, [init]);

  // Installed printer names for the editor's printer dropdown (best-effort;
  // empty outside Tauri or on failure — the dropdown still offers "default").
  const [printerNames, setPrinterNames] = useState<string[]>([]);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    listPrinters()
      .then((ps) => {
        if (alive) setPrinterNames(ps.map((p) => p.name));
      })
      .catch(() => {
        /* non-fatal: the dropdown just stays on "System default". */
      });
    return () => {
      alive = false;
    };
  }, []);

  // An unsaved new/clone draft, plus nonces that force the editor to remount
  // (and so reset its working copy) when the draft identity or the underlying
  // profile changes out from under it.
  const [draft, setDraft] = useState<UiProfile | null>(null);
  const [draftNonce, setDraftNonce] = useState(0);
  const [editorNonce, setEditorNonce] = useState(0);
  const [confirmState, setConfirmState] = useState<ConfirmState | null>(null);
  // Mirrors the open editor's dirty flag (reported via ProfileEditor's
  // onDirtyChange). Used to guard actions that would discard unsaved edits.
  const [editorDirty, setEditorDirty] = useState(false);

  const readOnly = status === 'unavailable';
  const loading = status === 'idle' || status === 'loading';

  // Run `action`, but if the editor has unsaved edits, confirm the discard
  // first. The editor's own Cancel button stays an explicit (undialoged)
  // discard; this only guards implicit loss from switching/New/Clone.
  const guardDirty = (action: () => void) => {
    if (editorDirty) {
      setConfirmState({
        title: 'Discard unsaved changes?',
        message:
          'This profile has edits that haven’t been saved. Discard them?',
        confirmLabel: 'Discard',
        run: action,
      });
    } else {
      action();
    }
  };

  const topBar = (
    <MPTopBar
      title="Profiles"
      subtitle="Saved combinations of printer + settings"
      right={
        <MPButton
          variant="primary"
          icon="plus"
          disabled={status !== 'ready'}
          onClick={() =>
            guardDirty(() => {
              const base = findProfileIn(BUILT_IN_PROFILES, 'builtin-quick');
              setDraft(
                draftFrom(base, nextUniqueName('New profile', profiles)),
              );
              setDraftNonce((n) => n + 1);
            })
          }
        >
          New profile
        </MPButton>
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
          loadingTitle="Loading profiles…"
          errorTitle="Profiles unavailable"
        />
      </>
    );
  }

  const showDraft = draft !== null;
  const sel = showDraft ? draft : findProfileIn(profiles, selectedId);
  const editing = showDraft || !sel.builtin;

  const select = (id: string) => {
    setDraft(null);
    onSelect?.(id);
  };

  const startClone = (p: UiProfile) => {
    setDraft(draftFrom(p, nextUniqueName('Copy of ' + p.name, profiles)));
    setDraftNonce((n) => n + 1);
  };

  const handleSave = async (p: UiProfile): Promise<UiProfile | null> => {
    const saved = await save(p);
    if (saved) {
      toast.success(`Saved "${saved.name}"`);
      setDraft(null);
      setEditorNonce((n) => n + 1);
      onSelect?.(saved.id);
    }
    return saved;
  };

  const handleCancel = () => {
    setDraft(null);
    // Bump so an in-place edit of a saved custom resets to the stored values.
    setEditorNonce((n) => n + 1);
  };

  const doDelete = async (p: UiProfile) => {
    const removed = await remove(p.id);
    if (!removed) return;
    setDraft(null);
    if (selectedId === removed.id) onSelect?.(defaultProfile().id);
    toast('Profile deleted', {
      duration: 5000,
      action: {
        label: 'Undo',
        onClick: () => void save(removed),
      },
    });
  };

  const askDelete = (p: UiProfile) =>
    setConfirmState({
      title: 'Delete profile',
      message: `Delete "${p.name}"? You'll have a few seconds to undo.`,
      confirmLabel: 'Delete',
      run: () => doDelete(p),
    });

  const detailRows = (
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
                Editing profiles requires the desktop app. These are the
                built-in defaults.
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
                {showDraft && <MPBadge tone="neutral">unsaved</MPBadge>}
                {!showDraft && sel.builtin && (
                  <MPBadge tone="neutral" icon="lock">
                    built-in
                  </MPBadge>
                )}
                {!showDraft && sel.default && (
                  <MPBadge tone="accent">default</MPBadge>
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
                id: {showDraft ? '(unsaved)' : sel.id}
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
                    Clone
                  </MPButton>
                ) : null}
                <MPButton
                  variant="ghost"
                  icon="star"
                  size="sm"
                  disabled={readOnly || sel.default}
                  onClick={() => void setDefault(sel.id)}
                >
                  Make default
                </MPButton>
                {!sel.builtin && (
                  <span
                    style={{ display: 'inline-flex' }}
                    title={
                      sel.default
                        ? 'Set another profile as default first'
                        : undefined
                    }
                  >
                    <MPButton
                      variant="danger"
                      icon="trash"
                      size="sm"
                      disabled={sel.default}
                      onClick={() => askDelete(sel)}
                    >
                      Delete
                    </MPButton>
                  </span>
                )}
              </div>
            )}
          </div>

          {editing ? (
            <ProfileEditor
              key={
                showDraft
                  ? `draft-${draftNonce}`
                  : `sel-${sel.id}-${editorNonce}`
              }
              initial={sel}
              existing={profiles}
              printerNames={printerNames}
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
