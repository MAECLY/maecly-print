import { $, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * About flow (FR-110): the identity block, the components card, the live
 * filesystem paths and the external links all render against the real backend.
 *
 * We deliberately do NOT click "Check for updates" — that hits api.github.com,
 * which would make the suite flaky and rate-limited — nor any of the link
 * buttons, which hand a URL to the OS default browser and would leave a window
 * open on the CI agent. Existence and enabled-ness is what this level can
 * honestly assert; the behaviour behind each button is covered by
 * `AboutScreen.test.tsx` and the Rust unit tests.
 */
describe('About', () => {
  beforeEach(async () => {
    await dismissOnboarding();
    await navigateTo('About');
  });

  it('shows a semver version string', async () => {
    await expect($('body')).toHaveText(/\d+\.\d+\.\d+/);
  });

  it('exposes the license and release-notes links', async () => {
    await expect($('button*=License')).toBeExisting();
    await expect($('button*=Release notes')).toBeExisting();
  });

  it('FR-110: exposes the documentation and issue links too', async () => {
    await expect($('button*=Documentation')).toBeExisting();
    await expect($('button*=Report an issue')).toBeExisting();
  });

  it('FR-113: shows the live log directory and offers to open it', async () => {
    // The paths card only renders once `app_info` resolved, so its presence is
    // also proof the IPC round-trip worked inside the packaged app.
    await expect($('body')).toHaveText(/logs:/);
    await expect($('button*=Open logs folder')).toBeExisting();
  });

  it('reports the running React and Tauri versions and the host platform', async () => {
    await expect($('body')).toHaveText(/React/);
    await expect($('body')).toHaveText(/Tauri/);
    // `Platform: windows · x86_64` — from `app_info`, not a frontend guess.
    await expect($('body')).toHaveText(/Platform:/);
    await expect($('body')).toHaveText(/windows/);
  });

  it('enables the update check inside the real host', async () => {
    // Outside Tauri this button is disabled; in the packaged app it must not
    // be — that difference is the whole point of running this spec on Windows.
    const check = await $('button*=Check for updates');
    await expect(check).toBeExisting();
    await expect(check).toBeEnabled();
  });

  it('does not show the IPC-unavailable fallback in the packaged app', async () => {
    await expect($('body')).not.toHaveText(/IPC unavailable/);
    await expect($('body')).not.toHaveText(/standalone web preview/);
  });
});
