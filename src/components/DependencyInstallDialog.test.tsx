import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider } from '@/i18n/I18nProvider';
import type {
  DependencyReport,
  InstallOutcome,
  InstallOption,
} from '@/ipc/bindings';
import { useSettingsStore } from '@/stores/settingsStore';
import { fixtureAppSettings } from '@/test/fixtures';

import {
  DependencyInstallDialog,
  type InstallProgressSource,
} from './DependencyInstallDialog';

const WINGET: InstallOption = {
  method: 'winget',
  command:
    'winget install --id TheDocumentFoundation.LibreOffice --exact --source winget --accept-package-agreements --accept-source-agreements --disable-interactivity',
  needs_admin: true,
  paid: false,
  approx_mb: 350,
  available: true,
};

const WEB: InstallOption = {
  method: 'web',
  command: 'https://www.libreoffice.org/download/download-libreoffice/',
  needs_admin: false,
  paid: false,
  approx_mb: null,
  available: true,
};

function office(install: InstallOption[]): DependencyReport {
  return {
    id: 'libre-office',
    name: 'LibreOffice',
    applicable: true,
    found: false,
    path: null,
    found_in: null,
    blocks: ['docx'],
    remediation: 'LibreOffice is missing — …',
    install,
  };
}

const noProgress: InstallProgressSource = () => Promise.resolve(() => {});

afterEach(() => {
  useSettingsStore.setState({ settings: fixtureAppSettings });
});

describe('DependencyInstallDialog', () => {
  it('FR-058: shows the exact command, the UAC prompt and the size before anything runs', () => {
    const run = vi.fn();
    render(
      <DependencyInstallDialog
        report={office([WINGET, WEB])}
        onClose={() => {}}
        onResolved={() => {}}
        run={run}
        progress={noProgress}
      />,
    );
    expect(
      screen.getByRole('dialog', { name: 'Install LibreOffice' }),
    ).toBeInTheDocument();
    expect(screen.getByTestId('install-command')).toHaveTextContent(
      WINGET.command,
    );
    expect(screen.getByText(/administrator permission \(UAC\)/)).toBeVisible();
    expect(screen.getByText(/about 350 MB/)).toBeVisible();
    expect(screen.getByText(/Nothing is installed until/)).toBeVisible();
    // Opening the dialog is not consent.
    expect(run).not.toHaveBeenCalled();
  });

  it('FR-058: install_requires_consent — only the Install button runs it, Cancel does not', async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    const onClose = vi.fn();
    render(
      <DependencyInstallDialog
        report={office([WINGET, WEB])}
        onClose={onClose}
        onResolved={() => {}}
        run={run}
        progress={noProgress}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
  });

  it('FR-058: runs the chosen method, streams its output and hands back the fresh report', async () => {
    const user = userEvent.setup();
    let emit: (id: 'libre-office', line: string) => void = () => {};
    const progress: InstallProgressSource = (onLine) => {
      emit = onLine;
      return Promise.resolve(() => {});
    };
    let finish: (o: InstallOutcome) => void = () => {};
    const run = vi.fn(
      () =>
        new Promise<InstallOutcome>((resolve) => {
          finish = resolve;
        }),
    );
    const onResolved = vi.fn();
    render(
      <DependencyInstallDialog
        report={office([WINGET, WEB])}
        onClose={() => {}}
        onResolved={onResolved}
        run={run}
        progress={progress}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Install' }));
    expect(run).toHaveBeenCalledWith('libre-office', 'winget');
    await waitFor(() => expect(screen.getByText('Installing…')).toBeVisible());
    emit('libre-office', 'Successfully installed');
    await screen.findByText(/Successfully installed/);

    const fresh = { ...office([]), found: true, blocks: [] };
    finish({ status: 'installed', exit_code: 0, report: fresh });
    expect(
      await screen.findByText(/LibreOffice is installed/),
    ).toBeInTheDocument();
    expect(onResolved).toHaveBeenCalledWith(fresh);
  });

  it('FR-058: winget_absent_falls_back_web — an unavailable method is disabled and the page is preselected', async () => {
    const user = userEvent.setup();
    const run = vi.fn(() =>
      Promise.resolve<InstallOutcome>({
        status: 'opened',
        exit_code: null,
        report: office([{ ...WINGET, available: false }, WEB]),
      }),
    );
    render(
      <DependencyInstallDialog
        report={office([{ ...WINGET, available: false }, WEB])}
        onClose={() => {}}
        onResolved={() => {}}
        run={run}
        progress={noProgress}
      />,
    );
    const radios = screen.getAllByRole('radio');
    expect(radios[0]).toBeDisabled();
    expect(radios[1]).toBeChecked();
    expect(screen.getByText('Not available on this computer.')).toBeVisible();
    expect(screen.getByTestId('install-command')).toHaveTextContent(
      WEB.command,
    );
    await user.click(screen.getByRole('button', { name: 'Open' }));
    expect(run).toHaveBeenCalledWith('libre-office', 'web');
    expect(
      await screen.findByText(/Finish the installation/),
    ).toBeInTheDocument();
  });

  it('FR-058: a paid Store component says so', () => {
    render(
      <DependencyInstallDialog
        report={{
          ...office([]),
          id: 'hevc-codec',
          name: 'HEVC Video Extensions',
          install: [
            {
              method: 'store',
              command: 'ms-windows-store://pdp/?ProductId=9NMZLZ57R3T7',
              needs_admin: false,
              paid: true,
              approx_mb: null,
              available: true,
            },
          ],
        }}
        onClose={() => {}}
        onResolved={() => {}}
        run={vi.fn()}
        progress={noProgress}
      />,
    );
    expect(screen.getByText(/may cost money/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Open' })).toBeEnabled();
  });

  it('FR-058: remediation_localized_es_en — the dialog speaks Spanish in es-ES', () => {
    useSettingsStore.setState({
      settings: {
        ...fixtureAppSettings,
        app: { ...fixtureAppSettings.app, locale: 'es-ES' },
      },
    });
    render(
      <I18nProvider>
        <DependencyInstallDialog
          report={office([WINGET, WEB])}
          onClose={() => {}}
          onResolved={() => {}}
          run={vi.fn()}
          progress={noProgress}
        />
      </I18nProvider>,
    );
    expect(
      screen.getByRole('dialog', { name: 'Instalar LibreOffice' }),
    ).toBeInTheDocument();
    expect(screen.getByText(/^Falta LibreOffice/)).toBeVisible();
    expect(screen.getByText(/permiso de administrador/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Instalar' })).toBeEnabled();
    expect(screen.queryByText(/administrator permission/)).toBeNull();
  });
});
