import { useEffect, useId, useRef, useState } from 'react';
import { toast } from 'sonner';

import { ModalOverlay } from '@/components/Modal';
import { useModalFocus } from '@/components/useModal';
import { MPButton } from '@/components/ui';
import { useT } from '@/i18n/I18nProvider';
import {
  commands,
  events,
  type Dependency,
  type DependencyReport,
  type InstallMethod,
  type InstallOutcome,
} from '@/ipc/bindings';
import { cancelMayContinue, remediationKey } from '@/lib/deps';
import { unwrap } from '@/lib/ipc';
import { errorMessage } from '@/lib/ipcError';
import {
  useInstallStore,
  type InstallEnd,
  type InstallHandlers,
} from '@/stores/installStore';

/** Run an install the user agreed to (FR-058). Injected by tests. */
export type InstallRunner = (
  id: Dependency,
  method: InstallMethod,
) => Promise<InstallOutcome>;

/** Stop the running install; resolves to whether one was running. */
export type InstallCanceller = () => Promise<boolean>;

/** Subscribe to an install's output lines; resolves to the unsubscribe. */
export type InstallProgressSource = (
  onLine: (id: Dependency, line: string) => void,
) => Promise<() => void>;

const realRunner: InstallRunner = (id, method) =>
  // The dialog's Install button is the consent (D13).
  unwrap(commands.installDependency(id, method, true));

const realCancel: InstallCanceller = () =>
  unwrap(commands.cancelDependencyInstall());

const realProgress: InstallProgressSource = (onLine) =>
  events.dependencyInstallProgress.listen(({ payload }) =>
    onLine(payload.id, payload.line),
  );

const RUNS_A_COMMAND: InstallMethod[] = ['winget', 'winget-store', 'brew-cask'];

/**
 * FR-058 / D13: install a missing dependency with the user's explicit
 * consent. The dialog states the exact command that will run (or the page
 * that will open), whether Windows will ask for administrator permission,
 * whether it costs money and roughly how big it is; nothing happens until the
 * user presses Install. A command's output is shown as it runs; afterwards
 * the dependency is probed again and `onResolved` receives the fresh report.
 *
 * A running install never holds the screen hostage: "Stop installation"
 * asks it to stop, and closing the dialog — the button, Escape or the
 * backdrop — lets it carry on in the background, with a toast saying how it
 * ended. The install lives in `useInstallStore`, not in the dialog, so a
 * dialog reopened for the same dependency shows it still running, Stop
 * included, and one opened for another dependency names the busy one.
 *
 * `resumesJobs` says the caller resumes the jobs waiting for the dependency
 * once it is found (the Queue), so the "opened" result can promise that.
 */
