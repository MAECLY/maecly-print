import { useState, type CSSProperties, type ReactNode } from 'react';
import { toast } from 'sonner';

import { Icon, type IconName } from '@/components/Icon';
import {
  APP_SHORTCUTS,
  shortcutLabel,
  type ShortcutId,
} from '@/components/AppShell/MPAppShell';
import {
  MPButton,
  MPCard,
  MPKbd,
  MPSectionLabel,
  FileGlyph,
  StatusDot,
  MPTopBar,
} from '@/components/ui';
import { useT } from '@/i18n/I18nProvider';
import { findProfileIn } from '@/lib/profiles';
import { useProfilesStore } from '@/stores/profilesStore';
import { useQueueStore } from '@/stores/queueStore';

/**
 * Extensions the print engine accepts today, and the single place the Home
 * screen is allowed to name a format.
 *
 * This mirrors `SUPPORTED_EXTS` in `src-tauri/src/engine/mod.rs`, which is the
 * only authority — `HomeScreen.test.tsx` reads that Rust file and fails if the
 * two ever drift, so the drop zone can no longer advertise a format the engine
 * rejects. It is exported because `App` needs the same list for the OS file
 * picker's filter; the proper long-term home is a `supported_formats` IPC
 * command (see the report), which would also fold in the copy that
 * `PrintJobScreen` keeps.
 */
export const PRINTABLE_EXTENSIONS = [
  'pdf',
  'jpg',
  'jpeg',
  'png',
  'tif',
  'tiff',
  'bmp',
  'webp',
] as const;

/** What a Home affordance asks the app to do; every one of them is wired. */
export type HomeAction = 'newjob' | 'queue' | 'files' | 'folder';

interface HomeScreenProps {
  onAction?: (action: HomeAction) => void;
}

/**
 * A card-shaped control. A real `<button>` rather than a `<div onClick>`: the
 * quick actions have to be reachable by Tab and activatable with Enter/Space
 * (NFR-041), and the focus ring comes free with `.mp-focus` (NFR-042).
 */
function CardButton({
  onClick,
  style,
  children,
  ...rest
}: {
  onClick: () => void;
  style?: CSSProperties;
  children: ReactNode;
  'aria-label'?: string;
  'aria-expanded'?: boolean;
}) {
  const [hover, setHover] = useState(false);
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      className="mp-focus"
      {...rest}
      style={{
        display: 'block',
        width: '100%',
        textAlign: 'left',
        font: 'inherit',
        color: 'inherit',
        background: 'var(--mp-surface)',
        border: '1px solid',
        borderColor: hover ? 'var(--mp-borderStrong)' : 'var(--mp-border)',
        borderRadius: 8,
        boxShadow: hover ? '0 2px 6px rgba(0,0,0,0.04)' : 'none',
        cursor: 'pointer',
        transition: 'box-shadow .15s, border-color .12s',
        ...style,
      }}
    >
      {children}
    </button>
  );
}

/** Centered notice for the recent-activity card (loading / error / empty). */
function RecentNotice({
  title,
  body,
  action,
}: {
  title: string;
  body?: string;
  action?: ReactNode;
}) {
  return (
    <div
      style={{
        padding: '18px 14px',
        textAlign: 'center',
        fontSize: 12,
        color: 'var(--mp-textMuted)',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 6,
      }}
    >
      <div>{title}</div>
      {body && <div style={{ color: 'var(--mp-textDim)' }}>{body}</div>}
      {action}
    </div>
  );
}

