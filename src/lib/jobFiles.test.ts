/**
 * Unit tests for the "add files to the job" pipeline. The IPC layer is mocked
 * at `@/lib/ipc` (not at the Tauri transport) so these stay hermetic: no host,
 * no config, no real filesystem.
 */

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';

import type { FileInfo, FolderExpansion } from '@/lib/ipc';
import type { UiFile } from '@/types/domain';

const ipc = vi.hoisted(() => ({
  inspectFiles: vi.fn<(paths: string[]) => Promise<FileInfo[]>>(),
  expandFolders: vi.fn<() => Promise<FolderExpansion[]>>(),
}));
const toasts = vi.hoisted(() => ({
  error: vi.fn(),
  warning: vi.fn(),
  success: vi.fn(),
  info: vi.fn(),
}));

vi.mock('@/lib/ipc', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/ipc')>();
  return {
    ...actual,
    inspectFiles: ipc.inspectFiles,
    expandFolders: ipc.expandFolders,
  };
});
vi.mock('sonner', () => ({ toast: toasts }));

import {
  addPathsToJob,
  clearPendingFolders,
  expandFolderIntoJob,
  getPendingFolders,
  inspectPaths,
  resolvePendingFolder,
  subscribePendingFolders,
} from './jobFiles';

function fileInfo(path: string, over: Partial<FileInfo> = {}): FileInfo {
  const name = path.split('/').pop() ?? path;
  return {
    path,
    name,
    ext: (name.split('.').pop() ?? '').toLowerCase(),
    bytes: 2048,
    pages: 3,
    supported: true,
    reason: null,
    is_dir: false,
    has_subfolders: false,
    ...over,
  };
}

function folderInfo(path: string, hasSubfolders: boolean): FileInfo {
  return fileInfo(path, {
    ext: '',
    bytes: 0,
    pages: null,
    supported: false,
    reason: 'Folder — expand it to print the files inside',
    is_dir: true,
    has_subfolders: hasSubfolders,
  });
}

function expansion(over: Partial<FolderExpansion> = {}): FolderExpansion {
  return {
    root: '/docs',
    scan: 'top_level',
    files: [],
    skipped: [],
    files_truncated: false,
    depth_truncated: false,
    limits: { max_files: 5000, max_depth: 16 },
    ...over,
  };
}

/** A `setFiles` that behaves like React's, exposing the accumulated list. */
function collector() {
  let list: UiFile[] = [];
  const setFiles = (update: UiFile[] | ((cur: UiFile[]) => UiFile[])) => {
    list = typeof update === 'function' ? update(list) : update;
  };
  return { setFiles, get: () => list };
}

