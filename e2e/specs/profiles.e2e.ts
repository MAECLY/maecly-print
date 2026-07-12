import { $, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * Profiles CRUD flow: create a custom profile and promote it to default. These
 * run against the real app on Windows/Linux CI (tauri-driver), not this Mac;
 * selectors prefer visible text so they survive minor markup changes and get
 * tuned when the CI e2e job first runs.
 */
describe('Profiles', () => {
  it('creates a custom profile and makes it the default', async () => {
    await dismissOnboarding();
    await navigateTo('Profiles');

    // Start a new draft in the editor.
    await (await $('button*=New profile')).click();

    // Name it — the editor's name field placeholder is "Profile name".
    const name = await $('input[placeholder="Profile name"]');
    await name.waitForExist({ timeout: 10_000 });
    await name.setValue('E2E profile');

    // A brand-new draft saves via "Create profile".
    await (await $('button*=Create profile')).click();

    // It shows up in the master list.
    await expect($('*=E2E profile')).toBeExisting();

    // Promote it to the app default; a "default" badge should appear.
    await (await $('button*=Make default')).click();
    await expect($('*=default')).toBeExisting();
  });
});
