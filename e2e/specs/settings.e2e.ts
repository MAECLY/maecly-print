import { browser, $, expect } from '@wdio/globals';

import {
  dismissOnboarding,
  navigateTo,
  restoreDefaultSettings,
  selectLanguage,
} from './helpers.js';

/** Read the class list on the document root (where the theme is applied). */
function rootClass(): Promise<string> {
  return browser.execute(() => document.documentElement.className);
}

/** Read the density the appearance layer publishes on the document root. */
function rootDensity(): Promise<string> {
  return browser.execute(() => document.documentElement.dataset.density ?? '');
}

/**
 * Drop every localStorage key and reload the webview. The instant-boot caches
 * (theme, density, onboarding) live there, so clearing them before the reload
 * means whatever the app renders afterwards can only have come from
 * `config.toml` via `get_app_settings` — which is exactly the persistence this
 * suite is here to prove.
 */
async function relaunchWithoutCaches(): Promise<void> {
  await browser.execute(() => {
    window.localStorage.clear();
    window.location.reload();
  });
  await (await $('button*=Settings')).waitForExist({ timeout: 20_000 });
  await dismissOnboarding();
}

/**
 * Settings, end to end. Two things Vitest cannot reach: that an appearance
 * change really lands on the document of the running webview, and that the
 * dual-write survives a boot with the localStorage cache wiped (FR-100…FR-103).
 *
 * Every test here writes the runner's real `config.toml`, so the restore runs
 * in `afterEach` — a hook Mocha runs whether the test passed, failed or threw
 * mid-way. Restoring at the end of the test body (as this suite used to) leaves
 * the config dark-themed, compact or in Spanish for every later spec the moment
 * one assertion fails.
 */
describe('Settings', () => {
  afterEach(async () => {
    await restoreDefaultSettings();
  });

  it('switches the theme to dark and applies the dark class', async () => {
    await dismissOnboarding();
    await navigateTo('Settings');

    // Switch to the dark theme (resilient: a control labelled "Dark").
    await (await $('button*=Dark')).click();

    await browser.waitUntil(
      async () => (await rootClass()).includes('mp-dark'),
      {
        timeout: 5_000,
        timeoutMsg: 'expected <html> to gain the mp-dark class',
      },
    );

    expect(await rootClass()).toContain('mp-dark');
  });

  it('FR-100: a changed setting survives a relaunch with the local cache cleared', async () => {
    await dismissOnboarding();
    await navigateTo('Settings');

    // Two settings from different sections, one of them appearance (which has
    // the localStorage cache we are about to wipe).
    await (await $('button*=Dark')).click();
    await (await $('button*=Compact')).click();
    await browser.waitUntil(async () => (await rootDensity()) === 'compact', {
      timeout: 5_000,
      timeoutMsg: 'expected data-density="compact"',
    });

    await relaunchWithoutCaches();

    // Nothing local survived the wipe, so a dark, compact boot can only come
    // from the persisted config.
    await browser.waitUntil(
      async () =>
        (await rootClass()).includes('mp-dark') &&
        (await rootDensity()) === 'compact',
      {
        timeout: 20_000,
        timeoutMsg: 'expected the persisted theme + density to be restored',
      },
    );
  });

  it('FR-094: switching the language re-renders the app, and the choice persists', async () => {
    await dismissOnboarding();
    await navigateTo('Settings');

    await selectLanguage('es-ES');

    // The whole shell follows the setting, not just this screen.
    await browser.waitUntil(
      async () => (await (await $('body')).getText()).includes('Ajustes'),
      { timeout: 10_000, timeoutMsg: 'expected the UI to switch to Spanish' },
    );

    await relaunchWithoutCaches();
    // The sidebar is not translated yet (it is not part of this screen), so it
    // is still the English "Settings" that gets us back to the screen.
    await navigateTo('Settings');

    await browser.waitUntil(
      async () => (await (await $('body')).getText()).includes('Ajustes'),
      {
        timeout: 20_000,
        timeoutMsg: 'expected the persisted locale to survive the relaunch',
      },
    );
  });
});
