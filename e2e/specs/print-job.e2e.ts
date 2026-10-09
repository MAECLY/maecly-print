import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { browser, $, expect } from '@wdio/globals';

import { dismissOnboarding, navigateTo } from './helpers.js';

/**
 * Print Job flow: assemble a batch, tune a setting, submit, and see the job
 * land in the Queue.
 *
 * These run against the real app on Windows CI (tauri-driver), never on macOS —
 * WKWebView exposes no WebDriver endpoint — so the selectors prefer visible
 * text, which survives markup churn.
 *
 * What is deliberately NOT asserted: that anything reached paper. Print
 * correctness belongs to the Rust integration layer against a virtual printer
 * (docs/08-development/testing-strategy.md § Print mock harness). This spec
 * covers the GUI → IPC → queue glue, which nothing below it can cover.
 *
 * Files reach the batch through the drag-drop event rather than the OS picker:
 * a native dialog is not drivable from WebDriver, while the drop is the very
 * path FR-007 specifies. The fixtures are written from the spec's own Node
 * context (wdio specs run in Node), so nothing binary lives in the repo.
 */

/** A minimal but structurally valid one-page PDF. */
const PDF = [
  '%PDF-1.4',
  '1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj',
  '2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj',
  '3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj',
  'trailer<</Root 1 0 R>>',
  '%%EOF',
  '',
].join('\n');

let fixtures: string;

/**
 * Replay a native file drop onto the window.
 *
 * `getCurrentWebview().onDragDropEvent` — what both the Print Job screen and
 * the app shell subscribe to — listens for `tauri://drag-drop`, so emitting
 * that event with the documented payload drives the production code path
 * rather than a test-only hook.
 *
 * The emit goes through `window.__TAURI_INTERNALS__.invoke` — the very call
 * `@tauri-apps/api/event`'s `emit` makes — because a script injected by
 * WebDriver cannot `import('@tauri-apps/api/event')`: the built app is a
 * bundle, so a bare module specifier does not resolve inside the webview.
 */
async function drop(paths: string[]): Promise<void> {
  await invokeIpc('plugin:event|emit', {
    event: 'tauri://drag-drop',
    payload: { paths, position: { x: 100, y: 100 } },
  });
}

/** Call a Tauri command from the page, through the host's own IPC bridge. */
async function invokeIpc(
  cmd: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  return browser.execute(
    async (c: string, a: Record<string, unknown>) => {
      const internals = (
        window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (
              cmd: string,
              args: Record<string, unknown>,
            ) => Promise<unknown>;
          };
        }
      ).__TAURI_INTERNALS__;
      return internals.invoke(c, a);
    },
    cmd,
    args,
  );
}

describe('Print Job', () => {
  before(() => {
    fixtures = mkdtempSync(join(tmpdir(), 'maeclyprint-e2e-'));
    writeFileSync(join(fixtures, 'report.pdf'), PDF, 'utf8');
    writeFileSync(join(fixtures, 'notes.txt'), 'not printable', 'utf8');
  });

  // The submit below goes to the host's real default printer. On a developer
  // machine that is paper; on a runner it is often "Microsoft Print to PDF",
  // which opens a Save dialog nobody answers and holds the single print slot
  // until the print timeout. Pause the whole queue first (FR-033) so the job
  // is created and listed but never spooled, then cancel it and resume.
  before(async () => {
    await invokeIpc('pause_queue');
  });

  after(async () => {
    await invokeIpc('cancel_all').catch(() => undefined);
    await invokeIpc('resume_queue').catch(() => undefined);
  });

  it('adds a dropped file, submits the batch and shows it in the Queue', async () => {
    await dismissOnboarding();
    await navigateTo('Print Job');

    // Print is disabled until the batch holds at least one file — the button
    // reflects validity, it does not fail on click (FR-042).
    await expect($('button*=Print 0')).toBeDisabled();

    await drop([join(fixtures, 'report.pdf')]);

    const row = await $('*=report.pdf');
    await row.waitForExist({ timeout: 10_000 });

    // Change one setting so the submitted request is not just the profile
    // baseline, then wait for the honesty preview (debounced 300 ms) to catch
    // up — what the user sees must be what the backend runs.
    await (await $('button*=B&W')).click();
    await (await $('*=Resolved command')).waitForExist({ timeout: 10_000 });

    await (await $('button*=Print 1')).click();

    // A successful submit navigates to the Queue on its own; navigating again
    // is harmless and makes the spec independent of that behaviour.
    await navigateTo('Queue');
    await (await $('*=report.pdf')).waitForExist({ timeout: 15_000 });
  });

  it('FR-005: refuses an unsupported file with a visible reason', async () => {
    await navigateTo('Print Job');

    await drop([join(fixtures, 'notes.txt')]);

    // The rejection is grouped into one toast naming the file and the reason —
    // never a silent skip (FR-059).
    await (
      await $('*=Unsupported file type')
    ).waitForExist({
      timeout: 10_000,
    });
    await expect($('*=notes.txt')).toBeExisting();
  });

  it('FR-004: asks whether to recurse before walking a dropped folder', async () => {
    await navigateTo('Print Job');

    // A folder that holds a subfolder: the question is mandatory, the backend
    // never guesses the depth.
    const root = mkdtempSync(join(tmpdir(), 'maeclyprint-e2e-tree-'));
    writeFileSync(join(root, 'top.pdf'), PDF, 'utf8');
    const nested = mkdtempSync(join(root, 'deep-'));
    writeFileSync(join(nested, 'inner.pdf'), PDF, 'utf8');

    await drop([root]);

    const topLevel = await $('button*=Top level only');
    await topLevel.waitForExist({ timeout: 10_000 });
    await expect($('button*=Include subfolders')).toBeExisting();

    await topLevel.click();

    // Top level only: the file beside the subfolder is in, the one inside it
    // is not.
    await (await $('*=top.pdf')).waitForExist({ timeout: 10_000 });
    await expect($('*=inner.pdf')).not.toBeExisting();
  });
});
