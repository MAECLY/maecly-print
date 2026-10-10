/**
 * The backend calls and the sentences of a dependency install (FR-058),
 * shared by the install dialog and the app shell, which picks up an install
 * still running after the window reloaded (`useInstallStore.hydrate`).
 */

import type { TFunction } from '@/i18n/I18nProvider';
import {
  commands,
  events,
  type Dependency,
  type InstallOutcome,
} from '@/ipc/bindings';
import { missingDependency } from '@/lib/deps';
import {
  fetchAppInfo,
  queueStatus,
  resumeJob,
  toUiQueueJob,
  unwrap,
} from '@/lib/ipc';
import { errorMessage, IpcFailure } from '@/lib/ipcError';
import {
  installerMayContinue,
  type InstallBackend,
  type InstallEnd,
  type ResumeTally,
} from '@/stores/installStore';
import { toast } from 'sonner';

/** The real backend: Tauri commands and the install's progress event. */
export const realInstallBackend = {
  // The dialog's Install button is the consent (D13).
  run: (id, method) => unwrap(commands.installDependency(id, method, true)),
  progress: (onLine) =>
    events.dependencyInstallProgress.listen(({ payload }) =>
      onLine(payload.id, payload.line),
    ),
  describe: errorMessage,
  status: () => unwrap(commands.dependencyInstallStatus()),
  probe: async (id) =>
    (await fetchAppInfo()).dependencies?.find((d) => d.id === id) ?? null,
  resumeWaiting: (id) => resumeJobsWaitingOn(id),
} satisfies InstallBackend;

/**
 * Resume every queue job paused on `dependency_missing` for `id`, through
 * the same `resume_job` command the Queue's Resume uses. The queue is read
 * from the backend, not from the Queue screen's store, so this works with
 * the Queue not on screen and right after a reload. A job that moved on
 * before its resume arrived (finished, removed, or no longer paused) is
 * neither resumed nor a failure. Rejects when the queue cannot be read.
 */
export async function resumeJobsWaitingOn(
  id: Dependency,
  backend: {
    queueStatus: typeof queueStatus;
    resumeJob: typeof resumeJob;
  } = { queueStatus, resumeJob },
): Promise<ResumeTally> {
  const snapshot = await backend.queueStatus();
  const waiting = snapshot.jobs
    .map(toUiQueueJob)
    .filter((j) => missingDependency(j) === id);
  const tally: ResumeTally = { resumed: 0, failed: 0, checked: true };
  for (const j of waiting) {
    try {
      await backend.resumeJob(j.id);
      tally.resumed++;
    } catch (e) {
      const movedOn =
        e instanceof IpcFailure &&
        (e.code === 'JobNotFound' || e.code === 'InvalidState');
      if (!movedOn) tally.failed++;
    }
  }
  return tally;
}

/**
 * The sentence for how an install of `name` ended. A stop the installer may
 * have outlived gives administrator rights as the reason only when the
 * installer had them (`outcome.elevated`). `resumesJobs`: the caller resumes
 * the jobs waiting for it (the Queue), so "opened" can promise that.
 */
export function installResultText(
  t: TFunction<'printJob'>,
  o: InstallOutcome,
  name: string,
  resumesJobs = false,
): string {
  switch (o.status) {
    case 'installed':
      return t('install.result.installed', { name });
    case 'not-detected':
      return t('install.result.notDetected', { name });
    case 'opened':
      return resumesJobs
        ? t('install.result.openedResumesJobs', { name })
        : t('install.result.opened');
    case 'timed-out':
      return t('install.result.timedOut');
    case 'timed-out-installer-may-continue':
      return o.elevated
        ? t('install.result.timedOutInstallerMayContinue', { name })
        : t('install.result.timedOutMayContinueUnconfirmed', { name });
    case 'cancelled':
      return t('install.result.cancelled');
    case 'cancel-requested-installer-may-continue':
      return o.elevated
        ? t('install.result.cancelRequestedInstallerMayContinue', { name })
        : t('install.result.cancelRequestedMayContinueUnconfirmed', { name });
    case 'failed':
      // No exit code means the OS gave none: say so instead of "code —".
      return o.exit_code == null
        ? t('install.result.failedNoCode')
        : t('install.result.failed', { code: o.exit_code });
  }
}

/**
 * Announce an install end no dialog is showing (a toast). When the store
 * resumed the queue's waiting jobs itself (`end.resumed`), the toast says
 * what happened to them. `standalone`: no screen's files were waiting for
 * it (the app shell announcing an install picked up after a reload), so an
 * install that unblocked nothing does not claim that files can print now.
 */
export function announceInstallEnd(
  t: TFunction<'printJob'>,
  end: InstallEnd,
  resumesJobs = false,
  standalone = false,
): void {
  const name = end.name;
  const title = t('install.title', { name });
  const installed = (): string =>
    standalone
      ? t('install.result.installedAlone', { name })
      : t('install.result.installed', { name });
  // Set only when the dependency is found now.
  const r = end.resumed;
  if (r) {
    if (!r.checked) {
      toast.warning(title, {
        description: t('install.result.installedResumeUnchecked', { name }),
      });
      return;
    }
    if (r.failed > 0) {
      toast.warning(title, {
        description: t('install.result.installedResumeFailed', {
          name,
          count: r.failed,
        }),
      });
      return;
    }
    if (r.resumed > 0) {
      toast.success(title, {
        description: t('install.result.installedResumed', {
          name,
          count: r.resumed,
        }),
      });
      return;
    }
    if (end.outcome?.status !== 'installed') {
      // Found after an end that did not say so (e.g. one nobody saw).
      toast.success(title, { description: installed() });
      return;
    }
  }
  if (!end.outcome) {
    toast.error(end.failure);
    return;
  }
  const o = end.outcome;
  const description =
    o.status === 'installed'
      ? installed()
      : installResultText(t, o, name, resumesJobs);
  if (o.status === 'installed') toast.success(title, { description });
  else if (o.status === 'cancelled' && !installerMayContinue(o))
    toast.message(title, { description });
  else toast.warning(title, { description });
}
