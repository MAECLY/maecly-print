/**
 * The folder-walk progress toast against the REAL sonner store and Toaster:
 * the unit tests in `jobFiles.test.ts` mock sonner, so they cannot see the
 * ordering race between sonner's synchronous dismiss and its deferred add.
 */

import { act, render, waitFor } from '@testing-library/react';
import { Toaster, toast } from 'sonner';
import { describe, expect, it, vi } from 'vitest';

import type { FileInfo, FolderExpansion } from '@/lib/ipc';

const ipc = vi.hoisted(() => ({
  inspectFiles: vi.fn<(paths: string[]) => Promise<FileInfo[]>>(),
  expandFolders: vi.fn<() => Promise<FolderExpansion[]>>(),
}));

vi.mock('@/lib/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/ipc')>();
  return {
    ...actual,
    inspectFiles: ipc.inspectFiles,
    expandFolders: ipc.expandFolders,
  };
});

import { expandFolderIntoJob } from './jobFiles';

function expansion(files: string[]): FolderExpansion {
  return {
    root: '/docs/flat',
    scan: 'top_level',
    files,
    skipped: [],
    files_truncated: false,
    depth_truncated: false,
    limits: { max_files: 5000, max_depth: 16 },
  };
}

function fileInfo(path: string): FileInfo {
  const name = path.split('/').pop() ?? path;
  return {
    path,
    name,
    ext: 'pdf',
    bytes: 2048,
    pages: 1,
    supported: true,
    reason: null,
    reason_code: null,
    is_dir: false,
    has_subfolders: false,
  } as unknown as FileInfo;
}

/**
 * A spinner that is on screen and not on its way out. sonner keeps a dismissed
 * toast in the DOM for its exit animation and flags it `data-removed`.
 */
const LIVE_SPINNER =
  '[data-sonner-toast][data-type="loading"]:not([data-removed="true"])';

describe('folder expansion progress toast (real sonner)', () => {
  // `src/test/setup.ts` retires every toast after each test; this file relies
  // on that rather than repeating it.
  it.each([
    // The progress toast is replaced, by id, with the "added" toast.
    ['with new files', ['/docs/flat/a.pdf'], 'success'],
    // The progress toast is dismissed and the walk reports an empty folder.
    ['with nothing to add', [], 'info'],
  ])(
    'FR-004: a walk that settles at once leaves no spinner behind (%s)',
    async (_label, files, outcome) => {
      // sonner 2 replays every still-active toast into a Toaster when it
      // mounts, so start from an empty store: a toast an earlier test left
      // behind must not leak into this one.
      expect(toast.getToasts()).toEqual([]);
      ipc.expandFolders.mockResolvedValue([expansion(files)]);
      ipc.inspectFiles.mockImplementation(async (paths) => paths.map(fileInfo));
      render(<Toaster />);

      await act(async () => {
        await expandFolderIntoJob(
          { path: '/docs/flat', name: 'flat', hasSubfolders: false },
          'top_level',
          () => undefined,
        );
      });

      // Wait for sonner to settle; do not sleep for a fixed time. sonner adds
      // a toast on a zero-delay timer, but a dismissal crosses two animation
      // frames: the store publishes it on one and the Toaster applies it on
      // the next. jsdom fakes frames with a 16 ms interval, so the removal
      // lands about 33 ms later on an idle machine and much later on a loaded
      // CI runner. The outcome toast anchors the wait. It is queued after the
      // spinner was added, so "no live spinner" cannot pass merely because
      // sonner has not rendered anything yet.
      await waitFor(() => {
        expect(
          document.querySelector(`[data-sonner-toast][data-type="${outcome}"]`),
        ).not.toBeNull();
        expect(document.querySelector(LIVE_SPINNER)).toBeNull();
      });
    },
  );
});
