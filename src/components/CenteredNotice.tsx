import { Icon, type IconName } from '@/components/Icon';

interface CenteredNoticeProps {
  /** Show the spinner + loading copy. Takes precedence over `error`. */
  loading: boolean;
  /** When set (and not loading), render the error copy with this message. */
  error: string | null;
  /** Title while loading. Defaults to the Printers-screen copy. */
  loadingTitle?: string;
  /** Title for the empty (no data, no error) state. */
  emptyTitle?: string;
  /** Title for the error state. Defaults to the Printers-screen copy. */
  errorTitle?: string;
  /** Body for the empty state. */
  hint?: string;
  /** Glyph for the non-loading states. Defaults to the Printers icon. */
  icon?: IconName;
}

/**
 * Centered loading / unavailable / empty state for a screen body. Extracted
 * from the Printers screen; the defaults reproduce its original copy so callers
 * that pass only `loading`/`error` behave identically, while Profiles/Settings/
 * About can override the titles, hint, and icon.
 */
export function CenteredNotice({
  loading,
  error,
  loadingTitle = 'Detecting printers…',
  emptyTitle = 'No printers installed',
  errorTitle = 'Printers unavailable',
  hint = 'Use "Add printer" to install one from the network or by IP address, or press Rescan.',
  icon = 'printer',
}: CenteredNoticeProps) {
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
        <Icon name={icon} size={26} color="var(--mp-textDim)" />
      )}
      <div style={{ fontSize: 13.5, fontWeight: 500 }}>
        {loading ? loadingTitle : error ? errorTitle : emptyTitle}
      </div>
      <div
        style={{
          fontSize: 12,
          color: 'var(--mp-textMuted)',
          maxWidth: 380,
          lineHeight: 1.5,
        }}
      >
        {loading ? 'Reading the system print spooler.' : (error ?? hint)}
      </div>
    </div>
  );
}
