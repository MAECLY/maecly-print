import { browser, $, $$, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * Printers screen, end to end against the real Windows spooler.
 *
 * This lane exists for the two things Vitest cannot reach: that
 * `list_printers` really talks to the OS (the CI runner always has "Microsoft
 * Print to PDF"), and that the add-printer modal's platform branch is driven by
 * the OS the *backend* reports — the component tests can only mock `app_info`.
 *
 * Nothing here installs or removes a spooler queue: mutating the runner's
 * printers would leak state between jobs, and `add_printer` is covered by the
 * Rust-side tests plus the component tests.
 */
describe('Printers', () => {
  it('enumerates the host spooler instead of rendering mock data', async () => {
    await dismissOnboarding();
    await navigateTo('Printers');

    // The subtitle is "{n} configured · {n} ready" once enumeration resolves;
    // while it is running it reads "Detecting printers…".
    await browser.waitUntil(
      async () => {
        const body = await (await $('body')).getText();
        return /configured\s+·/.test(body);
      },
      {
        timeout: 20_000,
        timeoutMsg: 'expected list_printers to resolve into the subtitle',
      },
    );

    const body = await (await $('body')).getText();
    // The Windows runner always has at least the virtual PDF printer, so an
    // empty state here means enumeration silently returned nothing.
    expect(body).not.toContain('No printers installed');
    expect(body).not.toContain('Printers unavailable');
    // Capabilities come from DeviceCapabilities, not from a fixture.
    expect(body).toContain('Capabilities');
  });

  it('offers the add-printer methods as tabs and honours the Windows transport limit', async () => {
    await dismissOnboarding();
    await navigateTo('Printers');

    await (
      await $('button*=Add printer')
    ).waitForClickable({
      timeout: 20_000,
    });
    await (await $('button*=Add printer')).click();

    const dialog = await $('[role="dialog"]');
    await dialog.waitForExist({ timeout: 10_000 });

    // The method switcher is a real tablist (it swaps the visible panel), so
    // the three methods expose the `tab` role and exactly one is selected.
    const tabs = await $$('[role="tab"]');
    expect(tabs.length).toBe(3);
    const selected = await $('[role="tab"][aria-selected="true"]');
    expect(await selected.getText()).toBe('Scan network');

    // Windows has no WSD/mDNS discovery yet: the panel must say so rather than
    // spin and then claim the network is empty.
    const panel = await $('[role="tabpanel"]');
    expect(await panel.getText()).toContain(
      'Network discovery is not available',
    );

    // By IP address: the backend reports `os = "windows"`, so raw/LPD are not
    // offered — the spooler would reject them.
    await (await $('[role="tab"]*=By IP address')).click();
    const options = await $$('[role="tabpanel"] select option');
    const labels: string[] = [];
    for (const option of options) labels.push(await option.getText());
    expect(labels).toContain('IPP');
    expect(labels).toContain('IPPS (secure)');
    expect(labels).not.toContain('LPD / LPR');
  });
});
