import { useCallback, useEffect, useState, type ReactNode } from 'react';

import { Icon, type IconName } from '@/components/Icon';
import {
  MPBadge,
  MPButton,
  MPCard,
  MPIconButton,
  MPSectionLabel,
  MPSelect,
  MPTopBar,
  StatusDot,
  type BadgeTone,
} from '@/components/ui';
import { toast } from 'sonner';
import {
  discoverPrinters,
  fetchDefaultPrinter,
  getPrinterPrefs,
  isTauri,
  listPrinters,
  openPrinterSettings,
  printTestPage,
  setDefaultPrinter,
  setPrinterPrefs,
  type PrinterPrefs,
} from '@/lib/ipc';
import type {
  UiDiscoveredPrinter,
  UiPrinter,
  UiPrinterState,
  UiPrinterStatus,
  UiPrinterType,
} from '@/types/domain';

type PrinterMode =
  | 'default'
  | 'add-network'
  | 'add-ip'
  | 'add-system'
  | 'missing';

/** StatusDot statuses reused for printer states. */
type DotStatus = 'done' | 'cancelled' | 'failed' | 'printing' | 'queued';

const TYPE_META: Record<UiPrinterType, { label: string; ico: IconName }> = {
  laser: { label: 'Laser', ico: 'printer' },
  mfp: { label: 'Multifunction', ico: 'printer' },
  wide: { label: 'Wide format', ico: 'image' },
  label: { label: 'Label', ico: 'page' },
  virtual: { label: 'Virtual', ico: 'file' },
};

const STATUS_META: Record<
  UiPrinterStatus,
  { tone: BadgeTone; label: string; dot: DotStatus }
> = {
  ready: { tone: 'success', label: 'Ready', dot: 'done' },
  offline: { tone: 'neutral', label: 'Offline', dot: 'cancelled' },
  error: { tone: 'error', label: 'Error', dot: 'failed' },
  busy: { tone: 'accent', label: 'Printing', dot: 'printing' },
};

const TONER_COLORS: Record<string, string> = {
  cyan: '#0E9CC0',
  magenta: '#C81E5B',
  yellow: '#D9A300',
  black: '#2A2620',
};

const STATE_META: Record<UiPrinterState, { label: string; tone: BadgeTone }> = {
  default: { label: 'Default', tone: 'accent' },
  active: { label: 'Active', tone: 'neutral' },
  disabled: { label: 'Disabled', tone: 'neutral' },
};

/** Normalize a user-typed extension to leading-dot lowercase (e.g. ".pdf"). */
function normalizeExt(raw: string): string | null {
  const t = raw.trim().toLowerCase().replace(/^\*/, '');
  if (!t) return null;
  const ext = t.startsWith('.') ? t : `.${t}`;
  return /^\.[a-z0-9]+$/.test(ext) ? ext : null;
}

/** The OS name a soft-disabled printer still lives in (best-effort). */
const OS_NAME =
  typeof navigator !== 'undefined' && /Win/i.test(navigator.platform)
    ? 'Windows'
    : 'macOS';

function LevelBar({
  label,
  value,
  color,
}: {
  label: string;
  value: number;
  color: string;
}) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      <span
        className="mp-mono"
        style={{
          fontSize: 10,
          color: 'var(--mp-textDim)',
          width: 54,
          textTransform: 'capitalize',
        }}
      >
        {label}
      </span>
      <div
        style={{
          flex: 1,
          height: 6,
          background: 'var(--mp-surface3)',
          borderRadius: 3,
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            height: '100%',
            width: `${value * 100}%`,
            background: color,
            transition: 'width .3s',
          }}
        />
      </div>
      <span
        className="mp-mono"
        style={{
          fontSize: 10,
          color: value < 0.2 ? 'var(--mp-error)' : 'var(--mp-textMuted)',
          width: 30,
          textAlign: 'right',
        }}
      >
        {Math.round(value * 100)}%
      </span>
    </div>
  );
}

function CapPill({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 5,
        padding: '4px 9px',
        borderRadius: 5,
        fontSize: 11.5,
        background: ok ? 'var(--mp-successSoft)' : 'var(--mp-surface2)',
        color: ok ? 'var(--mp-success)' : 'var(--mp-textDim)',
        border: `1px solid ${ok ? 'transparent' : 'var(--mp-border)'}`,
      }}
    >
      <Icon name={ok ? 'check' : 'x'} size={11} />
      {children}
    </div>
  );
}

