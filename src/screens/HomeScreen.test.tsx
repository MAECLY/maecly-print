/**
 * Home screen tests.
 *
 * Hermetic: the queue and profile stores are driven directly (no IPC host, no
 * user config) and `sonner` is mocked, so nothing here touches the network, the
 * filesystem of the user, or a real backend. The one file read is the engine's
 * own source inside this repository — the regression that keeps the advertised
 * format list from drifting away from what the engine accepts.
 */

import { render, screen, fireEvent, within } from '@testing-library/react';
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const toasts = vi.hoisted(() => ({
  error: vi.fn(),
  warning: vi.fn(),
  success: vi.fn(),
  info: vi.fn(),
}));
vi.mock('sonner', () => ({ toast: toasts }));

import { HomeScreen, PRINTABLE_EXTENSIONS } from './HomeScreen';
import { shortcutLabel, APP_SHORTCUTS } from '@/components/AppShell/MPAppShell';
import { BUILT_IN_PROFILES } from '@/lib/profiles';
import { toUiQueueJob } from '@/lib/ipc';
import { mkJob } from '@/test/fixtures';
import { useProfilesStore } from '@/stores/profilesStore';
import { useQueueStore } from '@/stores/queueStore';
import type { UiQueueJobView } from '@/lib/ipc';

const REPO_ROOT = process.cwd();

const doneJob = toUiQueueJob(
  mkJob({
    id: 'j-done',
    state: 'done',
    file_name: 'Site-Photo-North.jpg',
    profile_name: 'Photo Color',
    printer_resolved: 'EPSON L15160',
    finished_at: '2026-07-11T10:39:31Z',
  }),
);
const failedJob = toUiQueueJob(
  mkJob({
    id: 'j-failed',
    state: 'failed',
    file_name: 'Report-Broken.pdf',
    error: { code: 'print_failed', message: 'Paper jam in Tray 2' },
    finished_at: '2026-07-11T10:38:55Z',
  }),
);

/** Put the queue store in a known state; `ready` unless told otherwise. */
function setQueue(
  recentDone: UiQueueJobView[],
  over: Partial<{
    status: 'idle' | 'loading' | 'ready' | 'unavailable' | 'error';
    error: string;
  }> = {},
) {
  useQueueStore.setState({
    status: over.status ?? 'ready',
    error: over.error,
    recentDone,
  });
}

function renderHome(onAction = vi.fn()) {
  render(<HomeScreen onAction={onAction} />);
  return onAction;
}

