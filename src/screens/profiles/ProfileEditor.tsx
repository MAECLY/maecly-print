import { useEffect, useState, type ReactNode } from 'react';

import {
  MPButton,
  MPInput,
  MPSectionLabel,
  MPSelect,
  type SelectOption,
} from '@/components/ui';
import { useT, type TFunction } from '@/i18n/I18nProvider';
import {
  DEFAULT_PRINTER,
  isValidCopies,
  isValidPageRange,
  MAX_PROFILE_NAME_CHARS,
  profileNameLength,
  validateProfileName,
} from '@/lib/profiles';
import type { PrinterChoice } from '@/lib/printerIdentity';
import type { EditorWorking } from '@/stores/profilesStore';
import type { UiProfile } from '@/types/domain';

type T = TFunction<'profiles'>;

interface ProfileEditorProps {
  /** Seed values. A draft awaiting creation has `id === ''`. */
  initial: UiProfile;
  /** Every stored profile — for the name-uniqueness check. */
  existing: UiProfile[];
  /**
   * Installed printers for the dropdown (may be empty). The option VALUE is
   * the spooler queue id — what a profile stores and the spooler addresses —
   * and the label is the display name. Saving the display name instead made
   * `lp -d "HP LaserJet Pro"` fail for queue `HP_LaserJet_Pro`.
   */
  printers: PrinterChoice[];
  /**
   * A working copy retained from a previous mount (the user left the screen
   * mid-edit). When present it seeds the form instead of `initial`, and the
   * editor says so, so restored edits are never mistaken for stored ones.
   */
  restore?: EditorWorking | undefined;
  /** Publish the live working copy so it survives an unmount. */
  onWorkingChange?: ((p: UiProfile, copiesText: string) => void) | undefined;
  /** Persist the edited profile; resolves the saved profile or null on reject. */
  onSave: (p: UiProfile) => Promise<UiProfile | null>;
  /** Discard the draft / edits. */
  onCancel: () => void;
  /**
   * Report whether the working copy diverges from `initial`, so the parent can
   * warn before an action (select/New/Clone) would discard unsaved edits. Reset
   * to `false` when the editor unmounts.
   */
  onDirtyChange?: (dirty: boolean) => void;
}

const colorOptions = (t: T): SelectOption[] => [
  { value: 'color', label: t('option.color') },
  { value: 'monochrome', label: t('option.monochrome') },
];
const duplexOptions = (t: T): SelectOption[] => [
  { value: 'off', label: t('option.duplexOff') },
  { value: 'long-edge', label: t('option.duplexLongEdge') },
  { value: 'short-edge', label: t('option.duplexShortEdge') },
];
const orientationOptions = (t: T): SelectOption[] => [
  { value: 'auto', label: t('option.orientationAuto') },
  { value: 'portrait', label: t('option.orientationPortrait') },
  { value: 'landscape', label: t('option.orientationLandscape') },
];
const scalingOptions = (t: T): SelectOption[] => [
  { value: 'fit', label: t('option.scalingFit') },
  { value: 'shrink', label: t('option.scalingShrink') },
  { value: 'none', label: t('option.scalingNone') },
];
const qualityOptions = (t: T): SelectOption[] => [
  { value: 'draft', label: t('option.qualityDraft') },
  { value: 'normal', label: t('option.qualityNormal') },
  { value: 'high', label: t('option.qualityHigh') },
];
// Paper sizes are proper nouns — the same in every locale.
const PAPER_OPTS: SelectOption[] = ['A4', 'Letter', 'Legal', 'A3'].map((v) => ({
  value: v,
  label: v,
}));

/** A stable JSON key over the editable fields, for dirty comparison. */
function editableKey(p: UiProfile, copies: string, range: string): string {
  return JSON.stringify({
    name: p.name.trim(),
    printer: p.printer,
    color: p.color,
    duplex: p.duplex,
    orientation: p.orientation,
    scaling: p.scaling,
    quality: p.quality,
    paper: p.paper,
    tray: p.tray,
    copies: copies.trim(),
    range: range.trim(),
  });
}

function Field({
  label,
  error,
  children,
}: {
  label: string;
  error?: string | null;
  children: ReactNode;
}) {
  return (
    <div>
      <MPSectionLabel>{label}</MPSectionLabel>
      {children}
      {error && (
        <div style={{ fontSize: 11, color: 'var(--mp-error)', marginTop: 4 }}>
          {error}
        </div>
      )}
    </div>
  );
}

