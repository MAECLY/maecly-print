/**
 * Shared "add files to the print job" pipeline: inspect paths through the
 * backend, toast the unprintable ones, and append the rest to the job's file
 * list. Used by the Print Job screen (Add files button + its drop overlay) and
 * by the app-level drag & drop listener, so dropping files works from any
 * screen (FR-007) with identical behavior.
 *
 * Folders are the one input this module cannot finish on its own: FR-004 says
 * the user decides whether a folder is walked recursively or only one level
 * deep, and only a screen can ask. So a dropped folder is parked in the
 * {@link getPendingFolders} store instead of being expanded or rejected, and
 * the Print Job screen — which the app navigates to on any drop — renders the
 * question and calls `expand_folders` with the answer. A folder with no
 * subfolders is not parked at all: there is nothing to recurse into, so it is
 * expanded top-level immediately (FR-004 asks; it does not nag).
 *
 * Strings are translated here rather than passed in, so the app-level drop
 * handler gets the same localized toasts as the screen without every caller
 * having to thread a `t` through. The locale is read at call time from the
 * settings store — a toast is a one-shot render, so it does not need to be
 * reactive.
 */

import type { Dispatch, SetStateAction } from 'react';
import { toast } from 'sonner';

import {
  CATALOGS,
  matchLocale,
  translate,
  type TranslateParams,
  type TranslationKey,
} from '@/i18n';
import { detectSystemLocale } from '@/i18n/I18nProvider';
import {
  expandFolders,
  inspectFiles,
  type FileInfo,
  type FolderExpansion,
  type FolderScan,
} from '@/lib/ipc';
import { useSettingsStore } from '@/stores/settingsStore';
import type { UiFile } from '@/types/domain';

/** Translate a `printJob` key using whatever locale is active right now. */
function t(key: TranslationKey<'printJob'>, params?: TranslateParams): string {
  const configured = useSettingsStore.getState().settings?.app.locale;
  const locale = matchLocale(configured) ?? detectSystemLocale();
  return translate(CATALOGS[locale], 'printJob', key, params);
}

/** A path the backend refused, ready to list in a toast (FR-005). */
export interface RejectedFile {
  name: string;
  reason: string;
}

/** A dropped folder waiting on the FR-004 recurse-or-not question. */
export interface PendingFolder {
  path: string;
  name: string;
  hasSubfolders: boolean;
}

/** What one `inspect_files` round-trip resolved to. */
export interface InspectResult {
  files: UiFile[];
  rejected: RejectedFile[];
  folders: PendingFolder[];
}

/** `report.pdf` + `pdf` → `report`; leaves names without the ext untouched. */
function stripExt(name: string, ext: string): string {
  const suffix = `.${ext}`;
  return name.toLowerCase().endsWith(suffix)
    ? name.slice(0, -suffix.length)
    : name;
}

function toUiFile(fi: FileInfo): UiFile {
  return {
    id: fi.path,
    name: stripExt(fi.name, fi.ext),
    ext: fi.ext,
    kb: Math.round(fi.bytes / 1024),
    pages: fi.pages,
    path: fi.path,
  };
}

// ---------------------------------------------------------------------------
// Pending folders (FR-004)
// ---------------------------------------------------------------------------

let pendingFolders: readonly PendingFolder[] = [];
const folderListeners = new Set<() => void>();

function setPendingFolders(next: readonly PendingFolder[]): void {
  pendingFolders = next;
  for (const listener of folderListeners) listener();
}

/**
 * The folders still awaiting an answer. Stable between changes so it can back a
 * `useSyncExternalStore` without re-rendering on every unrelated update.
 */
export function getPendingFolders(): readonly PendingFolder[] {
  return pendingFolders;
}

/** Subscribe to {@link getPendingFolders}; returns the unsubscribe function. */
export function subscribePendingFolders(listener: () => void): () => void {
  folderListeners.add(listener);
  return () => folderListeners.delete(listener);
}

/** Drop one folder from the pending list (answered, or dismissed). */
export function resolvePendingFolder(path: string): void {
  setPendingFolders(pendingFolders.filter((f) => f.path !== path));
}

/** Forget every pending folder (used when the job is cleared, and by tests). */
export function clearPendingFolders(): void {
  setPendingFolders([]);
}

// ---------------------------------------------------------------------------
// Inspection
// ---------------------------------------------------------------------------

/**
 * Inspect `paths` and split them the three ways the UI acts on: printable
 * files, folders that need the FR-004 question, and refusals with a reason.
 *
 * Never throws for a bad path — the backend reports those per entry — but does
 * propagate a failure of the call itself.
 */