describe('jobFiles', () => {
  beforeEach(() => {
    clearPendingFolders();
    ipc.inspectFiles.mockReset();
    ipc.expandFolders.mockReset();
    for (const fn of Object.values(toasts)) fn.mockReset();
  });
  afterEach(() => clearPendingFolders());

  // -- inspectPaths ------------------------------------------------------

  it('FR-005: splits an inspection into files, folders and refusals', async () => {
    ipc.inspectFiles.mockResolvedValue([
      fileInfo('/docs/a.pdf'),
      folderInfo('/docs/reports', true),
      fileInfo('/docs/notes.txt', {
        supported: false,
        reason: 'Unsupported file type',
      }),
    ]);

    const out = await inspectPaths(['/docs/a.pdf', '/docs/reports', '/x.txt']);

    expect(out.files.map((f) => f.path)).toEqual(['/docs/a.pdf']);
    expect(out.folders).toEqual([
      { path: '/docs/reports', name: 'reports', hasSubfolders: true },
    ]);
    expect(out.rejected).toEqual([
      { name: 'notes.txt', reason: 'Unsupported file type' },
    ]);
  });

  it('strips the extension from the display name and rounds the size', async () => {
    ipc.inspectFiles.mockResolvedValue([
      fileInfo('/docs/report.PDF', {
        name: 'report.PDF',
        ext: 'pdf',
        bytes: 3072,
      }),
    ]);
    const [file] = (await inspectPaths(['/docs/report.PDF'])).files;
    expect(file).toMatchObject({ name: 'report', ext: 'pdf', kb: 3 });
  });

  it('never calls the backend for an empty selection', async () => {
    await inspectPaths([]);
    expect(ipc.inspectFiles).not.toHaveBeenCalled();
  });

  // -- addPathsToJob -----------------------------------------------------

  it('appends printable files and dedupes against what is already there', async () => {
    ipc.inspectFiles.mockResolvedValue([
      fileInfo('/docs/a.pdf'),
      fileInfo('/docs/b.pdf'),
    ]);
    const files = collector();
    files.setFiles([
      { id: '/docs/a.pdf', name: 'a', ext: 'pdf', kb: 2, path: '/docs/a.pdf' },
    ]);

    await addPathsToJob(['/docs/a.pdf', '/docs/b.pdf'], files.setFiles);

    expect(files.get().map((f) => f.path)).toEqual([
      '/docs/a.pdf',
      '/docs/b.pdf',
    ]);
  });

  it('FR-005: lists every refusal by name and reason in one toast', async () => {
    ipc.inspectFiles.mockResolvedValue([
      fileInfo('/a.txt', { supported: false, reason: 'Unsupported file type' }),
      fileInfo('/b.heic', {
        supported: false,
        reason: 'HEIC is not supported yet — convert it to JPEG or PNG first',
      }),
    ]);
    const files = collector();

    await addPathsToJob(['/a.txt', '/b.heic'], files.setFiles);

    expect(toasts.error).toHaveBeenCalledTimes(1);
    const [title, opts] = toasts.error.mock.calls[0] as [
      string,
      { description: string },
    ];
    expect(title).toContain('2');
    expect(opts.description).toContain('a.txt — Unsupported file type');
    expect(opts.description).toContain('HEIC is not supported yet');
  });

  it('surfaces an inspection failure as a toast instead of throwing', async () => {
    ipc.inspectFiles.mockRejectedValue(new Error('IPC exploded'));
    const files = collector();

    // Nothing reached the job, and the caller is told so rather than having
    // to catch: a context-menu "print now" must not spool an empty batch.
    await expect(addPathsToJob(['/a.pdf'], files.setFiles)).resolves.toEqual(
      [],
    );

    expect(toasts.error).toHaveBeenCalledWith('IPC exploded');
    expect(files.get()).toEqual([]);
  });

  // -- folders (FR-004) --------------------------------------------------

  it('FR-004: parks a folder with subfolders instead of guessing', async () => {
    ipc.inspectFiles.mockResolvedValue([folderInfo('/docs/reports', true)]);
    const files = collector();

    await addPathsToJob(['/docs/reports'], files.setFiles);

    expect(ipc.expandFolders).not.toHaveBeenCalled();
    expect(getPendingFolders()).toEqual([
      { path: '/docs/reports', name: 'reports', hasSubfolders: true },
    ]);
  });

  it('FR-004: expands a folder with no subfolders without asking', async () => {
    ipc.inspectFiles.mockImplementation(async (paths) =>
      paths[0] === '/docs/flat'
        ? [folderInfo('/docs/flat', false)]
        : paths.map((p) => fileInfo(p)),
    );
    ipc.expandFolders.mockResolvedValue([
      expansion({ root: '/docs/flat', files: ['/docs/flat/a.pdf'] }),
    ]);
    const files = collector();

    await addPathsToJob(['/docs/flat'], files.setFiles);

    expect(getPendingFolders()).toEqual([]);
    expect(ipc.expandFolders).toHaveBeenCalledWith(['/docs/flat'], 'top_level');
    expect(files.get().map((f) => f.path)).toEqual(['/docs/flat/a.pdf']);
  });

  it('FR-004: does not park the same folder twice', async () => {
    ipc.inspectFiles.mockResolvedValue([folderInfo('/docs/reports', true)]);
    const files = collector();

    await addPathsToJob(['/docs/reports'], files.setFiles);
    await addPathsToJob(['/docs/reports'], files.setFiles);

    expect(getPendingFolders()).toHaveLength(1);
  });

  it('FR-004: the answer expands the folder and clears the question', async () => {
    ipc.inspectFiles.mockResolvedValue([fileInfo('/docs/reports/a.pdf')]);
    ipc.expandFolders.mockResolvedValue([
      expansion({
        root: '/docs/reports',
        scan: 'recursive',
        files: ['/docs/reports/a.pdf'],
      }),
    ]);
    const files = collector();

    await expandFolderIntoJob(
      { path: '/docs/reports', name: 'reports', hasSubfolders: true },
      'recursive',
      files.setFiles,
    );

    expect(ipc.expandFolders).toHaveBeenCalledWith(
      ['/docs/reports'],
      'recursive',
    );
    expect(files.get().map((f) => f.path)).toEqual(['/docs/reports/a.pdf']);
    expect(getPendingFolders()).toEqual([]);
  });

  it('FR-005: shows the files a folder walk discarded, with their reason', async () => {
    ipc.inspectFiles.mockResolvedValue([fileInfo('/docs/r/a.pdf')]);
    ipc.expandFolders.mockResolvedValue([
      expansion({
        root: '/docs/r',
        files: ['/docs/r/a.pdf'],
        skipped: [
          {
            path: '/docs/r/notes.docx',
            name: 'notes.docx',
            reason: 'unsupported_format',
            detail: 'Unsupported file type (.docx)',
          },
        ],
      }),
    ]);
    const files = collector();

    await expandFolderIntoJob(
      { path: '/docs/r', name: 'r', hasSubfolders: false },
      'top_level',
      files.setFiles,
    );

    expect(toasts.warning).toHaveBeenCalledTimes(1);
    const [, opts] = toasts.warning.mock.calls[0] as [
      string,
      { description: string },
    ];
    expect(opts.description).toContain(
      'notes.docx — Unsupported file type (.docx)',
    );
  });

  it('warns when a safety ceiling truncated the walk', async () => {
    ipc.inspectFiles.mockResolvedValue([]);
    ipc.expandFolders.mockResolvedValue([
      expansion({
        scan: 'recursive',
        files_truncated: true,
        depth_truncated: true,
      }),
    ]);
    const files = collector();

    await expandFolderIntoJob(
      { path: '/docs/huge', name: 'huge', hasSubfolders: true },
      'recursive',
      files.setFiles,
    );

    // One warning for the entry ceiling, one for the depth ceiling.
    expect(toasts.warning).toHaveBeenCalledTimes(2);
  });

  it('says so when a folder holds nothing printable', async () => {
    ipc.expandFolders.mockResolvedValue([expansion()]);
    const files = collector();

    await expandFolderIntoJob(
      { path: '/docs/empty', name: 'empty', hasSubfolders: false },
      'top_level',
      files.setFiles,
    );

    expect(toasts.info).toHaveBeenCalledTimes(1);
  });

  it('a failed expansion is reported, not swallowed', async () => {
    ipc.expandFolders.mockRejectedValue(new Error('permission denied'));
    const files = collector();

    await expandFolderIntoJob(
      { path: '/docs/x', name: 'x', hasSubfolders: false },
      'top_level',
      files.setFiles,
    );

    expect(toasts.error).toHaveBeenCalledWith('permission denied');
  });

  // -- the pending-folder store -----------------------------------------

  it('notifies subscribers when the pending folders change', async () => {
    const seen = vi.fn();
    const unsubscribe = subscribePendingFolders(seen);
    ipc.inspectFiles.mockResolvedValue([folderInfo('/docs/reports', true)]);
    const files = collector();

    await addPathsToJob(['/docs/reports'], files.setFiles);
    expect(seen).toHaveBeenCalled();

    const before = getPendingFolders();
    resolvePendingFolder('/docs/reports');
    expect(getPendingFolders()).toEqual([]);
    expect(getPendingFolders()).not.toBe(before);

    unsubscribe();
    resolvePendingFolder('/nothing');
    expect(seen).toHaveBeenCalledTimes(2);
  });

  it('returns a referentially stable snapshot between changes', () => {
    expect(getPendingFolders()).toBe(getPendingFolders());
  });
});
