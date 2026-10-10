import { browser, $, expect } from '@wdio/globals';

import {
  byText,
  dismissOnboarding,
  invokeIpc,
  navigateTo,
  replaceInputText,
} from './helpers.js';

/** Names this spec creates; anything carrying them is removed afterwards. */
const CREATED = ['E2E profile', 'Interrupted draft'];

/**
 * Put the runner's profiles back: the built-in Quick profile is the default
 * again, and every profile this spec made (by a name that contains one of
 * `CREATED`, which also catches a run that typed into a pre-filled field) is
 * deleted. The specs drive the real binary, so `profiles.toml` is the
 * runner's own: a leftover default would leak into every later spec (Home's
 * CLI command names it) and into the next run. Never throws.
 */
async function removeE2eProfiles(): Promise<void> {
  try {
    await invokeIpc('set_default_profile', { id: 'builtin-quick' });
    const profiles = (await invokeIpc('list_profiles')) as {
      id: string;
      name: string;
      builtin?: boolean;
    }[];
    for (const p of profiles) {
      if (!p.builtin && CREATED.some((n) => p.name.includes(n))) {
        await invokeIpc('delete_profile', { id: p.id });
      }
    }
  } catch (error) {
    console.warn('could not remove the E2E profiles:', error);
  }
}

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
  before(async () => {
    await dismissOnboarding();
    await removeE2eProfiles();
  });

  after(async () => {
    await removeE2eProfiles();
  });

  it('creates a custom profile and makes it the default', async () => {
    await dismissOnboarding();
    await navigateTo('Profiles');

    // Start a new draft in the editor.
    await (await $('button*=New profile')).click();

    // Name it — the editor's name field placeholder is "Profile name". The
    // draft comes pre-filled, so the name is replaced, not appended to.
    const name = await $('input[placeholder="Profile name"]');
    await name.waitForExist({ timeout: 10_000 });
    await replaceInputText(name, 'E2E profile');
    await expect(name).toHaveValue('E2E profile');

    // A brand-new draft saves via "Create profile".
    await (await $('button*=Create profile')).click();

    // It shows up in the master list.
    await expect(byText('E2E profile')).toBeExisting();

    // Promote it to the app default: the backend records it, and the list
    // shows the "default" badge (an element whose whole text is "default").
    await (await $('button*=Make default')).click();
    await browser.waitUntil(
      async () => {
        const profiles = (await invokeIpc('list_profiles')) as {
          name: string;
          default?: boolean;
        }[];
        return profiles.some((p) => p.name === 'E2E profile' && p.default);
      },
      { timeout: 10_000, timeoutMsg: 'expected E2E profile to be the default' },
    );
    await expect($('//*[normalize-space(text())="default"]')).toBeExisting();
  });

  it('keeps unsaved edits when the user leaves the screen and comes back', async () => {
    await dismissOnboarding();
    await navigateTo('Profiles');

    // Open a draft and half-type a name.
    await (await $('button*=New profile')).click();
    // Leaving an unsaved edit behind asks first; this test starts clean.
    const discard = await $('button=Discard');
    if (await discard.isExisting()) await discard.click();
    const name = await $('input[placeholder="Profile name"]');
    await name.waitForExist({ timeout: 10_000 });
    await replaceInputText(name, 'Interrupted draft');

    // Leave through the sidebar (the editor is unmounted) and come back.
    await navigateTo('Queue');
    await navigateTo('Profiles');

    const restored = await $('input[placeholder="Profile name"]');
    await restored.waitForExist({ timeout: 10_000 });
    await expect(restored).toHaveValue('Interrupted draft');
  });
});
