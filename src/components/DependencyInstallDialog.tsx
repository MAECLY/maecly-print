import { useEffect, useId, useRef, useState } from 'react';

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
import { unwrap } from '@/lib/ipc';
import { errorMessage } from '@/lib/ipcError';

/** Run an install the user agreed to (FR-058). Injected by tests. */
export type InstallRunner = (
  id: Dependency,
  method: InstallMethod,
) => Promise<InstallOutcome>;

/** Subscribe to an install's output lines; resolves to the unsubscribe. */
export type InstallProgressSource = (
  onLine: (id: Dependency, line: string) => void,
) => Promise<() => void>;

const realRunner: InstallRunner = (id, method) =>
  // The dialog's Install button is the consent (D13).
  unwrap(commands.installDependency(id, method, true));

const realProgress: InstallProgressSource = (onLine) =>
  events.dependencyInstallProgress.listen(({ payload }) =>
    onLine(payload.id, payload.line),
  );

/** How many output lines stay on screen. */
const MAX_LINES = 8;

const RUNS_A_COMMAND: InstallMethod[] = ['winget', 'winget-store', 'brew-cask'];

/**
 * FR-058 / D13: install a missing dependency with the user's explicit
 * consent. The dialog states the exact command that will run (or the page
 * that will open), whether Windows will ask for administrator permission,
 * whether it costs money and roughly how big it is; nothing happens until the
 * user presses Install. A command's output is shown as it runs; afterwards
 * the dependency is probed again and `onResolved` receives the fresh report.
 */
export function DependencyInstallDialog({
  report,
  onClose,
  onResolved,
  run = realRunner,
  progress = realProgress,
}: {
  report: DependencyReport;
  onClose: () => void;
  onResolved: (report: DependencyReport) => void;
  run?: InstallRunner;
  progress?: InstallProgressSource;
}) {
  const t = useT('printJob');
  const tc = useT('common');
  const dialogRef = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const descId = useId();
  const options = report.install;
  const firstAvailable = Math.max(
    0,
    options.findIndex((o) => o.available),
  );
  const [selected, setSelected] = useState(firstAvailable);
  const [running, setRunning] = useState(false);
  const [lines, setLines] = useState<string[]>([]);
  const [outcome, setOutcome] = useState<InstallOutcome | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const close = () => {
    if (!running) onClose();
  };
  useModalFocus(dialogRef, { onEscape: close });

  useEffect(() => {
    if (!running) return;
    let unlisten: (() => void) | undefined;
    let alive = true;
    progress((id, line) => {
      if (id !== report.id) return;
      setLines((prev) => [...prev, line].slice(-MAX_LINES));
    })
      .then((stop) => {
        if (alive) unlisten = stop;
        else stop();
      })
      .catch(() => {});
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [running, progress, report.id]);

  const option = options[selected];
  const runsCommand = option ? RUNS_A_COMMAND.includes(option.method) : false;

  const start = async () => {
    if (!option || !option.available) return;
    setRunning(true);
    setFailure(null);
    setOutcome(null);
    setLines([]);
    try {
      const result = await run(report.id, option.method);
      setOutcome(result);
      onResolved(result.report);
    } catch (e) {
      setFailure(errorMessage(e));
    } finally {
      setRunning(false);
    }
  };

  const resultText = (o: InstallOutcome): string => {
    switch (o.status) {
      case 'installed':
        return t('install.result.installed', { name: report.name });
      case 'not-detected':
        return t('install.result.notDetected', { name: report.name });
      case 'opened':
        return t('install.result.opened');
      case 'failed':
        return t('install.result.failed', {
          code: o.exit_code ?? '—',
        });
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
          {tc(`deps.${report.id}.remediation`)}
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

        {outcome && (
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
          <MPButton
            variant="ghost"
            size="sm"
            onClick={close}
            disabled={running}
          >
            {outcome ? tc('actions.close') : tc('actions.cancel')}
          </MPButton>
          {outcome?.status !== 'installed' && (
            <MPButton
              variant="primary"
              size="sm"
              onClick={() => void start()}
              disabled={running || !option?.available}
            >
              {runsCommand ? t('install.install') : t('install.open')}
            </MPButton>
          )}
        </div>
      </div>
    </ModalOverlay>
  );
}
