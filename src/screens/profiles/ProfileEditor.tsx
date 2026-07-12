import { useEffect, useState, type ReactNode } from 'react';

import {
  MPButton,
  MPInput,
  MPSectionLabel,
  MPSelect,
  type SelectOption,
} from '@/components/ui';
import {
  DEFAULT_PRINTER,
  isValidCopies,
  isValidPageRange,
  validateProfileName,
} from '@/lib/profiles';
import type { UiProfile } from '@/types/domain';

interface ProfileEditorProps {
  /** Seed values. A draft awaiting creation has `id === ''`. */
  initial: UiProfile;
  /** Every stored profile — for the name-uniqueness check. */
  existing: UiProfile[];
  /** Installed printer names for the printer dropdown (may be empty). */
  printerNames: string[];
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

const COLOR_OPTS: SelectOption[] = [
  { value: 'color', label: 'Color' },
  { value: 'monochrome', label: 'Monochrome' },
];
const DUPLEX_OPTS: SelectOption[] = [
  { value: 'off', label: 'Single-sided' },
  { value: 'long-edge', label: 'Double · long edge' },
  { value: 'short-edge', label: 'Double · short edge' },
];
const ORIENT_OPTS: SelectOption[] = [
  { value: 'auto', label: 'Auto' },
  { value: 'portrait', label: 'Portrait' },
  { value: 'landscape', label: 'Landscape' },
];
const SCALING_OPTS: SelectOption[] = [
  { value: 'fit', label: 'Fit to page' },
  { value: 'shrink', label: 'Shrink to fit' },
  { value: 'none', label: 'Actual size (100%)' },
];
const QUALITY_OPTS: SelectOption[] = [
  { value: 'draft', label: 'Draft' },
  { value: 'normal', label: 'Normal' },
  { value: 'high', label: 'High' },
];
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
 */
export function ProfileEditor({
  initial,
  existing,
  printerNames,
  onSave,
  onCancel,
  onDirtyChange,
}: ProfileEditorProps) {
  const isDraft = initial.id === '';
  const [form, setForm] = useState<UiProfile>(initial);
  // Copies is buffered as text so an out-of-range keystroke shows an error
  // instead of being silently coerced.
  const [copiesText, setCopiesText] = useState(String(initial.copies));
  const [saving, setSaving] = useState(false);

  const set = <K extends keyof UiProfile>(key: K, value: UiProfile[K]) =>
    setForm((f) => ({ ...f, [key]: value }));

  const rangeText = form.pageRange ?? '';
  const nameError = validateProfileName(
    form.name,
    existing,
    isDraft ? undefined : initial.id,
  );
  const copiesError = !isValidCopies(copiesText);
  const rangeError = !isValidPageRange(rangeText);
  const invalid = nameError != null || copiesError || rangeError;

  const dirty =
    isDraft ||
    editableKey(form, copiesText, rangeText) !==
      editableKey(initial, String(initial.copies), initial.pageRange ?? '');
  const canSave = dirty && !invalid && !saving;

  // Surface dirtiness to the parent so it can guard destructive navigation.
  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);
  // On unmount the editor's edits are gone; clear the parent's dirty flag so a
  // stale `true` can't linger after switching to a read-only detail.
  useEffect(() => {
    return () => onDirtyChange?.(false);
  }, [onDirtyChange]);

  // Include the current printer even if it isn't in the enumerated list (a
  // saved profile can name a printer that is currently offline/removed).
  const printerOptions: SelectOption[] = [
    { value: DEFAULT_PRINTER, label: 'System default' },
    ...Array.from(
      new Set([
        ...printerNames,
        ...(form.printer && form.printer !== DEFAULT_PRINTER
          ? [form.printer]
          : []),
      ]),
    ).map((n) => ({ value: n, label: n })),
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
      <Field label="Name" error={nameError}>
        <MPInput
          fullWidth
          value={form.name}
          onChange={(e) => set('name', e.target.value)}
          placeholder="Profile name"
          error={nameError != null}
        />
      </Field>

      <Field label="Printer">
        <MPSelect
          fullWidth
          value={form.printer}
          onChange={(e) => set('printer', e.target.value)}
          options={printerOptions}
        />
      </Field>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        <Field label="Color">
          <MPSelect
            fullWidth
            value={form.color}
            onChange={(e) => set('color', e.target.value as UiProfile['color'])}
            options={COLOR_OPTS}
          />
        </Field>
        <Field label="Duplex">
          <MPSelect
            fullWidth
            value={form.duplex}
            onChange={(e) =>
              set('duplex', e.target.value as UiProfile['duplex'])
            }
            options={DUPLEX_OPTS}
          />
        </Field>
        <Field label="Orientation">
          <MPSelect
            fullWidth
            value={form.orientation}
            onChange={(e) =>
              set('orientation', e.target.value as UiProfile['orientation'])
            }
            options={ORIENT_OPTS}
          />
        </Field>
        <Field label="Scaling">
          <MPSelect
            fullWidth
            value={form.scaling}
            onChange={(e) =>
              set('scaling', e.target.value as UiProfile['scaling'])
            }
            options={SCALING_OPTS}
          />
        </Field>
        <Field label="Quality">
          <MPSelect
            fullWidth
            value={form.quality}
            onChange={(e) =>
              set('quality', e.target.value as UiProfile['quality'])
            }
            options={QUALITY_OPTS}
          />
        </Field>
        <Field label="Paper">
          <MPSelect
            fullWidth
            value={form.paper}
            onChange={(e) => set('paper', e.target.value)}
            options={PAPER_OPTS}
          />
        </Field>
        <Field
          label="Copies"
          error={copiesError ? 'Enter a whole number from 1 to 999.' : null}
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
        <Field label="Tray">
          <MPInput
            fullWidth
            value={form.tray}
            onChange={(e) => set('tray', e.target.value)}
            placeholder="auto"
          />
        </Field>
      </div>

      <Field label="Page range">
        <MPInput
          fullWidth
          mono
          placeholder="e.g. 1-5,10"
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
          {rangeError
            ? 'Use page numbers like 1-5,10 (ranges must ascend).'
            : 'Applies to PDFs only — leave blank for all pages.'}
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
          Cancel
        </MPButton>
        <MPButton
          variant="primary"
          icon="check"
          size="sm"
          disabled={!canSave}
          onClick={() => void submit()}
        >
          {isDraft ? 'Create profile' : 'Save changes'}
        </MPButton>
      </div>
    </div>
  );
}