/**
 * Controlled editor for a single custom profile (or an unsaved draft). Holds a
 * working copy so edits are cheap and reversible; validation gates the Save
 * button (invalid or unchanged → disabled). Server-side rejections surface via
 * the store's toast, and the editor is left open with its state intact so the
 * user can correct and retry. A brand-new draft (empty id) is always
 * considered dirty — creating it is itself the change.
 *
 * The working copy is published upwards on every keystroke (`onWorkingChange`)
 * so it outlives the component: leaving the screen mid-edit and coming back
 * restores exactly what was typed instead of silently dropping it.
 */
export function ProfileEditor({
  initial,
  existing,
  printers,
  restore,
  onWorkingChange,
  onSave,
  onCancel,
  onDirtyChange,
}: ProfileEditorProps) {
  const t = useT('profiles');
  const isDraft = initial.id === '';
  const [form, setForm] = useState<UiProfile>(restore?.profile ?? initial);
  // Copies is buffered as text so an out-of-range keystroke shows an error
  // instead of being silently coerced.
  const [copiesText, setCopiesText] = useState(
    restore ? restore.copiesText : String(initial.copies),
  );
  const [saving, setSaving] = useState(false);

  const set = <K extends keyof UiProfile>(key: K, value: UiProfile[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const rangeText = form.pageRange ?? '';
  const baselineKey = editableKey(
    initial,
    String(initial.copies),
    initial.pageRange ?? '',
  );
  const currentKey = editableKey(form, copiesText, rangeText);

  // `validateProfileName` owns the rules (and stays the single source of truth
  // shared with the backend's error wording); only the *message* is picked
  // locally, from the same three conditions, so it can be translated.
  const trimmedName = form.name.trim();
  const nameInvalid =
    validateProfileName(
      form.name,
      existing,
      isDraft ? undefined : initial.id,
    ) != null;
  const nameError = !nameInvalid
    ? null
    : trimmedName === ''
      ? t('editor.nameEmpty')
      : profileNameLength(trimmedName) > MAX_PROFILE_NAME_CHARS
        ? t('editor.nameTooLong')
        : t('editor.nameTaken');
  const copiesError = !isValidCopies(copiesText);
  const rangeError = !isValidPageRange(rangeText);
  const invalid = nameInvalid || copiesError || rangeError;

  const dirty = isDraft || currentKey !== baselineKey;
  const canSave = dirty && !invalid && !saving;

  // A restored copy that actually differs from what is stored deserves a note:
  // the form would otherwise look like the saved profile while it is not.
  const [showRestored] = useState(
    () =>
      restore != null &&
      !isDraft &&
      editableKey(
        restore.profile,
        restore.copiesText,
        restore.profile.pageRange ?? '',
      ) !==
        editableKey(initial, String(initial.copies), initial.pageRange ?? ''),
  );

  // Surface dirtiness to the parent so it can guard destructive navigation.
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);
  // On unmount the editor's edits are gone; clear the parent's dirty flag so a
  // stale `true` can't linger after switching to a read-only detail.
  useEffect(() => {
    return () => onDirtyChange?.(false);
  }, [onDirtyChange]);
  // Publish the working copy so an unmount (app-level navigation) does not
  // destroy it. `onWorkingChange` must be referentially stable.
  useEffect(() => {
    onWorkingChange?.(form, copiesText);
  }, [form, copiesText, onWorkingChange]);

  // Include the current printer even if it isn't in the enumerated list (a
  // saved profile can name a printer that is currently offline/removed).
  // A profile saved before ids were stored may hold a display name; the print
  // path still resolves it, and it stays selectable here under its own text.
  const savedRef =
    form.printer &&
    form.printer !== DEFAULT_PRINTER &&
    !printers.some((p) => p.id === form.printer)
      ? form.printer
      : null;
  const printerOptions: SelectOption[] = [
    { value: DEFAULT_PRINTER, label: t('option.systemDefault') },
    ...printers.map((p) => ({ value: p.id, label: p.name || p.id })),
    ...(savedRef ? [{ value: savedRef, label: savedRef }] : []),
  ];

  async function submit(): Promise<void> {
    if (!canSave) return;
    const next: UiProfile = {
      ...form,
      name: form.name.trim(),
      copies: Number(copiesText.trim()),
      builtin: false,
    };
    const pr = rangeText.trim();
    if (pr) next.pageRange = pr;
    else delete next.pageRange;
    setSaving(true);
    try {
      await onSave(next);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      {showRestored && (
        <div style={{ fontSize: 11.5, color: 'var(--mp-textMuted)' }}>
          {t('editor.restored')}
        </div>
      )}

      <Field label={t('field.name')} error={nameError}>
        <MPInput
          fullWidth
          value={form.name}
          onChange={(e) => set('name', e.target.value)}
          placeholder={t('editor.namePlaceholder')}
          error={nameError != null}
        />
      </Field>

      <Field label={t('field.printer')}>
        <MPSelect
          fullWidth
          value={form.printer}
          onChange={(e) => set('printer', e.target.value)}
          options={printerOptions}
        />
      </Field>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        <Field label={t('field.color')}>
          <MPSelect
            fullWidth
            value={form.color}
            onChange={(e) => set('color', e.target.value as UiProfile['color'])}
            options={colorOptions(t)}
          />
        </Field>
        <Field label={t('field.duplex')}>
          <MPSelect
            fullWidth
            value={form.duplex}
            onChange={(e) =>
              set('duplex', e.target.value as UiProfile['duplex'])
            }
            options={duplexOptions(t)}
          />
        </Field>
        <Field label={t('field.orientation')}>
          <MPSelect
            fullWidth
            value={form.orientation}
            onChange={(e) =>
              set('orientation', e.target.value as UiProfile['orientation'])
            }
            options={orientationOptions(t)}
          />
        </Field>
        <Field label={t('field.scaling')}>
          <MPSelect
            fullWidth
            value={form.scaling}
            onChange={(e) =>
              set('scaling', e.target.value as UiProfile['scaling'])
            }
            options={scalingOptions(t)}
          />
        </Field>
        <Field label={t('field.quality')}>
          <MPSelect
            fullWidth
            value={form.quality}
            onChange={(e) =>
              set('quality', e.target.value as UiProfile['quality'])
            }
            options={qualityOptions(t)}
          />
        </Field>
        <Field label={t('field.paper')}>
          <MPSelect
            fullWidth
            value={form.paper}
            onChange={(e) => set('paper', e.target.value)}
            options={PAPER_OPTS}
          />
        </Field>
        <Field
          label={t('field.copies')}
          error={copiesError ? t('editor.copiesError') : null}
        >
          <MPInput
            fullWidth
            mono
            type="number"
            value={copiesText}
            onChange={(e) => setCopiesText(e.target.value)}
            error={copiesError}
          />
        </Field>
        <Field label={t('field.tray')}>
          <MPInput
            fullWidth
            value={form.tray}
            onChange={(e) => set('tray', e.target.value)}
            placeholder={t('editor.trayPlaceholder')}
          />
        </Field>
      </div>

      {/*
        Honesty: quality and tray round-trip to `profiles.toml` but are not part
        of the wire `JobSettings` yet, so they do not reach the print engine
        (they are v1 settings — see docs/03-requirements/print-settings.md).
        Saying so beats two controls that silently do nothing.
      */}
      <div style={{ fontSize: 11, color: 'var(--mp-textDim)' }}>
        {t('editor.notApplied')}
      </div>

      <Field label={t('field.pageRange')}>
        <MPInput
          fullWidth
          mono
          placeholder={t('editor.rangePlaceholder')}
          value={rangeText}
          onChange={(e) => set('pageRange', e.target.value)}
          error={rangeError}
        />
        <div
          style={{
            fontSize: 11,
            color: rangeError ? 'var(--mp-error)' : 'var(--mp-textDim)',
            marginTop: 4,
          }}
        >
          {rangeError ? t('editor.rangeError') : t('editor.rangeHint')}
        </div>
      </Field>

      <div
        style={{
          display: 'flex',
          justifyContent: 'flex-end',
          gap: 8,
          marginTop: 4,
        }}
      >
        <MPButton variant="ghost" size="sm" onClick={onCancel}>
          {t('editor.cancel')}
        </MPButton>
        <MPButton
          variant="primary"
          icon="check"
          size="sm"
          disabled={!canSave}
          onClick={() => void submit()}
        >
          {isDraft ? t('editor.create') : t('editor.save')}
        </MPButton>
      </div>
    </div>
  );
}
