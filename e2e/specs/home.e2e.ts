import { browser, $, $$, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * Home screen, end to end in the real app.
 *
 * This lane exists for the two things Vitest cannot reach: that the global
 * keyboard layer is bound to a REAL window (the component tests dispatch
 * synthetic events onto jsdom's `window`, so a listener that never registers in
 * the packaged app would still pass there), and that the recent-activity card
 * is fed by a live `queue_status` rather than by the mocked store.
 *
 * Nothing here prints or cancels: mutating the runner's queue would leak state
 * into the other specs. The shortcuts exercised are the navigational ones.
 */
describe('Home', () => {
  beforeEach(async () => {
    await dismissOnboarding();
    await navigateTo('Home');
  });

  it('advertises only the formats the engine accepts', async () => {
    const body = await (await $('body')).getText();
    expect(body).toContain('PDF');
    expect(body).toContain('WEBP');
    // Formats the router rejects must never be promised on the first screen.
    for (const absent of ['Office', 'ODF', 'HTML', 'Markdown']) {
      expect(body).not.toContain(absent);
    }
  });

  it('does not claim the shell right-click integration already exists', async () => {
    const body = await (await $('body')).getText();
    expect(body).not.toContain('adds itself to the right-click menu');
    // The CLI, which this build really ships, is what the tip points at.
    expect(body).toContain('pluryprint print --files');
  });

  it('NFR-034: Ctrl/Cmd+N opens a new print job from Home', async () => {
    const modifier = process.platform === 'darwin' ? 'Meta' : 'Control';
    await browser.keys([modifier, 'n']);
    await browser.waitUntil(
      async () => (await (await $('body')).getText()).includes('Add files'),
      {
        timeout: 10_000,
        timeoutMsg: 'expected Ctrl/Cmd+N to open the job screen',
      },
    );
    // Release the modifier so it cannot leak into the next interaction.
    await browser.keys(['']);
  });

  it('NFR-034: Ctrl/Cmd+J opens the queue from Home', async () => {
    const modifier = process.platform === 'darwin' ? 'Meta' : 'Control';
    await browser.keys([modifier, 'j']);
    await browser.waitUntil(
      async () => {
        const body = await (await $('body')).getText();
        return (
          /active\s+·\s+\d+\s+pending\s+·\s+\d+\s+done/.test(body) ||
          body.includes('No jobs in the queue')
        );
      },
      { timeout: 10_000, timeoutMsg: 'expected Ctrl/Cmd+J to open the queue' },
    );
    await browser.keys(['']);
  });

  it('NFR-034: documents every shortcut it implements', async () => {
    const body = await (await $('body')).getText();
    expect(body).toContain('Keyboard shortcuts');
    for (const action of [
      'Start a new print job',
      'Print the current job',
      'Switch to the next profile',
      'Open the queue',
      'Cancel the job being printed',
    ]) {
      expect(body).toContain(action);
    }
  });

  it('NFR-041: the quick actions are focusable buttons, not decorative divs', async () => {
    const folder = await $('button*=Print a folder');
    await expect(folder).toBeExisting();
    await expect(folder).toBeClickable();
    // The drop zone is a control too, so a keyboard user can start a job.
    expect(await (await $$('button')).length).toBeGreaterThan(5);
  });

  it('shows live recent activity instead of a disguised failure', async () => {
    const body = await (await $('body')).getText();
    // Whatever the runner's history is, the card must not be stuck loading nor
    // reporting a read failure against a real backend.
    expect(body).not.toContain('Loading recent activity…');
    expect(body).not.toContain('Recent activity could not be read');
  });
});
