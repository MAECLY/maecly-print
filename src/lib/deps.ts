/**
 * Which `common:deps.<id>.*` sentence describes a missing dependency on this
 * platform (FR-058, FR-092).
 *
 * A dependency can block different files per OS — LibreOffice is RTF's route
 * on Windows, while macOS prints RTF through `textutil` — so a dependency may
 * carry a `remediationMac` variant next to its `remediation`. Every surface
 * that names a missing dependency (Print Job's card, the install dialog, a
 * paused job's error) goes through here so they all say the same thing.
 */

import { isMacPlatform } from '@/components/AppShell/MPAppShell';
import type { InstallOutcome } from '@/ipc/bindings';
import type { UiQueueJobView } from '@/lib/ipc';
import {
  CATALOGS,
  getActiveLocale,
  hasTranslation,
  type Locale,
  type TranslationKey,
} from '@/i18n';

/** The `common` key of `id`'s remediation sentence on this platform. */
export function remediationKey(
  id: string,
  opts: { mac?: boolean; locale?: Locale } = {},
): TranslationKey<'common'> {
  const mac = opts.mac ?? isMacPlatform();
  const catalog = CATALOGS[opts.locale ?? getActiveLocale()];
  const macKey = `deps.${id}.remediationMac`;
  if (mac && hasTranslation(catalog, 'common', macKey)) {
    return macKey as TranslationKey<'common'>;
  }
  return `deps.${id}.remediation` as TranslationKey<'common'>;
}

/**
 * The dependency a paused job is waiting for (FR-058), or `null`. A job that
 * stopped on `dependency_missing` carries the dependency id as its reason;
 * the Queue offers to install it (the same consent dialog Print Job uses)
 * and then resumes the job.
 */
export function missingDependency(
  j: Pick<UiQueueJobView, 'status' | 'errorInfo'>,
): string | null {
  if (j.status !== 'paused') return null;
  if (j.errorInfo?.code !== 'dependency_missing') return null;
  return j.errorInfo.reason ?? null;
}

/**
 * The honest end of a Stop the installer may outlive: Windows lets an
 * elevated installer (or the msiexec service) carry on after its parent is
 * killed. The backend names it with this reason, either as the install
 * outcome's `reason` or as its status (kebab-case like the others), and no
 * surface may then claim the install stopped.
 */
export const CANCEL_MAY_CONTINUE = 'cancel_requested_installer_may_continue';

/** True when `o` is a stop the installer may have outlived. */
export function cancelMayContinue(o: InstallOutcome): boolean {
  const loose = o as unknown as { status: string; reason?: string | null };
  return (
    loose.reason === CANCEL_MAY_CONTINUE ||
    loose.status === CANCEL_MAY_CONTINUE ||
    loose.status === CANCEL_MAY_CONTINUE.replaceAll('_', '-')
  );
}