export function HomeScreen({ onAction }: HomeScreenProps) {
  const t = useT('home');
  // Job states are labelled by the queue namespace so Home and the Queue screen
  // can never disagree about what "converted" is called.
  const tQueue = useT('queue');
  const tCommon = useT('common');
  const recent = useQueueStore((s) => s.recentDone);
  const queueStatus = useQueueStore((s) => s.status);
  const queueError = useQueueStore((s) => s.error);
  const initQueue = useQueueStore((s) => s.init);
  const profiles = useProfilesStore((s) => s.profiles);
  const [expanded, setExpanded] = useState<string | null>(null);

  // The CLI exists (FR-008), so the card shows the command a user can really
  // run, built around the profile their jobs default to rather than a made-up
  // name.
  const defaultProfile = findProfileIn(profiles, null);
  const cliCommand = `maeclyprint print --files "*.pdf" --profile "${defaultProfile.name}"`;

  const formats = PRINTABLE_EXTENSIONS.map((e) => e.toUpperCase()).join(', ');

  async function copyCliCommand() {
    try {
      await navigator.clipboard.writeText(cliCommand);
      toast.success(t('cli.copied'));
    } catch {
      // No clipboard permission, or no clipboard at all (the standalone web
      // preview): the command is on screen, so say how to get it manually.
      toast.error(t('cli.copyFailed'));
    }
  }

  const quickActions: {
    id: string;
    icon: IconName;
    title: string;
    body: string;
    kbd?: ShortcutId;
    run: () => void;
  }[] = [
    {
      id: 'newjob',
      icon: 'plus',
      title: t('quickActions.newJob.title'),
      body: t('quickActions.newJob.body'),
      kbd: 'newJob',
      run: () => onAction?.('newjob'),
    },
    {
      id: 'folder',
      icon: 'folder',
      title: t('quickActions.folder.title'),
      body: t('quickActions.folder.body'),
      kbd: 'printFolder',
      run: () => onAction?.('folder'),
    },
    {
      id: 'cli',
      icon: 'cmd',
      title: t('quickActions.cli.title'),
      body: t('quickActions.cli.body'),
      run: () => void copyCliCommand(),
    },
  ];

  let recentBody: ReactNode;
  if (queueStatus === 'unavailable') {
    recentBody = <RecentNotice title={t('recent.unavailable')} />;
  } else if (queueStatus === 'error') {
    // A failed read is not an empty history: saying "no recent activity yet"
    // here would hide a backend failure behind a reassuring sentence (NFR-033).
    recentBody = (
      <RecentNotice
        title={t('recent.errorTitle')}
        body={queueError ?? t('recent.errorHint')}
        action={
          <MPButton
            variant="ghost"
            icon="rotate"
            size="sm"
            onClick={() => void initQueue()}
          >
            {tCommon('actions.retry')}
          </MPButton>
        }
      />
    );
  } else if (queueStatus === 'idle' || queueStatus === 'loading') {
    recentBody = <RecentNotice title={t('recent.loading')} />;
  } else if (recent.length === 0) {
    recentBody = <RecentNotice title={t('recent.empty')} />;
  } else {
    recentBody = recent.map((j, i, arr) => {
      const open = expanded === j.id;
      const border =
        i < arr.length - 1 || open ? '1px solid var(--mp-border)' : 'none';
      return (
        <div key={j.id}>
          <CardButton
            aria-expanded={open}
            aria-label={
              open
                ? t('recent.collapse', { file: j.file })
                : t('recent.expand', { file: j.file })
            }
            onClick={() => setExpanded(open ? null : j.id)}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              padding: '10px 14px',
              background: 'transparent',
              border: 'none',
              borderBottom: border,
              borderRadius: 0,
              boxShadow: 'none',
            }}
          >
            <FileGlyph ext={j.ext} size={22} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div
                style={{
                  fontSize: 12.5,
                  fontWeight: 500,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {j.file}
              </div>
              <div
                className="mp-mono"
                style={{
                  fontSize: 10.5,
                  color: 'var(--mp-textDim)',
                  marginTop: 1,
                }}
              >
                {j.printer} · {j.profile}
              </div>
            </div>
            <StatusDot status={j.status} />
            <span
              style={{
                fontSize: 10.5,
                color: 'var(--mp-textMuted)',
                textTransform: 'uppercase',
                minWidth: 60,
                textAlign: 'right',
              }}
            >
              {/* Every `JobState` has a `status.<state>` entry. */}
              {tQueue(`status.${j.status}`)}
            </span>
          </CardButton>
          {open && (
            <div
              style={{
                padding: '10px 14px 12px 46px',
                borderBottom: i < arr.length - 1 ? border : 'none',
                display: 'flex',
                flexDirection: 'column',
                gap: 4,
                fontSize: 11.5,
                color: 'var(--mp-textMuted)',
              }}
            >
              <div>
                {t('recent.printer')}: {j.printer}
              </div>
              <div>
                {t('recent.profile')}: {j.profile}
              </div>
              <div>
                {t('recent.pages')}: {j.pages}
              </div>
              {j.finishedAt && (
                <div>
                  {t('recent.finished')}: {j.finishedAt}
                </div>
              )}
              {j.error && (
                <div style={{ color: 'var(--mp-error)' }}>
                  {t('recent.error')}: {j.error}
                </div>
              )}
              <div style={{ marginTop: 4 }}>
                <MPButton
                  variant="ghost"
                  icon="queue"
                  size="sm"
                  onClick={() => onAction?.('queue')}
                >
                  {t('recent.openQueue')}
                </MPButton>
              </div>
            </div>
          )}
        </div>
      );
    });
  }

  return (
    <>
      <MPTopBar
        title={t('title')}
        subtitle={t('subtitle', { shortcut: shortcutLabel('newJob') })}
        right={
          <MPButton
            variant="primary"
            icon="plus"
            onClick={() => onAction?.('newjob')}
          >
            {t('quickActions.newJob.title')}
          </MPButton>
        }
      />
      <div
        className="mp-scroll"
        style={{ flex: 1, overflow: 'auto', padding: 20 }}
      >
        {/* Dropping works from any screen (App owns the native listener); the
            zone is also a button so the same job can be started without a
            mouse and without a file manager. */}
        <CardButton
          onClick={() => onAction?.('files')}
          aria-label={t('dropzone.action')}
          style={{
            border: '1.5px dashed var(--mp-borderStrong)',
            borderRadius: 10,
            padding: '28px 20px',
            textAlign: 'center',
            marginBottom: 20,
          }}
        >
          <div
            style={{
              display: 'inline-flex',
              padding: 10,
              background: 'var(--mp-surface2)',
              borderRadius: 10,
              marginBottom: 10,
            }}
          >
            <Icon name="upload" size={20} color="var(--mp-textMuted)" />
          </div>
          <div style={{ fontSize: 14, fontWeight: 500, marginBottom: 4 }}>
            {t('dropzone.title')}
          </div>
          <div style={{ fontSize: 12, color: 'var(--mp-textMuted)' }}>
            {t('dropzone.formats', { formats })}
          </div>
          <div
            style={{ fontSize: 11.5, color: 'var(--mp-textDim)', marginTop: 4 }}
          >
            {t('dropzone.folders')}
          </div>
        </CardButton>

        <MPSectionLabel>{t('quickActions.label')}</MPSectionLabel>
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: '1fr 1fr 1fr',
            gap: 10,
            marginBottom: 24,
          }}
        >
          {quickActions.map((q) => (
            <CardButton key={q.id} onClick={q.run} style={{ padding: 14 }}>
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  marginBottom: 8,
                  minHeight: 18,
                }}
              >
                <Icon name={q.icon} size={16} color="var(--mp-accent)" />
                {q.kbd && <MPKbd>{shortcutLabel(q.kbd)}</MPKbd>}
              </div>
              <div style={{ fontSize: 13, fontWeight: 500 }}>{q.title}</div>
              <div
                style={{
                  fontSize: 11.5,
                  color: 'var(--mp-textMuted)',
                  marginTop: 2,
                }}
              >
                {q.body}
              </div>
              {q.id === 'cli' && (
                <div
                  className="mp-mono"
                  style={{
                    fontSize: 10.5,
                    color: 'var(--mp-textDim)',
                    marginTop: 6,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {cliCommand}
                </div>
              )}
            </CardButton>
          ))}
        </div>

        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            marginBottom: 8,
          }}
        >
          <MPSectionLabel>{t('recent.label')}</MPSectionLabel>
          <button
            onClick={() => onAction?.('queue')}
            className="mp-focus"
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: 11.5,
              color: 'var(--mp-textMuted)',
              display: 'inline-flex',
              alignItems: 'center',
              gap: 3,
            }}
          >
            {t('recent.viewAll')}
            <Icon name="chevr" size={11} />
          </button>
        </div>
        <MPCard padding={0}>{recentBody}</MPCard>

        {/* NFR-034 asks for the shortcuts to be documented, not just to work. */}
        <div style={{ marginTop: 24 }}>
          <MPSectionLabel>{t('shortcuts.label')}</MPSectionLabel>
        </div>
        <MPCard padding={14}>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: '6px 16px',
            }}
          >
            {APP_SHORTCUTS.map((s) => (
              <div
                key={s.id}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  gap: 10,
                  fontSize: 12,
                }}
              >
                <span>{t(`shortcuts.${s.id}`)}</span>
                <MPKbd>{shortcutLabel(s.id)}</MPKbd>
              </div>
            ))}
          </div>
          <div
            style={{
              fontSize: 11.5,
              color: 'var(--mp-textDim)',
              marginTop: 10,
            }}
          >
            {t('shortcuts.hint')}
          </div>
        </MPCard>

        <div
          style={{
            marginTop: 20,
            padding: '10px 12px',
            background: 'var(--mp-infoSoft)',
            borderRadius: 6,
            display: 'flex',
            gap: 8,
            alignItems: 'flex-start',
          }}
        >
          <Icon name="info" size={14} color="var(--mp-info)" />
          <div
            style={{ fontSize: 12, color: 'var(--mp-info)', lineHeight: 1.5 }}
          >
            <strong>{t('tip.label')} · </strong>
            {t('tip.body', { command: 'maeclyprint print --files' })}
          </div>
        </div>
      </div>
    </>
  );
}
