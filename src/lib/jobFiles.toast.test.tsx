/**
 * The folder-walk progress toast against the REAL sonner store and Toaster:
 * the unit tests in `jobFiles.test.ts` mock sonner, so they cannot see the
 * ordering race between sonner's synchronous dismiss and its deferred add.
 */

import { act, render } from '@testing-library/react';
import { Toaster, toast } from 'sonner';
import { afterEach, describe, expect, it, vi } from 'vitest';

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

describe('folder expansion progress toast (real sonner)', () => {
  afterEach(() => {
    toast.dismiss();
  });

  it.each([
    ['with new files', ['/docs/flat/a.pdf']],
    ['with nothing to add', []],
  ])(
    'FR-004: a walk that settles at once leaves no spinner behind (%s)',
    async (_label, files) => {
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

      // Let sonner add the toast and apply whatever followed it.
      await act(async () => {
        await new Promise((r) => setTimeout(r, 50));
      });
      // Gone, or leaving (sonner keeps a dismissed toast for its exit
      // animation and flags it `data-removed`) — never a live spinner.
      expect(
        document.querySelector(
          '[data-sonner-toast][data-type="loading"]:not([data-removed="true"])',
        ),
      ).toBeNull();
    },
  );
});
