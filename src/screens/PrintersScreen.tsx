import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';

import { Icon, type IconName } from '@/components/Icon';
import {
  MPBadge,
  MPButton,
  MPCard,
  MPCheckbox,
  MPIconButton,
  MPInput,
  MPSectionLabel,
  MPSegmented,
  MPSelect,
  MPTopBar,
  StatusDot,
  type BadgeTone,
  type SelectOption,
} from '@/components/ui';
import { toast } from 'sonner';
import {
  addPrinter,
  discoverPrinters,
  fetchDefaultPrinter,
  getPrinterPolicy,
  getPrinterPrefs,
  isTauri,
  listPrinters,
  openPrinterSettings,
  printTestPage,
  setDefaultPrinter,
  setPrinterPrefs,
  type PrinterPolicy,
  type PrinterPrefs,
} from '@/lib/ipc';
import {
  buildPrinterUri,
  DEFAULT_PORTS,
  isValidHost,
  type PrinterProtocol,
} from '@/lib/printerAddress';
import type {
  UiDiscoveredPrinter,
  UiPrinter,
  UiPrinterState,
  UiPrinterStatus,
  UiPrinterType,
} from '@/types/domain';

type PrinterMode = 'default' | 'add-network' | 'add-ip' | 'add-system';

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
    : typeof navigator !== 'undefined' && /Linux/i.test(navigator.platform)
      ? 'Linux'
      : 'macOS';

/**
 * Windows can only install driverless IPP/IPPS queues via the Microsoft IPP
 * Class Driver; raw socket / LPD / USB devices must go through the OS pane.
 */