describe('HomeScreen', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useProfilesStore.setState({
      status: 'ready',
      profiles: BUILT_IN_PROFILES,
      error: undefined,
    });
    setQueue([]);
  });

  // -------------------------------------------------------------------------
  // Formats advertised by the drop zone
  // -------------------------------------------------------------------------

  it('FR-005: the drop zone advertises exactly the formats the engine accepts', () => {
    renderHome();
    const zone = screen.getByRole('button', { name: /click to choose them/i });
    for (const ext of PRINTABLE_EXTENSIONS) {
      expect(zone).toHaveTextContent(ext.toUpperCase());
    }
    // The formats the engine rejects must not be promised anywhere on screen.
    for (const absent of [/office/i, /odf/i, /html/i, /markdown/i]) {
      expect(document.body.textContent ?? '').not.toMatch(absent);
    }
  });

  it('regression: the advertised list matches SUPPORTED_EXTS in the Rust engine', () => {
    const source = readFileSync(
      join(REPO_ROOT, 'src-tauri/src/engine/mod.rs'),
      'utf8',
    );
    const decl = /pub const SUPPORTED_EXTS: &\[&str\] = &\[([^\]]*)\]/.exec(
      source,
    );
    expect(decl, 'SUPPORTED_EXTS not found in the engine').not.toBeNull();
    const fromRust = Array.from(decl![1]!.matchAll(/"([a-z0-9]+)"/g)).map(
      (m) => m[1]!,
    );
    expect(fromRust.length).toBeGreaterThan(0);
    expect([...PRINTABLE_EXTENSIONS]).toEqual(fromRust);
  });

  // -------------------------------------------------------------------------
  // Quick actions (each one its own destination)
  // -------------------------------------------------------------------------

  it('FR-004: every quick action triggers its own action, not the same one', () => {
    const onAction = renderHome();
    fireEvent.click(screen.getByRole('button', { name: /^new print job$/i }));
    expect(onAction).toHaveBeenLastCalledWith('newjob');

    fireEvent.click(screen.getByRole('button', { name: /print a folder/i }));
    expect(onAction).toHaveBeenLastCalledWith('folder');

    fireEvent.click(
      screen.getByRole('button', { name: /click to choose them/i }),
    );
    expect(onAction).toHaveBeenLastCalledWith('files');

    fireEvent.click(screen.getByRole('button', { name: /view all queue/i }));
    expect(onAction).toHaveBeenLastCalledWith('queue');
    expect(onAction.mock.calls.map((c) => c[0])).toEqual([
      'newjob',
      'folder',
      'files',
      'queue',
    ]);
  });

  it('NFR-041: the quick actions are real buttons, operable with Enter alone', () => {
    const onAction = renderHome();
    const folder = screen.getByRole('button', { name: /print a folder/i });
    folder.focus();
    expect(folder).toHaveFocus();
    // A <button> fires click on Enter/Space natively; the regression this
    // guards is the old `<div onClick>`, which never reaches this assertion.
    fireEvent.click(document.activeElement!);
    expect(onAction).toHaveBeenCalledWith('folder');
  });

  // -------------------------------------------------------------------------
  // CLI (FR-008) — the command exists, so the card hands it over
  // -------------------------------------------------------------------------

  it('FR-008: the CLI card shows the real command and copies it', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    renderHome();
    const card = screen.getByRole('button', { name: /run from cli/i });
    expect(card).toHaveTextContent('maeclyprint print --files');
    // Built around the profile the app really defaults to.
    expect(card).toHaveTextContent('--profile "Quick"');

    fireEvent.click(card);
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    expect(writeText.mock.calls[0]![0]).toMatch(
      /^maeclyprint print --files ".+" --profile "Quick"$/,
    );
    expect(toasts.success).toHaveBeenCalled();
  });

  it('FR-008: a clipboard refusal is reported instead of silently doing nothing', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: vi.fn().mockRejectedValue(new Error('denied')),
      },
    });
    renderHome();
    fireEvent.click(screen.getByRole('button', { name: /run from cli/i }));
    await vi.waitFor(() => expect(toasts.error).toHaveBeenCalledTimes(1));
  });

  // -------------------------------------------------------------------------
  // Recent activity
  // -------------------------------------------------------------------------

  it('renders the recent jobs from the queue store, newest first', () => {
    setQueue([doneJob, failedJob]);
    renderHome();
    expect(screen.getByText('Site-Photo-North.jpg')).toBeInTheDocument();
    expect(screen.getByText('Report-Broken.pdf')).toBeInTheDocument();
    // Status is rendered with the shared queue labels, not the raw state.
    expect(screen.getByText('Done')).toBeInTheDocument();
    expect(screen.getByText('Failed')).toBeInTheDocument();
  });

  it('shows the empty state when the queue is ready and has no history', () => {
    renderHome();
    expect(screen.getByText(/no recent activity yet/i)).toBeInTheDocument();
  });

  it('uses the id of the clicked job to reveal its details (and can hide them)', () => {
    setQueue([doneJob, failedJob]);
    renderHome();
    const row = screen.getByRole('button', {
      name: /show the details of Report-Broken\.pdf/i,
    });
    expect(row).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(row);
    expect(row).toHaveAttribute('aria-expanded', 'true');
    // The detail belongs to the clicked job, not to the first row.
    expect(screen.getByText(/Paper jam in Tray 2/)).toBeInTheDocument();
    // The other row stays collapsed.
    expect(
      screen.getByRole('button', {
        name: /show the details of Site-Photo-North\.jpg/i,
      }),
    ).toHaveAttribute('aria-expanded', 'false');

    fireEvent.click(
      screen.getByRole('button', {
        name: /hide the details of Report-Broken\.pdf/i,
      }),
    );
    expect(screen.queryByText(/Paper jam in Tray 2/)).not.toBeInTheDocument();
  });

  it('offers a way into the queue from the expanded job', () => {
    setQueue([doneJob]);
    const onAction = renderHome();
    fireEvent.click(
      screen.getByRole('button', { name: /show the details of/i }),
    );
    fireEvent.click(screen.getByRole('button', { name: /open the queue/i }));
    expect(onAction).toHaveBeenCalledWith('queue');
  });

  // -------------------------------------------------------------------------
  // Queue failures are not disguised as an empty history (NFR-033)
  // -------------------------------------------------------------------------

  it('NFR-033: a queue read failure is reported with its message and a retry', () => {
    const init = vi.fn().mockResolvedValue(undefined);
    setQueue([], { status: 'error', error: 'queue.json is corrupt' });
    useQueueStore.setState({ init });
    renderHome();
    expect(
      screen.queryByText(/no recent activity yet/i),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/could not be read/i)).toBeInTheDocument();
    expect(screen.getByText('queue.json is corrupt')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /retry/i }));
    expect(init).toHaveBeenCalledTimes(1);
  });

  it('NFR-033: an unavailable backend says so instead of showing an empty history', () => {
    setQueue([], { status: 'unavailable' });
    renderHome();
    expect(
      screen.queryByText(/no recent activity yet/i),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(/run MaeclyPrint to see them/i),
    ).toBeInTheDocument();
  });

  it('distinguishes "still loading" from "nothing here"', () => {
    setQueue([], { status: 'loading' });
    renderHome();
    expect(screen.getByText(/loading recent activity/i)).toBeInTheDocument();
    expect(
      screen.queryByText(/no recent activity yet/i),
    ).not.toBeInTheDocument();
  });

  // -------------------------------------------------------------------------
  // Shortcuts surface (NFR-034)
  // -------------------------------------------------------------------------

  it('NFR-034: documents every shortcut the app implements, with its key cap', () => {
    renderHome();
    const section = screen.getByText(/keyboard shortcuts/i).parentElement!;
    const caps = within(section.parentElement!)
      .getAllByText(/^(Ctrl\+|⌘)/)
      .map((el) => el.textContent);
    for (const shortcut of APP_SHORTCUTS) {
      expect(caps).toContain(shortcutLabel(shortcut.id));
    }
  });

  it('NFR-034: every key cap on screen belongs to a real shortcut', () => {
    renderHome();
    const real = new Set(APP_SHORTCUTS.map((s) => shortcutLabel(s.id)));
    for (const cap of document.querySelectorAll('kbd')) {
      expect(real).toContain(cap.textContent);
    }
  });

  // -------------------------------------------------------------------------
  // Copy honesty
  // -------------------------------------------------------------------------

  it('does not claim the shell right-click integration already exists', () => {
    renderHome();
    const text = document.body.textContent ?? '';
    expect(text).not.toMatch(/adds itself to the right-click menu/i);
    // The CLI, which does exist, is what the tip points at.
    expect(text).toMatch(/maeclyprint print --files/);
  });
});