export async function inspectPaths(paths: string[]): Promise<InspectResult> {
  const out: InspectResult = { files: [], rejected: [], folders: [] };
  if (paths.length === 0) return out;
  const infos = await inspectFiles(paths);
  const seen = new Set<string>();
  for (const fi of infos) {
    if (fi.is_dir) {
      out.folders.push({
        path: fi.path,
        name: fi.name,
        hasSubfolders: fi.has_subfolders,
      });
      continue;
    }
    if (!fi.supported) {
      out.rejected.push({ name: fi.name, reason: fi.reason ?? '' });
      continue;
    }
    if (seen.has(fi.path)) continue;
    seen.add(fi.path);
    out.files.push(toUiFile(fi));
  }
  return out;
}

/**
 * Append `files` to the job, deduplicating by path.
 *
 * The dedup happens inside the functional state update, so concurrent adds — a
 * drop racing the file picker — can't lose files or append duplicates. Returns
 * nothing: the caller learns what stuck by watching the list, not by counting.
 */
export function appendFiles(
  setFiles: Dispatch<SetStateAction<UiFile[]>>,
  files: UiFile[],
): void {
  if (files.length === 0) return;
  setFiles((cur) => {
    const have = new Set(cur.map((f) => f.path));
    const added = files.filter((f) => !have.has(f.path));
    return added.length > 0 ? [...cur, ...added] : cur;
  });
}

/** One toast listing every refusal by name + reason (FR-005). */
export function toastRejected(rejected: RejectedFile[]): void {
  if (rejected.length === 0) return;
  toast.error(t('toasts.rejected', { count: rejected.length }), {
    description: rejected
      .map((r) => (r.reason ? `${r.name} — ${r.reason}` : r.name))
      .join('\n'),
  });
}

/** Paths already in the job right now, for honest "added N" counts. */
export type CurrentPaths = () => ReadonlySet<string>;

/** `files` minus anything already in the job (and minus repeats among them). */
function freshOnly(files: UiFile[], currentPaths?: CurrentPaths): UiFile[] {
  const have = new Set(currentPaths?.() ?? []);
  return files.filter((f) => {
    if (have.has(f.path)) return false;
    have.add(f.path);
    return true;
  });
}

// ---------------------------------------------------------------------------
// Folder walks in progress (FR-004)
// ---------------------------------------------------------------------------

let expanding = 0;
const expandingListeners = new Set<() => void>();

function setExpanding(next: number): void {
  expanding = next;
  for (const listener of expandingListeners) listener();
}

/**
 * How many folder walks are still running. While it is above zero the job is
 * still growing, so Print stays disabled: a batch spooled now would leave the
 * late files behind without a word.
 */
export function getExpandingCount(): number {
  return expanding;
}

/** Subscribe to {@link getExpandingCount}; returns the unsubscribe function. */
export function subscribeExpanding(listener: () => void): () => void {
  expandingListeners.add(listener);
  return () => expandingListeners.delete(listener);
}

// ---------------------------------------------------------------------------
// Adding to the job
// ---------------------------------------------------------------------------

/**
 * Inspect `paths` and append the printable ones to the job's file list.
 *
 * Folders with subfolders are parked for the FR-004 question; folders without
 * are expanded top-level right away. Unsupported entries surface in one toast
 * listing name + reason; an inspect failure surfaces as an error toast.
 *
 * Resolves with the files that were new to the job (given `currentPaths`; all
 * of them without it) — the inspected ones plus whatever the question-free
 * folders yielded.
 */
export async function addPathsToJob(
  paths: string[],
  setFiles: Dispatch<SetStateAction<UiFile[]>>,
  currentPaths?: CurrentPaths,
): Promise<UiFile[]> {
  if (paths.length === 0) return [];
  let result: InspectResult;
  try {
    result = await inspectPaths(paths);
  } catch (e) {
    toast.error(e instanceof Error ? e.message : String(e));
    return [];
  }
  const added = freshOnly(result.files, currentPaths);
  appendFiles(setFiles, result.files);
  toastRejected(result.rejected);

  const askable = result.folders.filter((f) => f.hasSubfolders);
  const flat = result.folders.filter((f) => !f.hasSubfolders);
  if (askable.length > 0) {
    const known = new Set(pendingFolders.map((f) => f.path));
    const fresh = askable.filter((f) => !known.has(f.path));
    if (fresh.length > 0) setPendingFolders([...pendingFolders, ...fresh]);
  }
  for (const folder of flat) {
    added.push(
      ...(await expandFolderIntoJob(folder, 'top_level', setFiles, () => {
        const have = new Set(currentPaths?.() ?? []);
        for (const f of added) have.add(f.path);
        return have;
      })),
    );
  }
  return added;
}