export function DependencyInstallDialog({
  report,
  onClose,
  onResolved,
  resumesJobs = false,
  run = realRunner,
  cancel = realCancel,
  progress = realProgress,
}: {
  report: DependencyReport;
  onClose: () => void;
  onResolved: (report: DependencyReport) => void;
  resumesJobs?: boolean;
  run?: InstallRunner;
  cancel?: InstallCanceller;
  progress?: InstallProgressSource;
}) {
  const t = useT('printJob');
  const tc = useT('common');
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const descId = useId();
  const options = report.install;
  const active = useInstallStore((s) => s.active);
  const storeLines = useInstallStore((s) => s.lines);
  const storeStopping = useInstallStore((s) => s.stopping);
  const last = useInstallStore((s) => s.last);
  const startInstall = useInstallStore((s) => s.start);
  const stopInstall = useInstallStore((s) => s.stop);
  const watch = useInstallStore((s) => s.watch);
  /** This dependency is being installed (maybe from an earlier dialog). */
  const running = active?.id === report.id;
  /** Another dependency is being installed: this one has to wait. */
  const busyWith = active && !running ? active : null;
  const [selected, setSelected] = useState(() => {
    // Reopened on a running install: show the way it is being installed.
    const current = useInstallStore.getState().active;
    const i =
      current?.id === report.id
        ? options.findIndex((o) => o.method === current.method)
        : -1;
    return i >= 0
      ? i
      : Math.max(
          0,
          options.findIndex((o) => o.available),
        );
  });
  const stopping = running && storeStopping;
  const [outcome, setOutcome] = useState<InstallOutcome | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // The last end the store had when this dialog opened: anything newer for
  // this dependency ended while it was open, and is shown here.
  const [seenSeq] = useState(() => useInstallStore.getState().last?.seq ?? 0);
  const endedHere =
    last !== null && last.id === report.id && last.seq > seenSeq;
  // The output stays on screen after the end it led to.
  const lines = running || endedHere ? storeLines : [];

  // Closing never blocks: a running install carries on in the background.
  const close = () => onClose();
  useModalFocus(dialogRef, { onEscape: close });

  const resultText = (o: InstallOutcome): string => {
    if (cancelMayContinue(o)) {
      return t('install.result.cancelRequestedInstallerMayContinue', {
        name: report.name,
      });
    }
    switch (o.status) {
      case 'installed':
        return t('install.result.installed', { name: report.name });
      case 'not-detected':
        return t('install.result.notDetected', { name: report.name });
      case 'opened':
        return resumesJobs
          ? t('install.result.openedResumesJobs', { name: report.name })
          : t('install.result.opened');
      case 'timed-out':
        return t('install.result.timedOut');
      case 'cancelled':
        return t('install.result.cancelled');
      case 'timed-out-installer-may-continue':
      case 'cancel-requested-installer-may-continue':
        return t('install.result.cancelRequestedInstallerMayContinue', {
          name: report.name,
        });
      case 'failed':
        // No exit code means the OS gave none: say so instead of "code —".
        return o.exit_code == null
          ? t('install.result.failedNoCode')
          : t('install.result.failed', { code: o.exit_code });
    }
  };

  /** How an install that ended with no dialog open for it is announced. */
  const announce = (end: InstallEnd) => {
    if (!end.outcome) {
      toast.error(end.failure);
      return;
    }
    const o = end.outcome;
    const title = t('install.title', { name: report.name });
    const description = resultText(o);
    if (o.status === 'installed') toast.success(title, { description });
    else if (o.status === 'cancelled' && !cancelMayContinue(o))
      toast.message(title, { description });
    else toast.warning(title, { description });
  };

  // While this dialog is open, its dependency's install reports through it.
  const handlersRef = useRef<InstallHandlers>({ onResolved, announce });
  useEffect(() => {
    handlersRef.current = { onResolved, announce };
  });
  const [handlers] = useState<InstallHandlers>(() => ({
    onResolved: (r) => handlersRef.current.onResolved(r),
    announce: (e) => handlersRef.current.announce(e),
  }));
  useEffect(() => watch(report.id, handlers), [watch, report.id, handlers]);

  // Show an end that happens while this dialog is open.
  const shownSeq = useRef(seenSeq);
  useEffect(() => {
    if (!last || last.seq <= shownSeq.current) return;
    shownSeq.current = last.seq;
    if (last.id !== report.id) return;
    setOutcome(last.outcome ?? null);
    setFailure(last.failure ?? null);
  }, [last, report.id]);

  const option = options[selected];
  const runsCommand = option ? RUNS_A_COMMAND.includes(option.method) : false;

  const start = () => {
    if (!option || !option.available || active) return;
    setFailure(null);
    setOutcome(null);
    startInstall(
      report,
      option.method,
      { run, progress, describe: errorMessage },
      handlers,
    );
  };

  const stop = async () => {
    try {
      await stopInstall(cancel);
    } catch (e) {
      setFailure(errorMessage(e));
    }
  };

  return (
    <ModalOverlay onBackdropClick={close}>
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descId}
        style={{
          width: 480,
          maxWidth: 'calc(100vw - 32px)',
          background: 'var(--mp-surface)',
          border: '1px solid var(--mp-border)',
          borderRadius: 12,
          boxShadow: '0 24px 60px rgba(0,0,0,0.28)',
          padding: 18,
        }}
      >
        <div
          id={titleId}
          style={{ fontSize: 14, fontWeight: 600, marginBottom: 6 }}
        >
          {t('install.title', { name: report.name })}
        </div>
        <div
          id={descId}
          style={{
            fontSize: 12.5,
            color: 'var(--mp-textMuted)',
            lineHeight: 1.5,
            marginBottom: 12,
          }}
        >
          {tc(remediationKey(report.id))}
        </div>

        <fieldset
          disabled={running}
          style={{ border: 0, padding: 0, margin: '0 0 12px' }}
        >
          <legend style={{ fontSize: 12, fontWeight: 600, marginBottom: 6 }}>
            {t('install.how')}
          </legend>
          {options.map((o, i) => (
            <label
              key={o.method}
              style={{
                display: 'flex',
                alignItems: 'flex-start',
                gap: 8,
                fontSize: 12.5,
                marginBottom: 6,
                opacity: o.available ? 1 : 0.55,
              }}
            >
              <input
                type="radio"
                name={`install-${report.id}`}
                checked={selected === i}
                disabled={!o.available}
                onChange={() => setSelected(i)}
              />
              <span>
                {t(`install.method.${o.method}`)}
                {!o.available && (
                  <span
                    style={{
                      display: 'block',
                      fontSize: 11,
                      color: 'var(--mp-textDim)',
                    }}
                  >
                    {t('install.unavailable')}
                  </span>
                )}
              </span>
            </label>
          ))}
        </fieldset>

        {option && (
          <div style={{ fontSize: 12, lineHeight: 1.5, marginBottom: 12 }}>
            <div style={{ marginBottom: 4 }}>
              {runsCommand ? t('install.willRun') : t('install.willOpen')}
            </div>
            <code
              data-testid="install-command"
              style={{
                display: 'block',
                fontSize: 11.5,
                padding: '6px 8px',
                borderRadius: 6,
                background: 'var(--mp-surface2)',
                fontFamily: 'var(--mp-font-mono)',
                border: '1px solid var(--mp-border)',
                overflowWrap: 'anywhere',
              }}
            >
              {option.command}
            </code>
            <ul style={{ margin: '8px 0 0', paddingLeft: 18 }}>
              {option.needs_admin && <li>{t('install.needsAdmin')}</li>}
              {option.paid && <li>{t('install.paid')}</li>}
              {option.approx_mb != null && (
                <li>{t('install.size', { mb: option.approx_mb })}</li>
              )}
              <li>{t('install.neverSilent')}</li>
            </ul>
          </div>
        )}

        {(running || lines.length > 0) && (
          <pre
            aria-live="polite"
            aria-label={t('install.output')}
            style={{
              fontSize: 11,
              maxHeight: 120,
              overflow: 'auto',
              whiteSpace: 'pre-wrap',
              margin: '0 0 12px',
              padding: 8,
              borderRadius: 6,
              border: '1px solid var(--mp-border)',
            }}
          >
            {running && lines.length === 0 ? t('install.running') : null}
            {lines.join('\n')}
          </pre>
        )}

        {running && (
          <div
            style={{
              fontSize: 11.5,
              color: 'var(--mp-textMuted)',
              marginBottom: 12,
            }}
          >
            {stopping ? t('install.stopping') : t('install.backgroundHint')}
          </div>
        )}

        {busyWith && (
          <div
            role="status"
            data-testid="install-busy"
            style={{ fontSize: 12.5, marginBottom: 12 }}
          >
            {t('install.busyWith', { name: busyWith.name })}
          </div>
        )}

        {outcome && !running && (
          <div role="status" style={{ fontSize: 12.5, marginBottom: 12 }}>
            {resultText(outcome)}
          </div>
        )}
        {failure && (
          <div
            role="alert"
            style={{
              fontSize: 12.5,
              marginBottom: 12,
              color: 'var(--mp-error)',
            }}
          >
            {failure}
          </div>
        )}

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          {running ? (
            <>
              <MPButton
                variant="danger"
                size="sm"
                onClick={() => void stop()}
                disabled={stopping}
              >
                {t('install.stop')}
              </MPButton>
              <MPButton variant="ghost" size="sm" onClick={close}>
                {t('install.background')}
              </MPButton>
            </>
          ) : (
            <MPButton variant="ghost" size="sm" onClick={close}>
              {outcome ? tc('actions.close') : tc('actions.cancel')}
            </MPButton>
          )}
          {!running && outcome?.status !== 'installed' && (
            <MPButton
              variant="primary"
              size="sm"
              onClick={start}
              disabled={busyWith !== null || !option?.available}
            >
              {runsCommand ? t('install.install') : t('install.open')}
            </MPButton>
          )}
        </div>
      </div>
    </ModalOverlay>
  );
}
