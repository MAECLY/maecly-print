import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { version as REACT_VERSION } from 'react';
import tauriPkg from '@tauri-apps/api/package.json';

import { AboutScreen } from './AboutScreen';
import { MPSidebar } from '@/components/AppShell/MPSidebar';
import { Toaster } from '@/components/AppShell/Toaster';
import {
  DOCS_URL,
  LICENSE_URL,
  RELEASES_URL,
  REPO_URL,
  UNKNOWN_VERSION,
} from '@/lib/repo';
import {
  fixtureAppInfo,
  fixtureAppSettings,
  fixtureUpdateCheckAvailable,
  fixtureUpdateCheckUpToDate,
} from '@/test/fixtures';
import type { AppSettings, UpdateCheck } from '@/lib/ipc';

// A few assertions read files instead of rendering: they guard the repository
// slug and the "no hardcoded version" rule at the source level, which no amount
// of rendering can catch. Vitest runs from the project root.
const REPO_ROOT = process.cwd();

// Mock the settings store so these tests own only the About screen. The
// channel suffix reads `settings.updater.channel`, so we drive `settings`
// directly rather than through the real store's IPC path.
const { settingsState } = vi.hoisted(() => ({
  settingsState: {
    settings: null as AppSettings | null,
    init: () => Promise.resolve(),
  },
}));

vi.mock('@/stores/settingsStore', () => ({
  useSettingsStore: (selector: (s: typeof settingsState) => unknown) =>
    selector(settingsState),
}));

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

interface MockOpts {
  /** Override the `app_info` payload. */
  appInfo?: unknown;
  /** Return value for `check_for_updates`. */
  update?: unknown;
  /** When set, `check_for_updates` throws with this message. */
  updateError?: string;
}

/** Install IPC mocks (which also flags the Tauri host) and record the calls. */
function mockAbout(opts: MockOpts = {}): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === 'app_info') return opts.appInfo ?? fixtureAppInfo;
    if (cmd === 'check_for_updates') {
      if (opts.updateError) throw new Error(opts.updateError);
      return opts.update ?? fixtureUpdateCheckUpToDate;
    }
    // open_external / open_logs_folder / report_issue resolve with null.
    return null;
  });
  return calls;
}

/** Render with a mounted Toaster so any error-path toast has somewhere to land. */
function renderAbout() {
  return render(
    <>
      <AboutScreen />
      <Toaster theme="light" />
    </>,
  );
}

/** The version line only shows once `app_info` has resolved. */
function waitForAppInfo() {
  return screen.findByText(/build 2026-07-01/);
}