const IS_WINDOWS = OS_NAME === 'Windows';

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
          : (error ??
            'Use "Add printer" to install one from the network or by IP address, or press Rescan.')}
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
  // Latest prefs for slow async handlers: merging into a render-time capture
  // after an awaited OS call would silently revert concurrent prefs changes.
  const prefsRef = useRef(prefs);
  prefsRef.current = prefs;
  const [policy, setPolicy] = useState<PrinterPolicy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [defaultId, setDefaultId] = useState<string | null>(null);
  const [mode, setMode] = useState<PrinterMode>('default');
  const [busy, setBusy] = useState(false);
  const [ruleDraft, setRuleDraft] = useState<string | null>(null);
  const [confirmState, setConfirmState] = useState<ConfirmState | null>(null);
  // Missing printers the user chose to "keep paused" — hidden until reload.
  const [dismissedMissing, setDismissedMissing] = useState<Set<string>>(
    () => new Set(),
  );
  // Reassign target for the missing-printer banner (falls back to first ready).
  const [reassignTarget, setReassignTarget] = useState<string | null>(null);

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
      const [list, def, p, pol] = await Promise.all([
        listPrinters(),
        fetchDefaultPrinter().catch(() => null),
        getPrinterPrefs().catch(() => ({}) as PrinterPrefs),
        getPrinterPolicy().catch(() => null),
      ]);
      setPrinters(list);
      setPrefs(p ?? {});
      setPolicy(pol);
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
            disabled={printers === null}
            onClick={() => setMode('add-network')}
          >
            Add printer
          </MPButton>
        </>
      }
    />
  );

  if (printers === null || printers.length === 0) {
    // The add-printer flow must stay reachable with zero installed printers —
    // that is exactly the first-run scenario it exists for. The network/by-IP
    // tabs need no installed printers at all.
    return (
      <>
        {topBar}
        <CenteredNotice loading={printers === null} error={error} />
        {printers !== null &&
          (mode === 'add-network' ||
            mode === 'add-ip' ||
            mode === 'add-system') && (
            <AddPrinterModal
              mode={mode}
              onMode={setMode}
              onOpenSettings={() =>
                void openPrinterSettings().catch((e) =>
                  toast.error(e instanceof Error ? e.message : String(e)),
                )
              }
              onAdded={() => {
                setMode('default');
                void load();
              }}
              printers={printers}
              prefs={prefs}
              onSavePrefs={savePrefs}
            />
          )}
      </>
    );
  }

  const sel = printers.find((p) => p.id === selectedId) ?? printers[0];
  const tm = TYPE_META[sel.type];
  const sm = STATUS_META[sel.status];

  const disabledSet = new Set(prefs.disabled ?? []);
  // Effective default: org policy wins, then the MaeclyPrint app default, then
  // whatever the OS reports.
  const appDefaultId =
    policy?.default_printer ?? prefs.default_printer ?? defaultId;
  // When policy pins a default, the app treats it as read-only.
  const policyLocked = policy?.default_printer != null;
  const lockedName =
    printers.find((p) => p.id === policy?.default_printer)?.name ?? null;
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

  // Missing-printer reconciliation: ids the prefs reference (routing targets,
  // per-printer overrides, app default) that the spooler no longer enumerates.
  // Ids the user chose to keep paused this session are hidden.
  const enumeratedIds = new Set(printers.map((p) => p.id));
  const referencedIds = new Set<string>();
  for (const pid of Object.values(prefs.routes ?? {})) {
    if (pid) referencedIds.add(pid);
  }
  for (const pid of Object.keys(prefs.overrides ?? {})) referencedIds.add(pid);
  if (prefs.default_printer) referencedIds.add(prefs.default_printer);
  const missingIds = [...referencedIds]
    .filter((id) => !enumeratedIds.has(id) && !dismissedMissing.has(id))
    .sort();
  const missingId = missingIds[0] ?? null;
  // Reassign targets: ready, not soft-disabled in MaeclyPrint, and not a
  // virtual queue — routing a physical printer's rules to "Save as PDF" is
  // almost never what the user meant.
  const reassignCandidates = printers.filter(
    (p) =>
      p.status === 'ready' && !disabledSet.has(p.id) && p.type !== 'virtual',
  );
  const reassignEffective =
    reassignTarget && reassignCandidates.some((p) => p.id === reassignTarget)
      ? reassignTarget
      : (reassignCandidates[0]?.id ?? '');

  // Action handlers — plain functions (declared after the early return, so
  // they are not hooks). Each gives toast feedback + error handling.
  const handleSetDefault = async () => {
    setBusy(true);
    const id = toast.loading(`Setting ${sel.name} as default…`);
    try {
      await setDefaultPrinter(sel.id);
      setDefaultId(sel.id);
      // Merge into the LATEST prefs, not the render-time capture: the OS call
      // above can be slow, and whole-object-saving stale prefs would silently
      // revert any concurrent prefs change (routes, disabled, overrides).
      await savePrefs({ ...prefsRef.current, default_printer: sel.id });
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
    setRuleDraft(null);
    // Blurring or submitting the still-empty input just cancels the draft;
    // only actual invalid text earns an error toast.
    if (!raw.trim()) return;
    const ext = normalizeExt(raw);
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

  // Repoint every reference to a missing printer at `target`.
  const handleReassignMissing = async (mid: string, target: string) => {
    if (!target) return;
    const targetName = printers.find((p) => p.id === target)?.name ?? target;
    const routes = { ...(prefs.routes ?? {}) };
    for (const [ext, pid] of Object.entries(routes)) {
      if (pid === mid) routes[ext] = target;
    }
    // Tray/paper overrides are bin/media names specific to one model — they
    // don't transfer to a different device, so drop them rather than carry
    // stale names forward.
    const overrides = { ...(prefs.overrides ?? {}) };
    delete overrides[mid];
    const next: PrinterPrefs = { ...prefs, routes, overrides };
    if (prefs.default_printer === mid) next.default_printer = target;
    setReassignTarget(null);
    await savePrefs(next, `References updated to ${targetName}`);
  };

  // Drop every reference to a missing printer without reassigning.
  const handleForgetMissing = async (mid: string) => {
    const routes = { ...(prefs.routes ?? {}) };
    for (const [ext, pid] of Object.entries(routes)) {
      if (pid === mid) delete routes[ext];
    }
    const overrides = { ...(prefs.overrides ?? {}) };
    delete overrides[mid];
    const next: PrinterPrefs = { ...prefs, routes, overrides };
    if (prefs.default_printer === mid) next.default_printer = null;
    await savePrefs(next, 'Removed references to the missing printer');
  };

  // Keep the missing printer's jobs paused; hide the banner for this session
  // without touching prefs.
  const handleDismissMissing = (mid: string) => {
    setReassignTarget(null);
    setDismissedMissing((cur) => new Set(cur).add(mid));
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
                  policyLocked
                    ? 'Your organization manages the default printer'
                    : isDefault
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
                  disabled={busy || isDefault || isDisabled || policyLocked}
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
              ) : sel.type !== 'virtual' ? (
                <MPButton
                  variant="danger"
                  icon="trash"
                  size="sm"
                  onClick={() => void handleToggleDisabled(true)}
                >
                  Remove
                </MPButton>
              ) : null}
            </div>
          </div>

          {/* Missing-printer reconciliation banner */}
          {missingId && (
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
                  Printer referenced by MaeclyPrint settings is no longer
                  available
                </div>
                <div
                  style={{
                    fontSize: 12,
                    color: 'var(--mp-text)',
                    marginTop: 3,
                    lineHeight: 1.5,
                  }}
                >
                  <span className="mp-mono">{missingId}</span> is referenced by
                  your MaeclyPrint settings, but the system spooler no longer
                  reports it. Jobs sent to it will <strong>fail</strong> until
                  you reassign or forget it.
                  {missingIds.length > 1 && (
                    <> ({missingIds.length - 1} more also missing.)</>
                  )}
                </div>
                <div
                  style={{
                    display: 'flex',
                    gap: 6,
                    marginTop: 10,
                    alignItems: 'center',
                    flexWrap: 'wrap',
                  }}
                >
                  <span
                    style={{ fontSize: 11.5, color: 'var(--mp-textMuted)' }}
                  >
                    Reassign to
                  </span>
                  <MPSelect
                    size="sm"
                    value={reassignEffective}
                    onChange={(e) => setReassignTarget(e.target.value)}
                    options={reassignCandidates.map((p) => ({
                      value: p.id,
                      label: p.name,
                    }))}
                  />
                  <MPButton
                    variant="primary"
                    size="sm"
                    disabled={!reassignEffective}
                    onClick={() =>
                      void handleReassignMissing(missingId, reassignEffective)
                    }
                  >
                    Update references
                  </MPButton>
                  <MPButton
                    variant="ghost"
                    size="sm"
                    onClick={() => void handleForgetMissing(missingId)}
                  >
                    Forget
                  </MPButton>
                  <MPButton
                    variant="ghost"
                    size="sm"
                    onClick={() => handleDismissMissing(missingId)}
                  >
                    Keep paused
                  </MPButton>
                </div>
              </div>
            </div>
          )}

          {/* Org-managed default note (policy.json) */}
          {policyLocked && (
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
                {lockedName ? (
                  <>
                    Default printer <strong>{lockedName}</strong> is managed by
                    your organization via{' '}
                    <span className="mp-mono">policy.json</span>.
                  </>
                ) : (
                  <>
                    Your organization pins the default printer to{' '}
                    <span className="mp-mono">{policy?.default_printer}</span>{' '}
                    via <span className="mp-mono">policy.json</span>, but the
                    system spooler does not currently report it. Ask your
                    administrator, or add it via Add printer.
                  </>
                )}
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
            onAdded={() => {
              setMode('default');
              void load();
            }}
            printers={printers}
            prefs={prefs}
            onSavePrefs={savePrefs}
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

type AddTab = 'network' | 'ip' | 'system';

interface AddPrinterModalProps {
  mode: PrinterMode;
  onMode: (m: PrinterMode) => void;
  onOpenSettings: () => void;
  /** Called after a printer is installed — closes the modal + reloads. */
  onAdded: () => void;
  printers: UiPrinter[];
  prefs: PrinterPrefs;
  onSavePrefs: (next: PrinterPrefs, success?: string) => void | Promise<void>;
}

const ADD_TABS: { value: AddTab; label: string; mode: PrinterMode }[] = [
  { value: 'network', label: 'Scan network', mode: 'add-network' },
  { value: 'ip', label: 'By IP address', mode: 'add-ip' },
  { value: 'system', label: 'From system', mode: 'add-system' },
];

function AddPrinterModal({
  mode,
  onMode,
  onOpenSettings,
  onAdded,
  printers,
  prefs,
  onSavePrefs,
}: AddPrinterModalProps) {
  const tab: AddTab =
    mode === 'add-ip' ? 'ip' : mode === 'add-system' ? 'system' : 'network';

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
        role="dialog"
        aria-modal="true"
        aria-label="Add a printer"
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
          <div style={{ marginBottom: 14 }}>
            <MPSegmented
              size="sm"
              value={tab}
              onChange={(t) =>
                onMode(
                  ADD_TABS.find((x) => x.value === t)?.mode ?? 'add-network',
                )
              }
              options={ADD_TABS.map((x) => ({
                value: x.value,
                label: x.label,
              }))}
            />
          </div>

          {tab === 'network' && <NetworkTab onAdded={onAdded} />}
          {tab === 'ip' && <IpTab onAdded={onAdded} />}
          {tab === 'system' && (
            <SystemTab
              printers={printers}
              prefs={prefs}
              onSavePrefs={onSavePrefs}
            />
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
          <MPButton variant="ghost" icon="openwith" onClick={onOpenSettings}>
            Open printer settings
          </MPButton>
          <MPButton variant="secondary" onClick={() => onMode('default')}>
            {tab === 'system' ? 'Done' : 'Close'}
          </MPButton>
        </div>
      </div>
    </div>
  );
}

/** Label + control stacked vertically, reused by the modal forms. */
function ModalField({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div>
      <div
        style={{
          fontSize: 11,
          color: 'var(--mp-textMuted)',
          marginBottom: 5,
        }}
      >
        {label}
      </div>
      {children}
    </div>
  );
}

/** Inline error text shown under a control or row after a failed add. */
function InlineError({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: 'flex',
        gap: 6,
        alignItems: 'flex-start',
        fontSize: 11,
        color: 'var(--mp-error)',
        lineHeight: 1.45,
      }}
    >
      <Icon name="alert" size={12} color="var(--mp-error)" />
      <span style={{ flex: 1 }}>{children}</span>
    </div>
  );
}

/** "Scan network" tab — mDNS/Bonjour discovery with a per-row Add action. */
function NetworkTab({ onAdded }: { onAdded: () => void }) {
  // null = still scanning; discovery re-runs each time this tab mounts.
  const [discovered, setDiscovered] = useState<UiDiscoveredPrinter[] | null>(
    null,
  );
  const [addingId, setAddingId] = useState<string | null>(null);
  const [rowError, setRowError] = useState<Record<string, string>>({});
  const [scanError, setScanError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setDiscovered(null);
    setScanError(null);
    discoverPrinters()
      .then((d) => {
        if (alive) setDiscovered(d);
      })
      .catch((e) => {
        // A failed scan is not an empty network — say so, don't render the
        // "no new printers" copy over a real error.
        if (alive) {
          setDiscovered([]);
          setScanError(e instanceof Error ? e.message : String(e));
        }
      });
    return () => {
      alive = false;
    };
  }, []);

  const handleAdd = async (d: UiDiscoveredPrinter) => {
    setAddingId(d.id);
    setRowError((e) => {
      const next = { ...e };
      delete next[d.id];
      return next;
    });
    const tid = toast.loading(`Adding ${d.name}…`);
    try {
      await addPrinter(d.name, d.id);
      toast.success(`${d.name} added`, { id: tid });
      onAdded();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setRowError((e) => ({ ...e, [d.id]: msg }));
      toast.error(msg, { id: tid });
    } finally {
      setAddingId(null);
    }
  };

  return (
    <>
      <div
        style={{
          fontSize: 12,
          color: 'var(--mp-textMuted)',
          marginBottom: 12,
          lineHeight: 1.5,
        }}
      >
        Printers on your network, discovered via mDNS / Bonjour. Not listed? Try{' '}
        <strong>By IP address</strong>, or{' '}
        <strong>Open printer settings</strong> for USB and other transports.
      </div>

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
            color: scanError ? 'var(--mp-error)' : 'var(--mp-textMuted)',
            padding: '8px 2px',
          }}
        >
          {scanError
            ? `Network scan failed: ${scanError}`
            : 'No new printers found on the network.'}
        </div>
      )}
      {discovered !== null && discovered.length > 0 && (
        <MPCard padding={0}>
          {discovered.map((d, i) => (
            <div
              key={d.id}
              style={{
                borderBottom:
                  i < discovered.length - 1
                    ? '1px solid var(--mp-border)'
                    : 'none',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: '11px 12px',
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
                  icon="plus"
                  // Any in-flight add disables every row: clicking a second
                  // row mid-add would race two lpadmin/Add-Printer calls.
                  disabled={addingId !== null}
                  onClick={() => void handleAdd(d)}
                >
                  {addingId === d.id ? 'Adding…' : 'Add'}
                </MPButton>
              </div>
              {rowError[d.id] && (
                <div style={{ padding: '0 12px 10px 52px' }}>
                  <InlineError>{rowError[d.id]}</InlineError>
                </div>
              )}
            </div>
          ))}
        </MPCard>
      )}
    </>
  );
}

