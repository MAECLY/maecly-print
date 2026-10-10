import { Icon, type IconName } from '@/components/Icon';

interface CenteredNoticeProps {
  /** Show the spinner + loading copy. Takes precedence over `error`. */
  loading: boolean;
  /** When set (and not loading), render the error copy with this message. */
  error: string | null;
  /** Title while loading. */
  loadingTitle: string;
  /** Body while loading (what is being read). */
  loadingBody: string;
  /** Title for the error state. */
  errorTitle: string;
  /** Title for the empty (no data, no error) state; omit when the screen has
   *  no empty state. */
  emptyTitle?: string | undefined;
  /** Body for the empty state. */
  hint?: string | undefined;
  /** Glyph for the non-loading states. Defaults to the Printers icon. */
  icon?: IconName;
}

/**
 * Centered loading / unavailable / empty state for a screen body. Every string
 * comes from the caller's catalog (FR-092) — there is no built-in copy, so a
 * screen can never show another screen's text or an untranslated default.
 *
 * The region is a polite live region: a screen-reader user hears the loading
 * line and then the error (or empty) line that replaces it (NFR-044).
 */
export function CenteredNotice({
  loading,
  error,
  loadingTitle,
  loadingBody,
  emptyTitle,
  errorTitle,
  hint,
  icon = 'printer',
}: CenteredNoticeProps) {
  return (
    <div
      role="status"
      aria-live="polite"
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
          aria-hidden="true"
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
        {loading ? loadingBody : (error ?? hint)}
      </div>
    </div>
  );
}