/**
 * Walk one folder with the user's answer and fold the result into the job:
 * accepted files are re-inspected (so each row carries its real size and page
 * count) and appended; discarded ones are toasted (FR-005) — the backend has
 * already written them to the app log (FR-059).
 *
 * While the walk runs a loading toast names the folder and
 * {@link getExpandingCount} counts it, so a slow recursive walk never looks
 * like a dead click and Print waits for it. The "added" toast counts only the
 * files that were new to the job (`currentPaths`), so dropping the same folder
 * twice never claims to have added anything the second time.
 *
 * Resolves with those new files.
 */
export async function expandFolderIntoJob(
  folder: PendingFolder,
  scan: FolderScan,
  setFiles: Dispatch<SetStateAction<UiFile[]>>,
  currentPaths?: CurrentPaths,
): Promise<UiFile[]> {
  resolvePendingFolder(folder.path);
  setExpanding(expanding + 1);
  const progress = toast.loading(t('folders.expanding', { name: folder.name }));
  try {
    const walked = await walkFolder(folder, scan);
    const fresh = freshOnly(walked, currentPaths);
    appendFiles(setFiles, walked);
    if (fresh.length > 0) {
      toast.success(
        t('toasts.added', { count: fresh.length, name: folder.name }),
      );
    } else if (walked.length > 0) {
      toast.info(t('toasts.alreadyAdded', { name: folder.name }));
    }
    return fresh;
  } finally {
    toast.dismiss(progress);
    setExpanding(expanding - 1);
  }
}

/**
 * Resolve a context-menu selection that prints (or queues) right away into the
 * files it stands for — **without** touching the interactive job.
 *
 * "Print with MaeclyPrint" never shows a dialog, so nobody is there to answer
 * the FR-004 question: every folder is walked top-level, as
 * `context-menu-actions.md` specifies for that entry (recursion is what
 * "Configure print job…" is for). Refusals and walk warnings are toasted as
 * usual; the result is deduplicated by path.
 */
export async function resolveSelectionFiles(
  paths: string[],
): Promise<UiFile[]> {
  if (paths.length === 0) return [];
  let result: InspectResult;
  try {
    result = await inspectPaths(paths);
  } catch (e) {
    toast.error(e instanceof Error ? e.message : String(e));
    return [];
  }
  toastRejected(result.rejected);
  const out = [...result.files];
  for (const folder of result.folders) {
    out.push(...(await walkFolder(folder, 'top_level')));
  }
  return freshOnly(out);
}

/**
 * Expand one folder and inspect what it holds, toasting what the walk
 * discarded or truncated. Touches neither the job nor the pending questions.
 */
async function walkFolder(
  folder: PendingFolder,
  scan: FolderScan,
): Promise<UiFile[]> {
  let expansions: FolderExpansion[];
  try {
    expansions = await expandFolders([folder.path], scan);
  } catch (e) {
    toast.error(e instanceof Error ? e.message : String(e));
    return [];
  }
  const files: UiFile[] = [];
  for (const expansion of expansions) {
    files.push(...(await ingestExpansion(folder.name, expansion)));
  }
  return files;
}

async function ingestExpansion(
  name: string,
  expansion: FolderExpansion,
): Promise<UiFile[]> {
  const files: UiFile[] = [];
  if (expansion.files.length > 0) {
    try {
      const inspected = await inspectPaths(expansion.files);
      files.push(...inspected.files);
      toastRejected(inspected.rejected);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  } else if (expansion.skipped.length === 0) {
    toast.info(t('folders.empty', { name }));
  }

  if (expansion.skipped.length > 0) {
    toast.warning(
      t('toasts.skipped', { count: expansion.skipped.length, name }),
      {
        description: expansion.skipped
          .map((s) => `${s.name} — ${s.detail}`)
          .join('\n'),
      },
    );
  }
  if (expansion.files_truncated) {
    toast.warning(
      t('folders.truncatedFiles', { name, max: expansion.limits.max_files }),
    );
  }
  if (expansion.depth_truncated && expansion.scan === 'recursive') {
    toast.warning(
      t('folders.truncatedDepth', { name, max: expansion.limits.max_depth }),
    );
  }
  return files;
}
