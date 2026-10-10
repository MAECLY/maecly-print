import { readFileSync } from 'node:fs';

import { browser, $, $$, expect } from '@wdio/globals';

import {
  byText,
  dismissOnboarding,
  navigateTo,
  visibleText,
} from './helpers.js';

/**
 * The English Home catalog the app renders (the wdio `before` hook forces
 * English). Asserting against it rather than against copied sentences keeps
 * this spec about the *wiring* — the strings themselves, and that the format
 * list matches the engine, are unit-tested (`HomeScreen.test.tsx`).
 */
const HOME = JSON.parse(
  readFileSync(
    new URL('../../src/i18n/locales/en/home.json', import.meta.url),
    'utf8',
  ),
) as {
  dropzone: { title: string; formatList: string };
  shortcuts: Record<string, string>;
};

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
    // The drop zone renders the catalog's format list — which a unit test
    // holds equal to the engine's SUPPORTED_EXTS — and nothing else.
    const zone = await (await byText(HOME.dropzone.title)).parentElement();
    const text = await zone.getText();
    expect(text).toContain(HOME.dropzone.formatList);
    // GIF is refused on purpose (decision D9): never promised.
    expect(text.toLowerCase()).not.toContain('gif');
  });

  it('does not claim the shell right-click integration already exists', async () => {
    const body = await (await $('body')).getText();
    expect(body).not.toContain('adds itself to the right-click menu');
    // The CLI, which this build really ships, is what the tip points at. The
    // command names the console program by its full (maybe quoted) path when
    // it is not on PATH, e.g. `C:\...\pluryprint-cli.exe print --files`.
    expect(body).toMatch(/pluryprint(-cli)?(\.exe)?'? print --files/);
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
    // Rendered text: the heading is CSS-uppercased, so compare lower-cased.
    const body = await visibleText();
    expect(body).toContain(HOME.shortcuts.label.toLowerCase());
    for (const key of [
      'newJob',
      'printFolder',
      'print',
      'switchProfile',
      'openQueue',
      'cancel',
    ]) {
      expect(body).toContain(String(HOME.shortcuts[key]).toLowerCase());
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
