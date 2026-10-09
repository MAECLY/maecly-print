import { useEffect, useRef, type ReactNode } from 'react';

/**
 * Root application shell, and the home of the app-wide keyboard layer.
 *
 * The window uses native OS decorations (`decorations: true` in
 * tauri.conf.json), so the title bar and min/maximize/close controls are drawn
 * by macOS and Windows themselves — correct per platform, no custom chrome.
 * This component is just the full-height content surface below that native bar.
 * App branding lives in the sidebar (see `MPSidebar`).
 *
 * The shortcut registry lives here rather than in a screen because it has three
 * consumers that must never disagree: `App` (which owns the actions), the
 * sidebar (which prints a key cap next to two nav items) and the Home screen
 * (which documents the whole set, NFR-034). One table, one label formatter, one
 * matcher.
 */

// ---------------------------------------------------------------------------
// Keyboard shortcuts (NFR-034)
// ---------------------------------------------------------------------------

/**
 * The primary actions NFR-034 requires a shortcut for — print, cancel, switch
 * profile, open queue — plus the two entry points into a job that the Home
 * screen advertises. Nothing else: a key cap the app does not honour is worse
 * than no key cap at all.
 */
export type ShortcutId =
  | 'newJob'
  | 'printFolder'
  | 'print'
  | 'switchProfile'
  | 'openQueue'
  | 'cancel';

export interface Shortcut {
  id: ShortcutId;
  /** `KeyboardEvent.key`, lowercased. Always combined with the platform mod. */
  key: string;
}

/**
 * The whole shortcut table. `key` is matched against `KeyboardEvent.key`, which
 * is layout-dependent by design: a French keyboard prints what the user's own
 * key cap says, and `code` would fire on the physically-equivalent key of a
 * different letter.
 *
 * `.` for cancel follows the macOS convention (⌘.) and is unclaimed on Windows;
 * `j` opens the queue because ⌘Q quits on macOS and Ctrl+Q is a habit on
 * Windows.
 */
export const APP_SHORTCUTS: readonly Shortcut[] = [
  { id: 'newJob', key: 'n' },
  { id: 'printFolder', key: 'o' },
  { id: 'print', key: 'p' },
  { id: 'switchProfile', key: 'k' },
  { id: 'openQueue', key: 'j' },
  { id: 'cancel', key: '.' },
];

/** Whether the modifier to render and listen for is ⌘ rather than Ctrl. */
export function isMacPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  const withData = navigator as Navigator & {
    userAgentData?: { platform?: string };
  };
  const platform =
    withData.userAgentData?.platform ??
    navigator.platform ??
    navigator.userAgent;
  return /mac|iphone|ipad/i.test(platform);
}

/** How a shortcut is written on a key cap: `⌘N` on macOS, `Ctrl+N` elsewhere. */
export function shortcutLabel(id: ShortcutId): string {
  const shortcut = APP_SHORTCUTS.find((s) => s.id === id);
  if (!shortcut) return '';
  const key = shortcut.key.toUpperCase();
  return isMacPlatform() ? `⌘${key}` : `Ctrl+${key}`;
}

/**
 * Whether the event started inside something the user is typing into. Without
 * this guard a global Ctrl/⌘+N would fire while renaming a profile, and every
 * text field in the app would silently lose keys to the shell.
 */
function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
}

/**
 * The shortcut a keydown stands for, or `undefined` when it is not one of ours.
 *
 * Exactly one modifier is accepted (⌘ on macOS, Ctrl elsewhere) and the other
 * must be up, so Ctrl+⌘+N on a Mac stays available to the OS. Auto-repeat is
 * ignored: holding Ctrl+P must queue one print, not one per repeat tick.
 */
export function matchShortcut(e: KeyboardEvent): ShortcutId | undefined {
  if (e.altKey || e.shiftKey || e.repeat) return undefined;
  const mac = isMacPlatform();
  const wanted = mac ? e.metaKey : e.ctrlKey;
  const forbidden = mac ? e.ctrlKey : e.metaKey;
  if (!wanted || forbidden) return undefined;
  if (isTypingTarget(e.target)) return undefined;
  const key = e.key.toLowerCase();
  return APP_SHORTCUTS.find((s) => s.key === key)?.id;
}

/**
 * Bind the shortcut table to `handlers` for as long as the component lives.
 *
 * Handlers are read through a ref, so a caller may pass fresh closures on every
 * render (they always do — they close over the current job) without the
 * listener being torn down and re-registered each time. A matched shortcut is
 * always `preventDefault`ed: Ctrl/⌘+P would otherwise open the webview's own
 * print dialog on top of ours.
 *
 * `enabled` exists for the modal states that must own the keyboard — the
 * onboarding overlay and the confirmation dialog — where a global Ctrl+P
 * printing behind the modal would be a genuine surprise.
 */
export function useAppShortcuts(
  handlers: Partial<Record<ShortcutId, () => void>>,
  enabled = true,
): void {
  const latest = useRef(handlers);
  latest.current = handlers;
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      // The webview's own Ctrl/⌘+P (WebView2 keeps browser accelerators on)
      // would print the app's UI. It is swallowed *always* — while typing in
      // a field and while a modal owns the keyboard too — even when our own
      // print shortcut stands down.
      if (isPrintChord(e)) e.preventDefault();
      if (!enabledRef.current) return;
      const id = matchShortcut(e);
      if (!id) return;
      const run = latest.current[id];
      if (!run) return;
      e.preventDefault();
      run();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);
}

/** Platform-mod + P, with or without other keys held, repeats included. */
function isPrintChord(e: KeyboardEvent): boolean {
  const mod = isMacPlatform() ? e.metaKey : e.ctrlKey;
  return mod && e.key.toLowerCase() === 'p';
}

// ---------------------------------------------------------------------------
// Shell
// ---------------------------------------------------------------------------

interface MPAppShellProps {
  children: ReactNode;
}

export function MPAppShell({ children }: MPAppShellProps) {
  return (
    <div
      className="mp-app"
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--mp-bg)',
        color: 'var(--mp-text)',
        overflow: 'hidden',
      }}
    >
      {children}
    </div>
  );
}