describe('AboutScreen', () => {
  beforeEach(() => {
    settingsState.settings = null;
    // clearMocks() only strips the invoke fn but leaves the __TAURI_INTERNALS__
    // shell on window, which keeps isTauri() true. Remove the key so a test
    // that installs no IPC mock sees a genuinely host-less window.
    delete (window as unknown as Record<string, unknown>)[
      '__TAURI_INTERNALS__'
    ];
  });
  afterEach(() => clearMocks());

  it('FR-110: shows the version and opens the license and docs links', async () => {
    const calls = mockAbout();
    renderAbout();

    // Version line is sourced from app_info (version · build date).
    expect(await waitForAppInfo()).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /license gpl-3\.0/i }));
    fireEvent.click(screen.getByRole('button', { name: /documentation/i }));

    await waitFor(() => {
      const urls = calls
        .filter((c) => c.cmd === 'open_external')
        .map((c) => c.args.url);
      expect(urls).toContain(LICENSE_URL);
      expect(urls).toContain(DOCS_URL);
    });
  });

  it('renders the standalone fallback version line and disables update actions outside Tauri', async () => {
    // No mockIPC → not a Tauri host: fetchAppInfo rejects, actions are disabled.
    renderAbout();

    expect(
      await screen.findByText(/standalone web preview/),
    ).toBeInTheDocument();
    expect(await screen.findByText(/IPC unavailable/i)).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /check for updates/i }),
    ).toBeDisabled();
  });

  it('checks for updates: shows a checking state, then an up-to-date card with the checked time', async () => {
    mockAbout({ update: fixtureUpdateCheckUpToDate });
    renderAbout();
    await waitForAppInfo();

    fireEvent.click(screen.getByRole('button', { name: /check for updates/i }));

    // Transient checking state (spinner + disabled button).
    expect(screen.getByRole('button', { name: /checking/i })).toBeDisabled();

    // Resolves to a real up-to-date card with the formatted checked_at time.
    expect(await screen.findByText(/up to date/i)).toBeInTheDocument();
    expect(screen.getByText(/checked \d{2}:\d{2}/)).toBeInTheDocument();
    expect(screen.getByText(/version 1\.0\.0 · checked/i)).toBeInTheDocument();
  });

  it('shows an update-available card and opens the specific release notes URL', async () => {
    const calls = mockAbout({ update: fixtureUpdateCheckAvailable });
    renderAbout();
    await waitForAppInfo();

    fireEvent.click(screen.getByRole('button', { name: /check for updates/i }));

    const card = await screen.findByRole('group', {
      name: /update available/i,
    });
    expect(
      within(card).getByText(/version 9\.9\.9 is available/i),
    ).toBeInTheDocument();

    fireEvent.click(
      within(card).getByRole('button', { name: /release notes/i }),
    );

    await waitFor(() => {
      const ext = calls.find((c) => c.cmd === 'open_external');
      expect(ext?.args.url).toBe(fixtureUpdateCheckAvailable.url);
    });
  });

  it('surfaces the thrown update-check error inline, and retry re-runs the check', async () => {
    const calls = mockAbout({
      updateError: 'GitHub rate limit reached — try again in a bit.',
    });
    renderAbout();
    await waitForAppInfo();

    fireEvent.click(screen.getByRole('button', { name: /check for updates/i }));
    expect(
      await screen.findByText(/github rate limit reached/i),
    ).toBeInTheDocument();

    const before = calls.filter((c) => c.cmd === 'check_for_updates').length;
    fireEvent.click(screen.getByRole('button', { name: /retry/i }));
    await waitFor(() =>
      expect(
        calls.filter((c) => c.cmd === 'check_for_updates').length,
      ).toBeGreaterThan(before),
    );
  });

  it('treats "no releases yet" (available:false, latest:null) as an honest up-to-date state', async () => {
    const noReleases: UpdateCheck = {
      current: '1.0.0',
      latest: null,
      available: false,
      url: null,
      notes: null,
      checked_at: '2026-07-11T10:00:00Z',
    };
    mockAbout({ update: noReleases });
    renderAbout();
    await waitForAppInfo();

    fireEvent.click(screen.getByRole('button', { name: /check for updates/i }));

    expect(await screen.findByText(/up to date/i)).toBeInTheDocument();
    expect(screen.queryByText(/available/i)).not.toBeInTheDocument();
    expect(
      screen.queryByText(/rate limit|could not reach/i),
    ).not.toBeInTheDocument();
  });

  it('files an issue and opens the logs folder through IPC', async () => {
    const calls = mockAbout();
    renderAbout();
    await waitForAppInfo();

    fireEvent.click(screen.getByRole('button', { name: /report an issue/i }));
    fireEvent.click(screen.getByRole('button', { name: /open logs folder/i }));

    await waitFor(() => {
      expect(calls.some((c) => c.cmd === 'report_issue')).toBe(true);
      expect(calls.some((c) => c.cmd === 'open_logs_folder')).toBe(true);
    });
  });

  it('lists the real React and Tauri versions in the components card', async () => {
    mockAbout();
    renderAbout();
    await waitForAppInfo();

    // Sourced from react's runtime `version` export and @tauri-apps/api's
    // own package.json — asserted against the very values the app imports.
    expect(screen.getByText(REACT_VERSION)).toBeInTheDocument();
    expect(screen.getByText(tauriPkg.version)).toBeInTheDocument();
    // The removed bundled-tooling rows must be gone until they are real.
    expect(screen.queryByText(/SumatraPDF|LibreOffice|ImageMagick/)).toBeNull();
  });

  it('does not render the removed hardcoded "12 minutes ago" placeholder', async () => {
    mockAbout();
    renderAbout();
    await waitForAppInfo();

    expect(screen.queryByText(/12 minutes/i)).toBeNull();
    expect(screen.queryByText(/last checked/i)).toBeNull();
  });

  it('appends the update channel from settings to the version line', async () => {
    settingsState.settings = fixtureAppSettings; // channel: 'stable'
    mockAbout();
    renderAbout();

    expect(await screen.findByText(/stable channel/i)).toBeInTheDocument();
  });

  it('opens the releases page from the persistent Release notes link', async () => {
    const calls = mockAbout();
    renderAbout();
    await waitForAppInfo();

    fireEvent.click(screen.getByRole('button', { name: /release notes/i }));
    await waitFor(() =>
      expect(
        calls.some(
          (c) => c.cmd === 'open_external' && c.args.url === RELEASES_URL,
        ),
      ).toBe(true),
    );
  });

  it('opens the source repository from the footer link', async () => {
    const calls = mockAbout();
    renderAbout();
    await waitForAppInfo();

    fireEvent.click(
      screen.getByRole('button', {
        name: /github\.com\/MAECLY\/maecly-print/i,
      }),
    );
    await waitFor(() =>
      expect(
        calls.some((c) => c.cmd === 'open_external' && c.args.url === REPO_URL),
      ).toBe(true),
    );
  });

  it('FR-110: renders the version app_info reports, not a frontend literal', async () => {
    // A version deliberately unlike anything written anywhere in the tree: it
    // can only reach the screen through the backend payload.
    mockAbout({ appInfo: { ...fixtureAppInfo, version: '4.5.6' } });
    renderAbout();

    expect(await screen.findByText(/^4\.5\.6 · build/)).toBeInTheDocument();
    // …and the components card echoes the same value.
    expect(screen.getByText('4.5.6')).toBeInTheDocument();
    // The retired hardcoded fallbacks must not reappear.
    expect(screen.queryByText(/0\.0\.1/)).toBeNull();
  });

  it('shows the unknown-version placeholder, not a version number, without a backend', async () => {
    // No mockIPC → fetchAppInfo rejects; there is no version to report.
    renderAbout();

    expect(
      await screen.findByText(`${UNKNOWN_VERSION} · standalone web preview`),
    ).toBeInTheDocument();
    // The MaeclyPrint row of the components card falls back to the same
    // placeholder rather than repeating a stale number.
    expect(screen.queryByText(/\d+\.\d+\.\d+-dev/)).toBeNull();
  });
});

