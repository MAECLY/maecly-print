/**
 * Shared "add files to the print job" pipeline: inspect paths through the
 * backend, toast the unprintable ones, and append the rest to the job's file
 * list. Used by the Print Job screen (Add files button + its drop overlay) and
 * by the app-level drag & drop listener, so dropping files works from any
 * screen (FR-007) with identical behavior.
 */

import type { Dispatch, SetStateAction } from 'react';
import { toast } from 'sonner';

import { inspectFiles, type FileInfo } from '@/lib/ipc';
import { pluralize } from '@/lib/format';
import type { UiFile } from '@/types/domain';

/** `report.pdf` + `pdf` → `report`; leaves names without the ext untouched. */
export function stripExt(name: string, ext: string): string {
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

/**
 * Inspect `paths` and append the printable ones to the job's file list.
 *
 * Deduplication (by path) happens inside the functional state update, so
 * concurrent calls — a drop racing the file picker — can't lose files or
 * append duplicates. Unsupported entries surface in one toast listing name +
 * reason; an inspect failure surfaces as an error toast.
 */
export async function addPathsToJob(
  paths: string[],
  setFiles: Dispatch<SetStateAction<UiFile[]>>,
): Promise<void> {
  if (paths.length === 0) return;
  let infos: FileInfo[];
  try {
    infos = await inspectFiles(paths);
  } catch (e) {
    toast.error(e instanceof Error ? e.message : String(e));
    return;
  }
  const rejected: string[] = [];
  const candidates: UiFile[] = [];
  const batchSeen = new Set<string>();
  for (const fi of infos) {
    if (!fi.supported) {
      rejected.push(`${fi.name}${fi.reason ? ` — ${fi.reason}` : ''}`);
      continue;
    }
    if (batchSeen.has(fi.path)) continue;
    batchSeen.add(fi.path);
    candidates.push(toUiFile(fi));
  }
  if (candidates.length > 0) {
    setFiles((cur) => {
      const have = new Set(cur.map((f) => f.path));
      const added = candidates.filter((f) => !have.has(f.path));
      return added.length > 0 ? [...cur, ...added] : cur;
    });
  }
  if (rejected.length > 0) {
    toast.error(
      `Can't print ${rejected.length} ${pluralize(rejected.length, 'file')}`,
      { description: rejected.join('\n') },
    );
  }
}