/** "By IP address" tab — compose a device URI and install it. */
function IpTab({ onAdded }: { onAdded: () => void }) {
  const [host, setHost] = useState('');
  const [proto, setProto] = useState<PrinterProtocol>('ipp');
  const [port, setPort] = useState(String(DEFAULT_PORTS.ipp));
  const [queuePath, setQueuePath] = useState('');
  const [name, setName] = useState('');
  const [nameEdited, setNameEdited] = useState(false);
  const [busy, setBusy] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  // The display name tracks the host until the user types their own.
  const effectiveName = nameEdited ? name : host;
  const hostOk = isValidHost(host);
  const canConnect = !busy && hostOk && effectiveName.trim() !== '';
  const hasQueuePath = proto === 'ipp' || proto === 'ipps';

  const protoOptions: SelectOption[] = IS_WINDOWS
    ? [
        { value: 'ipp', label: 'IPP' },
        { value: 'ipps', label: 'IPPS (secure)' },
      ]
    : [
        { value: 'ipp', label: 'IPP' },
        { value: 'ipps', label: 'IPPS (secure)' },
        { value: 'socket', label: 'Raw · JetDirect 9100' },
        { value: 'lpd', label: 'LPD / LPR' },
      ];

  const onProtoChange = (value: string) => {
    const p = value as PrinterProtocol;
    setProto(p);
    setPort(String(DEFAULT_PORTS[p]));
  };

  const handleConnect = async () => {
    const finalName = effectiveName.trim();
    if (!hostOk || !finalName) return;
    const parsed = Number(port);
    const portArg =
      port.trim() !== '' && Number.isFinite(parsed) ? parsed : undefined;
    const uri = buildPrinterUri(proto, host, portArg, queuePath);
    setBusy(true);
    setErrorMsg(null);
    const tid = toast.loading(`Adding ${finalName}…`);
    try {
      await addPrinter(finalName, uri);
      toast.success(`${finalName} added`, { id: tid });
      onAdded();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setErrorMsg(msg);
      toast.error(msg, { id: tid });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <ModalField label="Host / IP address">
        <MPInput
          fullWidth
          value={host}
          onChange={(e) => setHost(e.target.value)}
          placeholder="10.0.0.41 or printer.local"
          error={host !== '' && !hostOk}
        />
      </ModalField>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
        <ModalField label="Protocol">
          <MPSelect
            fullWidth
            value={proto}
            onChange={(e) => onProtoChange(e.target.value)}
            options={protoOptions}
          />
        </ModalField>
        <ModalField label="Port">
          <MPInput
            fullWidth
            type="number"
            value={port}
            onChange={(e) => setPort(e.target.value)}
          />
        </ModalField>
      </div>

      {IS_WINDOWS && (
        <div
          style={{
            display: 'flex',
            gap: 7,
            padding: '8px 10px',
            background: 'var(--mp-surface2)',
            border: '1px solid var(--mp-border)',
            borderRadius: 6,
          }}
        >
          <Icon name="info" size={12} color="var(--mp-textMuted)" />
          <span
            style={{
              fontSize: 11,
              color: 'var(--mp-textMuted)',
              lineHeight: 1.45,
            }}
          >
            Windows installs IPP / IPPS queues only. Add raw (9100), LPD, or USB
            printers via <strong>Open printer settings</strong>.
          </span>
        </div>
      )}

      <ModalField label="Queue path (optional)">
        <MPInput
          fullWidth
          mono
          value={queuePath}
          onChange={(e) => setQueuePath(e.target.value)}
          placeholder={hasQueuePath ? 'ipp/print' : 'none'}
        />
      </ModalField>

      <ModalField label="Display name">
        <MPInput
          fullWidth
          value={effectiveName}
          onChange={(e) => {
            setNameEdited(true);
            setName(e.target.value);
          }}
          placeholder="Front desk printer"
        />
      </ModalField>

      {errorMsg && <InlineError>{errorMsg}</InlineError>}

      <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
        <MPButton
          variant="primary"
          icon="plus"
          disabled={!canConnect}
          onClick={() => void handleConnect()}
        >
          {busy ? 'Connecting…' : 'Connect'}
        </MPButton>
      </div>
    </div>
  );
}

/** "From system" tab — tick queues to manage in MaeclyPrint (soft-disable). */
function SystemTab({
  printers,
  prefs,
  onSavePrefs,
}: {
  printers: UiPrinter[];
  prefs: PrinterPrefs;
  onSavePrefs: (next: PrinterPrefs, success?: string) => void | Promise<void>;
}) {
  const disabled = new Set(prefs.disabled ?? []);

  const toggle = async (p: UiPrinter) => {
    const next = new Set(disabled);
    const willHide = !next.has(p.id);
    if (willHide) next.add(p.id);
    else next.delete(p.id);
    await onSavePrefs(
      { ...prefs, disabled: [...next] },
      willHide
        ? `${p.name} hidden from MaeclyPrint`
        : `${p.name} is managed by MaeclyPrint again`,
    );
  };

  return (
    <>
      <div
        style={{
          fontSize: 12,
          color: 'var(--mp-textMuted)',
          marginBottom: 12,
          lineHeight: 1.5,
        }}
      >
        Queues already known to {OS_NAME}. Unticking one hides it from
        MaeclyPrint (soft-disable) — the OS print queue is left untouched.
      </div>
      <MPCard padding={0}>
        {printers.map((p, i) => (
          <div
            key={p.id}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              padding: '10px 12px',
              borderBottom:
                i < printers.length - 1 ? '1px solid var(--mp-border)' : 'none',
            }}
          >
            <MPCheckbox
              checked={!disabled.has(p.id)}
              onChange={() => void toggle(p)}
              label={
                <span style={{ fontSize: 12.5 }}>
                  <span>{p.name}</span>
                  <span
                    className="mp-mono"
                    style={{ color: 'var(--mp-textDim)', marginLeft: 6 }}
                  >
                    · {TYPE_META[p.type].label}
                  </span>
                </span>
              }
            />
          </div>
        ))}
      </MPCard>
    </>
  );
}
