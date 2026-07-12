import { browser, $, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/** Read the class list on the document root (where the theme is applied). */
function rootClass(): Promise<string> {
  return browser.execute(() => document.documentElement.className);
}

/**
 * Settings appearance flow: switching the theme to dark applies the `mp-dark`
 * class to <html> (see src/lib/theme.ts).
 */
describe('Settings', () => {
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

    // Note: this verifies in-session application only. Asserting persistence
    // (dual-write to config.toml) by relaunching and asserting a dark boot
    // would need a full tauri-driver session restart, which is impractical to
    // do reliably here.
  });
});
