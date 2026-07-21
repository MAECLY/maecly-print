import { $, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * Profiles flows against the real app. These run on Windows/Linux CI
 * (tauri-driver), not on a developer Mac; selectors prefer visible text so they
 * survive minor markup changes and get tuned when the CI e2e job first runs.
 *
 * Import/export are deliberately NOT driven here: both open a native OS file
 * dialog, which WebDriver cannot reach from inside the webview. Their contract
 * is covered by the Rust integration tests (`export_profile_at` /
 * `import_profiles_at`) and by the mocked-dialog tests in
 * `src/screens/ProfilesScreen.test.tsx`.
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

  it('keeps unsaved edits when the user leaves the screen and comes back', async () => {
    await dismissOnboarding();
    await navigateTo('Profiles');

    // Open a draft and half-type a name.
    await (await $('button*=New profile')).click();
    const name = await $('input[placeholder="Profile name"]');
    await name.waitForExist({ timeout: 10_000 });
    await name.setValue('Interrupted draft');

    // Leave through the sidebar (the editor is unmounted) and come back.
    await navigateTo('Queue');
    await navigateTo('Profiles');

    const restored = await $('input[placeholder="Profile name"]');
    await restored.waitForExist({ timeout: 10_000 });
    await expect(restored).toHaveValue('Interrupted draft');
  });
});