/**
 * The sidebar footer used to print a hand-written `v0.0.1`, which would have
 * lied from the first release bump onwards. These live here, next to the About
 * assertions, because both surfaces read the same authoritative `app_info`.
 */
describe('MPSidebar version footer', () => {
  beforeEach(() => {
    delete (window as unknown as Record<string, unknown>)[
      '__TAURI_INTERNALS__'
    ];
  });
  afterEach(() => clearMocks());

  it('renders the version reported by app_info', async () => {
    mockIPC((cmd) => (cmd === 'app_info' ? fixtureAppInfo : null));
    render(<MPSidebar active="home" />);

    expect(await screen.findByText(`v${fixtureAppInfo.version}`)).toBeVisible();
    expect(screen.getByText(/connected/)).toBeInTheDocument();
  });

  it('shows the placeholder while app_info is still in flight', () => {
    mockIPC((cmd) => (cmd === 'app_info' ? fixtureAppInfo : null));
    render(<MPSidebar active="home" />);

    // Synchronously after mount the invoke has not resolved: no version yet,
    // and the status dot says so instead of claiming a live backend.
    expect(screen.getByText(UNKNOWN_VERSION)).toBeInTheDocument();
    expect(screen.getByText(/connecting/)).toBeInTheDocument();
  });

  it('falls back to the placeholder outside the Tauri host', async () => {
    render(<MPSidebar active="home" />);

    expect(await screen.findByText(/no backend/)).toBeInTheDocument();
    expect(screen.getByText(UNKNOWN_VERSION)).toBeInTheDocument();
  });

  it('regression: no version number is written by hand in the sidebar source', () => {
    const source = readFileSync(
      join(REPO_ROOT, 'src/components/AppShell/MPSidebar.tsx'),
      'utf8',
    );
    // Any `x.y.z` literal in this file is a version being duplicated: the only
    // authoritative source is `app_info().version` (Rust `CARGO_PKG_VERSION`).
    expect(source).not.toMatch(/\d+\.\d+\.\d+/);
  });
});

/**
 * Repository identity. The slug lives in exactly two places — `repository` in
 * `src-tauri/Cargo.toml` (what Rust reads for the update check and the issue
 * URL) and `REPO_URL` here — so these assertions keep them from drifting and
 * keep the derived links pointing at paths that really exist.
 */
describe('project identity (src/lib/repo.ts)', () => {
  it('mirrors the repository configured in src-tauri/Cargo.toml', () => {
    const cargo = readFileSync(join(REPO_ROOT, 'src-tauri/Cargo.toml'), 'utf8');
    const configured = /^repository\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
    expect(configured).toBe(REPO_URL);
    // …and it is the real remote, not a repository that does not exist.
    expect(REPO_URL).toBe('https://github.com/MAECLY/maecly-print');
  });

  it('derives every link from the one slug', () => {
    for (const url of [RELEASES_URL, LICENSE_URL, DOCS_URL]) {
      expect(url.startsWith(`${REPO_URL}/`)).toBe(true);
    }
  });

  it('points the blob/tree links at paths that exist on the default branch', () => {
    // `git symbolic-ref refs/remotes/origin/HEAD` → refs/remotes/origin/main.
    for (const url of [LICENSE_URL, DOCS_URL]) {
      const [, kind, branch, ...rest] = url.slice(REPO_URL.length).split('/');
      expect(kind).toMatch(/^(blob|tree)$/);
      expect(branch).toBe('main');
      expect(existsSync(join(REPO_ROOT, rest.join('/')))).toBe(true);
    }
  });
});
