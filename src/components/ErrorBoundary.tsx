/**
 * Top-level error boundary. Without one, any exception thrown while rendering
 * unmounts the whole tree and leaves the desktop window blank with no way out
 * — the user can't even reach Settings to file a report. This catches it and
 * offers the two recoveries that always work: reload the webview, and open the
 * logs folder where the stack was written (via `log_frontend_error`).
 *
 * The class holds the boundary (React only supports them as classes); the
 * exported wrapper is a function component so the copy can come from `useT`.
 */

import { Component, type ErrorInfo, type ReactNode } from 'react';

import { MPButton } from '@/components/ui';
import { Icon } from '@/components/Icon';
import { useT, type TFunction } from '@/i18n/I18nProvider';
import { logError } from '@/lib/errorLog';
import { isTauri, openLogsFolder } from '@/lib/ipc';

interface BoundaryProps {
  children: ReactNode;
  t: TFunction<'common'>;
  /** Injected in tests; defaults to a real webview reload. */
  onReload?: (() => void) | undefined;
}

interface BoundaryState {
  error: Error | null;
}

class ErrorBoundaryInner extends Component<BoundaryProps, BoundaryState> {
  override state: BoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): BoundaryState {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo) {
    // Forwarded to the backend log (`log_frontend_error`), so the "open the
    // logs folder" button below leads the user to the actual stack.
    logError('error-boundary', error, info.componentStack);
  }

  private reload = () => {
    if (this.props.onReload) {
      this.props.onReload();
      return;
    }
    window.location.reload();
  };

  override render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    const t = this.props.t;
    return (
      <div
        role="alert"
        style={{
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 10,
          padding: 24,
          textAlign: 'center',
          background: 'var(--mp-bg)',
          color: 'var(--mp-text)',
        }}
      >
        <Icon name="alert" size={26} color="var(--mp-error)" />
        <div style={{ fontSize: 13.5, fontWeight: 600 }}>
          {t('errorBoundary.title')}
        </div>
        <div
          style={{
            fontSize: 12,
            color: 'var(--mp-textMuted)',
            maxWidth: 380,
            lineHeight: 1.5,
          }}
        >
          {t('errorBoundary.hint')}
        </div>
        <div
          aria-label={t('errorBoundary.detailsLabel')}
          style={{
            fontSize: 11.5,
            fontFamily: 'var(--mp-font-mono)',
            color: 'var(--mp-textDim)',
            maxWidth: 420,
            overflowWrap: 'anywhere',
          }}
        >
          {error.message || t('errors.unknown')}
        </div>
        <div style={{ display: 'flex', gap: 8, marginTop: 6 }}>
          <MPButton variant="primary" size="sm" onClick={this.reload}>
            {t('actions.reload')}
          </MPButton>
          {isTauri() && (
            <MPButton
              variant="secondary"
              size="sm"
              onClick={() => {
                // Already in the failure path: a rejection here must not
                // replace the panel with another crash.
                void openLogsFolder().catch((e: unknown) => {
                  console.error('could not open the logs folder', e);
                });
              }}
            >
              {t('actions.openLogsFolder')}
            </MPButton>
          )}
        </div>
      </div>
    );
  }
}

export function ErrorBoundary({
  children,
  onReload,
}: {
  children: ReactNode;
  onReload?: (() => void) | undefined;
}) {
  const t = useT('common');
  return (
    <ErrorBoundaryInner t={t} onReload={onReload}>
      {children}
    </ErrorBoundaryInner>
  );
}
