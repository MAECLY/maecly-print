/**
 * The backend calls and the sentences of a dependency install (FR-058),
 * shared by the install dialog and the app shell, which picks up an install
 * still running after the window reloaded (`useInstallStore.hydrate`).
 */

import type { TFunction } from '@/i18n/I18nProvider';
import { commands, events, type InstallOutcome } from '@/ipc/bindings';
import { unwrap } from '@/lib/ipc';
import { errorMessage } from '@/lib/ipcError';
import {
  installerMayContinue,
  type InstallBackend,
  type InstallEnd,
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
} satisfies InstallBackend;

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

/** Announce an install end no dialog is showing (a toast). */
export function announceInstallEnd(
  t: TFunction<'printJob'>,
  end: InstallEnd,
  resumesJobs = false,
): void {
  if (!end.outcome) {
    toast.error(end.failure);
    return;
  }
  const o = end.outcome;
  const title = t('install.title', { name: end.name });
  const description = installResultText(t, o, end.name, resumesJobs);
  if (o.status === 'installed') toast.success(title, { description });
  else if (o.status === 'cancelled' && !installerMayContinue(o))
    toast.message(title, { description });
  else toast.warning(title, { description });
}