function Block({
  label,
  action,
  children,
}: {
  label: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div style={{ marginBottom: 18 }}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 8,
        }}
      >
        <MPSectionLabel>{label}</MPSectionLabel>
        {action}
      </div>
      {children}
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '120px 1fr',
        gap: 14,
        padding: '9px 0',
        borderBottom: '1px solid var(--mp-border)',
        alignItems: 'center',
      }}
    >
      <span style={{ fontSize: 12, color: 'var(--mp-textMuted)' }}>
        {label}
      </span>
      <span style={{ fontSize: 12.5 }}>{children}</span>
    </div>
  );
}

function CenteredNotice({
  loading,
  error,
}: {
  loading: boolean;
  error: string | null;
}) {
  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 10,
        padding: 24,
        textAlign: 'center',
      }}
    >
      {loading ? (
        <div
          style={{
            width: 22,
            height: 22,
            border: '2px solid var(--mp-surface3)',
            borderTopColor: 'var(--mp-accent)',
            borderRadius: '50%',
            animation: 'mp-spin 0.9s linear infinite',
          }}
        />
      ) : (
        <Icon name="printer" size={26} color="var(--mp-textDim)" />
      )}
      <div style={{ fontSize: 13.5, fontWeight: 500 }}>
        {loading
          ? 'Detecting printers…'
          : error
            ? 'Printers unavailable'
            : 'No printers installed'}
      </div>
      <div
        style={{
          fontSize: 12,
          color: 'var(--mp-textMuted)',
          maxWidth: 380,
          lineHeight: 1.5,
        }}
      >
        {loading
          ? 'Reading the system print spooler.'
          : (error ?? 'Add a printer in your OS settings, then press Rescan.')}
      </div>
    </div>
  );
}

interface ConfirmState {
  title: string;
  message: string;
  confirmLabel: string;
  run: () => void | Promise<void>;
}

/**
 * In-app confirmation dialog. We can't use `window.confirm` — WKWebView (the
 * macOS Tauri webview) doesn't implement JS dialogs, so it silently returns
 * `false`. This renders a real modal that works everywhere.
 */
function ConfirmDialog({
  state,
  onClose,
}: {
  state: ConfirmState;
  onClose: () => void;
}) {
  return (
    <div
      onClick={onClose}
      style={{
        position: 'absolute',
        inset: 0,
        background: 'rgba(20,18,14,0.42)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 50,
        backdropFilter: 'blur(2px)',
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 360,
          background: 'var(--mp-surface)',
          border: '1px solid var(--mp-border)',
          borderRadius: 12,
          boxShadow: '0 24px 60px rgba(0,0,0,0.28)',
          padding: 18,
        }}
      >
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 6 }}>
          {state.title}
        </div>
        <div
          style={{
            fontSize: 12.5,
            color: 'var(--mp-textMuted)',
            lineHeight: 1.5,
            marginBottom: 16,
          }}
        >
          {state.message}
        </div>
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <MPButton variant="ghost" size="sm" onClick={onClose}>
            Cancel
          </MPButton>
          <MPButton
            variant="primary"
            size="sm"
            onClick={() => {
              onClose();
              void state.run();
            }}
          >
            {state.confirmLabel}
          </MPButton>
        </div>
      </div>
    </div>
  );
}

