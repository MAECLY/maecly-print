/** Shared helpers for the E2E specs (not a spec file — no `*.spec.ts` glob). */
import { $ } from '@wdio/globals';

/**
 * A fresh app launch shows the onboarding overlay (zIndex above the shell).
 * Dismiss it via its "Skip" button so the primary UI is interactive. A no-op
 * if onboarding was already completed.
 */
export async function dismissOnboarding(): Promise<void> {
  const skip = await $('button=Skip');
  if (await skip.isExisting()) {
    await skip.click();
    // Wait for the overlay to tear down before interacting with the shell.
    await skip
      .waitForExist({ reverse: true, timeout: 5_000 })
      .catch(() => undefined);
  }
}

/** Click a left-nav item by its visible label (e.g. "Printers", "About"). */
export async function navigateTo(label: string): Promise<void> {
  const item = await $(`button*=${label}`);
  await item.waitForClickable({ timeout: 10_000 });
  await item.click();
}
