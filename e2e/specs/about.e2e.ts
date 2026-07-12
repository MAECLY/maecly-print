import { $, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * About flow: the version and the license / release-notes links are visible.
 * We deliberately do NOT click "Check for updates" — that hits the network.
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
});