export function PrintersScreen() {
  const [printers, setPrinters] = useState<UiPrinter[] | null>(null);
  const [prefs, setPrefs] = useState<PrinterPrefs>({});
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [defaultId, setDefaultId] = useState<string | null>(null);
  const [mode, setMode] = useState<PrinterMode>('default');
  const [busy, setBusy] = useState(false);
  const [ruleDraft, setRuleDraft] = useState<string | null>(null);
  const [confirmState, setConfirmState] = useState<ConfirmState | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) {
      setError(
        'Printers are read from the operating system — run the MaeclyPrint desktop app to manage them.',
      );
      setPrinters([]);
      return;
    }
    setPrinters(null);
    setError(null);
    try {
      const [list, def, p] = await Promise.all([
        listPrinters(),
        fetchDefaultPrinter().catch(() => null),
        getPrinterPrefs().catch(() => ({}) as PrinterPrefs),
      ]);
      setPrinters(list);
      setPrefs(p ?? {});
      setDefaultId(def ?? list.find((q) => q.isDefault)?.id ?? null);
      setSelectedId((cur) =>
        cur && list.some((q) => q.id === cur) ? cur : (list[0]?.id ?? null),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setPrinters([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Reset the inline "add rule" input whenever the selected printer changes.
  useEffect(() => setRuleDraft(null), [selectedId]);

  // Persist a prefs change optimistically; reload + toast on failure.
  const savePrefs = useCallback(
    async (next: PrinterPrefs, success?: string) => {
      setPrefs(next);
      try {
        const saved = await setPrinterPrefs(next);
        setPrefs(saved);
        if (success) toast.success(success);
      } catch (e) {
        toast.error(e instanceof Error ? e.message : String(e));
        void load();
      }
    },
    [load],
  );

  const ready = printers?.filter((p) => p.status === 'ready').length ?? 0;
  const topBar = (
    <MPTopBar
      title="Printers"
      subtitle={
        printers === null
          ? 'Detecting printers…'
          : `${printers.length} configured · ${ready} ready`
      }
      right={
        <>
          <MPButton
            variant="ghost"
            icon="rotate"
            size="sm"
            onClick={() => void load()}
          >
            Rescan
          </MPButton>
          <MPButton
            variant="primary"
            icon="plus"
            size="sm"
            disabled={!printers || printers.length === 0}
            onClick={() => setMode('add-network')}
          >
            Add printer
          </MPButton>
        </>
      }
    />
  );

  if (printers === null || printers.length === 0) {
    return (
      <>
        {topBar}
        <CenteredNotice loading={printers === null} error={error} />
      </>
    );
  }

  const sel = printers.find((p) => p.id === selectedId) ?? printers[0];
  const tm = TYPE_META[sel.type];
  const sm = STATUS_META[sel.status];

  const disabledSet = new Set(prefs.disabled ?? []);
  const appDefaultId = prefs.default_printer ?? defaultId;
  const stateOf = (id: string): UiPrinterState =>
    disabledSet.has(id)
      ? 'disabled'
      : id === appDefaultId
        ? 'default'
        : 'active';
  const selState = stateOf(sel.id);
  const isDefault = selState === 'default';
  const isDisabled = selState === 'disabled';
  const selOverride = prefs.overrides?.[sel.id];
  const selRoutes = Object.entries(prefs.routes ?? {})
    .filter(([, pid]) => pid === sel.id)
    .map(([ext]) => ext)
    .sort();

  // Action handlers — plain functions (declared after the early return, so
  // they are not hooks). Each gives toast feedback + error handling.
  const handleSetDefault = async () => {
    setBusy(true);
    const id = toast.loading(`Setting ${sel.name} as default…`);
    try {
      await setDefaultPrinter(sel.id);
      setDefaultId(sel.id);
      await savePrefs({ ...prefs, default_printer: sel.id });
      toast.success(`${sel.name} is now the default`, { id });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e), { id });
    } finally {
      setBusy(false);
    }
  };

  const runTestPage = async () => {
    setBusy(true);
    const id = toast.loading(`Sending test page to ${sel.name}…`);
    try {
      await printTestPage(sel.id);
      toast.success(`Test page sent to ${sel.name}`, { id });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e), { id });
    } finally {
      setBusy(false);
    }
  };

  const handleTestPage = () => {
    setConfirmState({
      title: 'Send test page',
      message: `Send a test page to ${sel.name}? It will print on paper.`,
      confirmLabel: 'Send test page',
      run: runTestPage,
    });
  };

  const handleToggleDisabled = async (disable: boolean) => {
    const next = new Set(disabledSet);
    if (disable) next.add(sel.id);
    else next.delete(sel.id);
    await savePrefs(
      { ...prefs, disabled: [...next] },
      disable
        ? `Removed ${sel.name} from MaeclyPrint (still installed in ${OS_NAME})`
        : `Restored ${sel.name}`,
    );
  };

  const handleOverride = async (field: 'tray' | 'paper', value: string) => {
    const overrides = { ...(prefs.overrides ?? {}) };
    overrides[sel.id] = { ...(overrides[sel.id] ?? {}), [field]: value };
    await savePrefs({ ...prefs, overrides }, `Saved default ${field}`);
  };

  const handleAddRule = async (raw: string) => {
    const ext = normalizeExt(raw);
    setRuleDraft(null);
    if (!ext) {
      toast.error('Enter a valid extension, e.g. .pdf');
      return;
    }
    await savePrefs(
      { ...prefs, routes: { ...(prefs.routes ?? {}), [ext]: sel.id } },
      `${ext} routes to ${sel.name}`,
    );
  };

  const handleRemoveRule = async (ext: string) => {
    const routes = { ...(prefs.routes ?? {}) };
    delete routes[ext];
    await savePrefs({ ...prefs, routes });
  };

  const handleOpenSettings = async () => {
    try {
      await openPrinterSettings();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <>
      {topBar}
      <div
        style={{ flex: 1, display: 'flex', minHeight: 0, position: 'relative' }}
      >
        {/* Master list */}
        <div
          className="mp-scroll"
          style={{
            width: 264,
            flexShrink: 0,
            overflow: 'auto',
            borderRight: '1px solid var(--mp-border)',
            padding: '10px 8px',
            background: 'var(--mp-surface2)',
          }}
        >
          {printers.map((p) => {
            const active = p.id === sel.id;
            const s = STATUS_META[p.status];
            const t = TYPE_META[p.type];
            const pState = stateOf(p.id);
            return (
              <button
                key={p.id}
                onClick={() => setSelectedId(p.id)}
                className="mp-focus"
                style={{
                  width: '100%',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: '9px 10px',
                  border: 'none',
                  background: active ? 'var(--mp-surface)' : 'transparent',
                  borderRadius: 6,
                  cursor: 'pointer',
                  textAlign: 'left',
                  marginBottom: 2,
                  opacity: pState === 'disabled' ? 0.5 : 1,
                  boxShadow: active
                    ? '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px var(--mp-border)'
                    : 'none',
                }}
              >
                <div
                  style={{
                    width: 30,
                    height: 30,
                    borderRadius: 6,
                    background: 'var(--mp-surface2)',
                    border: '1px solid var(--mp-border)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    flexShrink: 0,
                  }}
                >
                  <Icon name={t.ico} size={15} color="var(--mp-textMuted)" />
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div
                    style={{ display: 'flex', alignItems: 'center', gap: 6 }}
                  >
                    <span
                      style={{
                        fontSize: 12.5,
                        fontWeight: 500,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {p.name}
                    </span>
                    {pState === 'default' && (
                      <Icon name="star" size={11} color="var(--mp-accent)" />
                    )}
                  </div>
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 5,
                      marginTop: 2,
                    }}
                  >
                    <StatusDot status={s.dot} />
                    <span
                      className="mp-mono"
                      style={{ fontSize: 10, color: 'var(--mp-textDim)' }}
                    >
                      {s.label} · {t.label}
                    </span>
                  </div>
                </div>
                <MPBadge tone={STATE_META[pState].tone}>
                  {STATE_META[pState].label}
                </MPBadge>
              </button>
            );
          })}
          <button
            onClick={() => setMode('add-network')}
            className="mp-focus"
            style={{
              width: '100%',
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              padding: '9px 10px',
              border: '1px dashed var(--mp-borderStrong)',
              background: 'transparent',
              borderRadius: 6,
              cursor: 'pointer',
              marginTop: 6,
              color: 'var(--mp-textMuted)',
              fontSize: 12.5,
            }}
          >
            <Icon name="plus" size={14} /> Add a printer
          </button>
        </div>

        {/* Detail */}
        <div
          className="mp-scroll"
          style={{ flex: 1, overflow: 'auto', padding: '18px 22px' }}
        >
          {/* Header */}
          <div
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              justifyContent: 'space-between',
              marginBottom: 16,
              gap: 12,
            }}
          >
            <div style={{ display: 'flex', gap: 12, minWidth: 0 }}>
              <div
                style={{
                  width: 44,
                  height: 44,
                  borderRadius: 9,
                  background: 'var(--mp-surface2)',
                  border: '1px solid var(--mp-border)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  flexShrink: 0,
                }}
              >
                <Icon name={tm.ico} size={22} color="var(--mp-textMuted)" />
              </div>
              <div style={{ minWidth: 0 }}>
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    flexWrap: 'wrap',
                  }}
                >
                  <h2
                    style={{
                      margin: 0,
                      fontSize: 17,
                      fontWeight: 600,
                      letterSpacing: '-0.01em',
                    }}
                  >
                    {sel.name}
                  </h2>
                  <MPBadge
                    tone={sm.tone}
                    icon={
                      sm.dot === 'done'
                        ? 'check'
                        : sm.dot === 'failed'
                          ? 'alert'
                          : undefined
                    }
                  >
                    {sm.label}
                  </MPBadge>
                  {isDefault && (
                    <MPBadge tone="accent" icon="star">
                      default
                    </MPBadge>
                  )}
                </div>
                <div
                  className="mp-mono"
                  style={{
                    fontSize: 11,
                    color: 'var(--mp-textDim)',
                    marginTop: 3,
                  }}
                >
                  {tm.label} · {sel.conn} · queue {sel.queue}
                </div>
              </div>
            </div>
            <div style={{ display: 'flex', gap: 6, flexShrink: 0 }}>
              <MPButton
                variant="ghost"
                icon="printer"
                size="sm"
                disabled={busy || isDisabled}
                onClick={handleTestPage}
              >
                Test page
              </MPButton>
              <span
                style={{ display: 'inline-flex' }}
                title={
                  isDefault
                    ? 'Already your default printer'
                    : isDisabled
                      ? 'Restore this printer before setting it as default'
                      : undefined
                }
              >
                <MPButton
                  variant="ghost"
                  icon="star"
                  size="sm"
                  disabled={busy || isDefault || isDisabled}
                  onClick={() => void handleSetDefault()}
                >
                  Set default
                </MPButton>
              </span>
              {isDisabled ? (
                <MPButton
                  variant="secondary"
                  icon="rotate"
                  size="sm"
                  onClick={() => void handleToggleDisabled(false)}
                >
                  Restore
                </MPButton>
              ) : (
                <MPButton
                  variant="danger"
                  icon="trash"
                  size="sm"
                  onClick={() => void handleToggleDisabled(true)}
                >
                  Remove
                </MPButton>
              )}
            </div>
          </div>

          {/* Missing-printer reconciliation banner */}
          {mode === 'missing' && (
            <div
              style={{
                display: 'flex',
                gap: 10,
                padding: '12px 14px',
                background: 'var(--mp-warnSoft)',
                border: '1px solid var(--mp-warn)',
                borderRadius: 8,
                marginBottom: 18,
              }}
            >
              <Icon name="alert" size={16} color="var(--mp-warn)" />
              <div style={{ flex: 1 }}>
                <div
                  style={{
                    fontSize: 12.5,
                    fontWeight: 500,
                    color: 'var(--mp-warn)',
                  }}
                >
                  Printer referenced by a profile is no longer available
                </div>
                <div
                  style={{
                    fontSize: 12,
                    color: 'var(--mp-text)',
                    marginTop: 3,
                    lineHeight: 1.5,
                  }}
                >
                  Profile <strong>Press A · Duplex Long</strong> points to{' '}
                  <span className="mp-mono">Sharp MX-3071</span>, which the
                  system spooler no longer reports. Jobs using it are{' '}
                  <strong>paused</strong>, not failed.
                </div>
                <div
                  style={{
                    display: 'flex',
                    gap: 6,
                    marginTop: 10,
                    alignItems: 'center',
                  }}
                >
                  <span
                    style={{ fontSize: 11.5, color: 'var(--mp-textMuted)' }}
                  >
                    Reassign to
                  </span>
                  <MPSelect
                    size="sm"
                    value="brother"
                    onChange={() => {}}
                    options={printers
                      .filter((p) => p.status === 'ready')
                      .map((p) => ({ value: p.id, label: p.name }))}
                  />
                  <MPButton variant="primary" size="sm">
                    Update profile
                  </MPButton>
                  <MPButton variant="ghost" size="sm">
                    Keep paused
                  </MPButton>
                </div>
              </div>
            </div>
          )}

          {/* Org-managed default note (policy.json) */}
          {sel.id === 'hp404' && mode !== 'missing' && (
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
                Default printer is managed by your organization via{' '}
                <span className="mp-mono">policy.json</span>.
              </span>
            </div>
          )}

          <div
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: 24,
            }}
          >
            {/* Left column */}
            <div>
              <Block label="Connection">
                <Row label="Driver">
                  <span className="mp-mono" style={{ fontSize: 11.5 }}>
                    {sel.driver}
                  </span>
                </Row>
                <Row label="Port / address">
                  <span className="mp-mono" style={{ fontSize: 11.5 }}>
                    {sel.conn}
                  </span>
                </Row>
                <Row label="Location">{sel.location}</Row>
                <Row label="Shared">{sel.shared ? 'Yes — on LAN' : 'No'}</Row>
                <Row label="Max resolution">
                  <span className="mp-mono" style={{ fontSize: 11.5 }}>
                    {sel.maxDpi} dpi
                  </span>
                </Row>
              </Block>

              <Block label="Defaults for this printer">
                <div
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '1fr 1fr',
                    gap: 10,
                  }}
                >
                  <div>
                    <div
                      style={{
                        fontSize: 11,
                        color: 'var(--mp-textMuted)',
                        marginBottom: 5,
                      }}
                    >
                      Default tray
                    </div>
                    <MPSelect
                      fullWidth
                      value={selOverride?.tray ?? sel.defTray ?? ''}
                      onChange={(e) =>
                        void handleOverride('tray', e.target.value)
                      }
                      options={sel.trays.map((t) => ({
                        value: t.name,
                        label: t.name,
                      }))}
                    />
                  </div>
                  <div>
                    <div
                      style={{
                        fontSize: 11,
                        color: 'var(--mp-textMuted)',
                        marginBottom: 5,
                      }}
                    >
                      Default paper
                    </div>
                    <MPSelect
                      fullWidth
                      value={selOverride?.paper ?? sel.defPaper ?? ''}
                      onChange={(e) =>
                        void handleOverride('paper', e.target.value)
                      }
                      options={sel.papers.map((p) => ({ value: p, label: p }))}
                    />
                  </div>
                </div>
              </Block>

              <Block label="Route file types here">
                <div
                  style={{
                    display: 'flex',
                    flexWrap: 'wrap',
                    gap: 6,
                    alignItems: 'center',
                  }}
                >
                  {selRoutes.map((x) => (
                    <span
                      key={x}
                      className="mp-mono"
                      style={{
                        fontSize: 11,
                        padding: '3px 8px',
                        background: 'var(--mp-accentSoft)',
                        color: 'var(--mp-accent)',
                        borderRadius: 5,
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: 4,
                      }}
                    >
                      {x}
                      <button
                        type="button"
                        aria-label={`Remove ${x} rule`}
                        className="mp-focus"
                        onClick={() => void handleRemoveRule(x)}
                        style={{
                          display: 'inline-flex',
                          background: 'none',
                          border: 'none',
                          padding: 0,
                          cursor: 'pointer',
                          color: 'var(--mp-accent)',
                        }}
                      >
                        <Icon name="x" size={9} />
                      </button>
                    </span>
                  ))}
                  {ruleDraft !== null ? (
                    <input
                      autoFocus
                      className="mp-mono mp-focus"
                      placeholder=".ext"
                      value={ruleDraft}
                      onChange={(e) => setRuleDraft(e.target.value)}
                      onBlur={() => void handleAddRule(ruleDraft)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') void handleAddRule(ruleDraft);
                        if (e.key === 'Escape') setRuleDraft(null);
                      }}
                      style={{
                        fontSize: 11,
                        width: 64,
                        padding: '3px 8px',
                        borderRadius: 5,
                        border: '1px solid var(--mp-accent)',
                        background: 'var(--mp-surface)',
                        color: 'var(--mp-text)',
                        outline: 'none',
                      }}
                    />
                  ) : (
                    <button
                      type="button"
                      className="mp-focus"
                      onClick={() => setRuleDraft('')}
                      style={{
                        fontSize: 11,
                        padding: '3px 8px',
                        background: 'transparent',
                        border: '1px dashed var(--mp-borderStrong)',
                        borderRadius: 5,
                        color: 'var(--mp-textMuted)',
                        cursor: 'pointer',
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: 4,
                      }}
                    >
                      <Icon name="plus" size={10} /> add rule
                    </button>
                  )}
                </div>
                <div
                  style={{
                    fontSize: 11,
                    color: 'var(--mp-textDim)',
                    marginTop: 8,
                    lineHeight: 1.5,
                  }}
                >
                  Files with these extensions default to this printer when no
                  profile overrides it.
                </div>
              </Block>
            </div>

            {/* Right column */}
            <div>
              <Block
                label="Capabilities"
                action={
                  <span
                    className="mp-mono"
                    style={{ fontSize: 9.5, color: 'var(--mp-textDim)' }}
                  >
                    queried from driver
                  </span>
                }
              >
                <div
                  style={{
                    display: 'flex',
                    flexWrap: 'wrap',
                    gap: 6,
                    marginBottom: 12,
                  }}
                >
                  <CapPill ok={sel.duplex}>
                    Duplex {sel.duplex ? '' : 'unavailable'}
                  </CapPill>
                  <CapPill ok={sel.color}>
                    {sel.color ? 'Color' : 'Monochrome only'}
                  </CapPill>
                  <CapPill ok={true}>{sel.papers.length} paper sizes</CapPill>
                  <CapPill ok={sel.trays.length > 1}>
                    {sel.trays.length}{' '}
                    {sel.trays.length === 1 ? 'tray' : 'trays'}
                  </CapPill>
                </div>
                {!sel.duplex && (
                  <div
                    style={{
                      display: 'flex',
                      gap: 7,
                      padding: '8px 10px',
                      background: 'var(--mp-warnSoft)',
                      borderRadius: 6,
                      marginBottom: 8,
                    }}
                  >
                    <Icon name="info" size={12} color="var(--mp-warn)" />
                    <span
                      style={{
                        fontSize: 11,
                        color: 'var(--mp-warn)',
                        lineHeight: 1.45,
                      }}
                    >
                      Profiles requesting duplex will pause with{' '}
                      <span className="mp-mono">DuplexNotSupported</span> on
                      this device.
                    </span>
                  </div>
                )}
                <div style={{ display: 'flex', flexWrap: 'wrap', gap: 5 }}>
                  {sel.papers.map((p) => (
                    <span
                      key={p}
                      className="mp-mono"
                      style={{
                        fontSize: 10.5,
                        padding: '3px 7px',
                        background: 'var(--mp-surface2)',
                        border: '1px solid var(--mp-border)',
                        borderRadius: 4,
                        color: 'var(--mp-textMuted)',
                      }}
                    >
                      {p}
                    </span>
                  ))}
                </div>
              </Block>

              {Object.keys(sel.levels).length > 0 && (
                <Block label="Supply levels">
                  <div
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 8,
                      padding: '4px 0',
                    }}
                  >
                    {Object.entries(sel.levels).map(([k, v]) => (
                      <LevelBar
                        key={k}
                        label={k}
                        value={v ?? 0}
                        color={TONER_COLORS[k] ?? 'var(--mp-text)'}
                      />
                    ))}
                  </div>
                </Block>
              )}

              <Block label="Input trays">
                <MPCard padding={0}>
                  {sel.trays.map((t, i) => (
                    <div
                      key={t.name}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 10,
                        padding: '9px 12px',
                        borderBottom:
                          i < sel.trays.length - 1
                            ? '1px solid var(--mp-border)'
                            : 'none',
                      }}
                    >
                      <Icon name="file" size={13} color="var(--mp-textDim)" />
                      <span
                        style={{ fontSize: 12, fontWeight: 500, width: 64 }}
                      >
                        {t.name}
                      </span>
                      <span
                        className="mp-mono"
                        style={{
                          fontSize: 11,
                          color: 'var(--mp-textMuted)',
                          flex: 1,
                        }}
                      >
                        {t.paper ?? '—'}
                      </span>
                      {t.level != null && (
                        <div
                          style={{
                            width: 46,
                            height: 5,
                            background: 'var(--mp-surface3)',
                            borderRadius: 3,
                            overflow: 'hidden',
                          }}
                        >
                          <div
                            style={{
                              height: '100%',
                              width: `${t.level * 100}%`,
                              background:
                                t.level < 0.2
                                  ? 'var(--mp-error)'
                                  : 'var(--mp-success)',
                            }}
                          />
                        </div>
                      )}
                    </div>
                  ))}
                </MPCard>
              </Block>
            </div>
          </div>
        </div>

        {/* Add-printer modal */}
        {(mode === 'add-network' ||
          mode === 'add-ip' ||
          mode === 'add-system') && (
          <AddPrinterModal
            mode={mode}
            onMode={setMode}
            onOpenSettings={() => void handleOpenSettings()}
          />
        )}

        {confirmState && (
          <ConfirmDialog
            state={confirmState}
            onClose={() => setConfirmState(null)}
          />
        )}
      </div>
    </>
  );
}

