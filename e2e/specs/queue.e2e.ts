import { browser, $, $$, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * Queue screen, end to end against the real backend queue.
 *
 * This lane exists for the two things Vitest cannot reach: that the screen is
 * wired to a LIVE `queue_status` (the component tests mock the whole IPC layer,
 * so an "unavailable" regression would still pass there), and that the three
 * views really swap in the running app — the view state lives in `App`, above
 * the screen, so only a full app run exercises the round trip.
 *
 * Nothing here submits or clears jobs: mutating the runner's queue would leak
 * state into the other specs, and every mutation is covered by the Rust
 * dispatcher tests plus the component tests.
 */
describe('Queue', () => {
  it('renders the live queue instead of an unavailable notice', async () => {
    await dismissOnboarding();
    await navigateTo('Queue');

    // The List subtitle is "{n} active · {n} pending · {n} done" once
    // `queue_status` resolves.
    await browser.waitUntil(
      async () => {
        const body = await (await $('body')).getText();
        return /active\s+·\s+\d+\s+pending\s+·\s+\d+\s+done/.test(body);
      },
      {
        timeout: 20_000,
        timeoutMsg: 'expected queue_status to resolve into the subtitle',
      },
    );

    const body = await (await $('body')).getText();
    // Any of these means the store never reached `ready` against a real host.
    expect(body).not.toContain('Queue unavailable');
    expect(body).not.toContain('Loading the queue…');
  });

  it('switches between the List, Board and Timeline views', async () => {
    await dismissOnboarding();
    await navigateTo('Queue');

    // The switcher follows the ARIA radio-group pattern (see MPSegmented).
    const options = await $$('[role="radiogroup"] [role="radio"]');
    expect(options.length).toBe(3);

    await (await $('[role="radio"]*=Board')).click();
    await browser.waitUntil(
      async () =>
        (await (await $('body')).getText()).includes(
          'Board view — kanban across job phases',
        ),
      { timeout: 5_000, timeoutMsg: 'expected the Board subtitle' },
    );
    // Board and Timeline are read-only: the whole-queue controls stay in List.
    expect(await (await $('button*=Pause queue')).isExisting()).toBe(false);

    await (await $('[role="radio"]*=Timeline')).click();
    await browser.waitUntil(
      async () =>
        (await (await $('body')).getText()).includes(
          'Timeline — chronological, with durations',
        ),
      { timeout: 5_000, timeoutMsg: 'expected the Timeline subtitle' },
    );

    // Back to List: the queue controls come back.
    await (await $('[role="radio"]*=List')).click();
    await (await $('button*=Pause queue')).waitForExist({ timeout: 5_000 });
  });

  it('offers the queue controls and keeps Clear done honest', async () => {
    await dismissOnboarding();
    await navigateTo('Queue');

    const pause = await $('button*=Pause queue');
    await pause.waitForExist({ timeout: 20_000 });

    // "Clear done" is enabled only when there IS history to clear, so its
    // disabled state must agree with the "done" count in the subtitle.
    const body = await (await $('body')).getText();
    const done = Number(/(\d+)\s+done/.exec(body)?.[1] ?? '0');
    const clear = await $('button*=Clear done');
    expect(await clear.isEnabled()).toBe(done > 0);
  });
});