function AddPrinterModal({
  mode,
  onMode,
  onOpenSettings,
}: {
  mode: PrinterMode;
  onMode: (m: PrinterMode) => void;
  onOpenSettings: () => void;
}) {
  const tab =
    mode === 'add-ip' ? 'ip' : mode === 'add-system' ? 'system' : 'network';

  // null = still scanning. Discovery runs whenever the network tab is shown.
  const [discovered, setDiscovered] = useState<UiDiscoveredPrinter[] | null>(
    null,
  );
  useEffect(() => {
    if (tab !== 'network') return;
    let alive = true;
    setDiscovered(null);
    discoverPrinters()
      .then((d) => alive && setDiscovered(d))
      .catch(() => alive && setDiscovered([]));
    return () => {
      alive = false;
    };
  }, [tab]);

  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        background: 'rgba(20,18,14,0.42)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 40,
        backdropFilter: 'blur(2px)',
      }}
    >
      <div
        className="mp-scroll"
        style={{
          width: 480,
          maxHeight: '88%',
          overflow: 'auto',
          background: 'var(--mp-surface)',
          border: '1px solid var(--mp-border)',
          borderRadius: 12,
          boxShadow: '0 24px 60px rgba(0,0,0,0.28)',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '14px 16px',
            borderBottom: '1px solid var(--mp-border)',
          }}
        >
          <div style={{ fontSize: 14, fontWeight: 600 }}>Add a printer</div>
          <MPIconButton
            icon="x"
            size={26}
            label="Close"
            onClick={() => onMode('default')}
          />
        </div>

        <div style={{ padding: '14px 16px' }}>
          <div
            style={{
              fontSize: 12,
              color: 'var(--mp-textMuted)',
              marginBottom: 12,
              lineHeight: 1.5,
            }}
          >
            Printers on your network, discovered via mDNS / Bonjour. To add one
            by IP, USB, or that isn&apos;t listed, use{' '}
            <strong>Open printer settings</strong> below.
          </div>

          {tab === 'network' && (
            <>
              {discovered === null && (
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    marginBottom: 10,
                  }}
                >
                  <div
                    style={{
                      width: 14,
                      height: 14,
                      border: '2px solid var(--mp-surface3)',
                      borderTopColor: 'var(--mp-accent)',
                      borderRadius: '50%',
                      animation: 'mp-spin 0.9s linear infinite',
                    }}
                  />
                  <span style={{ fontSize: 12, color: 'var(--mp-textMuted)' }}>
                    Scanning the network via mDNS / Bonjour…
                  </span>
                </div>
              )}
              {discovered !== null && discovered.length === 0 && (
                <div
                  style={{
                    fontSize: 12,
                    color: 'var(--mp-textMuted)',
                    padding: '8px 2px',
                  }}
                >
                  No new printers found on the network.
                </div>
              )}
              {discovered !== null && discovered.length > 0 && (
                <MPCard padding={0}>
                  {discovered.map((d, i) => (
                    <div
                      key={d.id}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 10,
                        padding: '11px 12px',
                        borderBottom:
                          i < discovered.length - 1
                            ? '1px solid var(--mp-border)'
                            : 'none',
                      }}
                    >
                      <div
                        style={{
                          width: 30,
                          height: 30,
                          borderRadius: 6,
                          background: 'var(--mp-surface2)',
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'center',
                        }}
                      >
                        <Icon
                          name={TYPE_META[d.type].ico}
                          size={15}
                          color="var(--mp-textMuted)"
                        />
                      </div>
                      <div style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ fontSize: 12.5, fontWeight: 500 }}>
                          {d.name}
                        </div>
                        <div
                          className="mp-mono"
                          style={{ fontSize: 10.5, color: 'var(--mp-textDim)' }}
                        >
                          {d.conn} · {d.via}
                        </div>
                      </div>
                      <MPButton
                        variant="secondary"
                        size="sm"
                        icon="openwith"
                        onClick={onOpenSettings}
                      >
                        Add
                      </MPButton>
                    </div>
                  ))}
                </MPCard>
              )}
            </>
          )}
        </div>

        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            gap: 8,
            padding: '12px 16px',
            borderTop: '1px solid var(--mp-border)',
          }}
        >
          <span style={{ fontSize: 11.5, color: 'var(--mp-textMuted)' }}>
            Printer not detected?
          </span>
          <div style={{ display: 'flex', gap: 8 }}>
            <MPButton variant="ghost" onClick={() => onMode('default')}>
              Cancel
            </MPButton>
            <MPButton
              variant="primary"
              icon="openwith"
              onClick={onOpenSettings}
            >
              Open printer settings
            </MPButton>
          </div>
        </div>
      </div>
    </div>
  );
}
