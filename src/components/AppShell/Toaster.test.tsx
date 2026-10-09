import { act, render, screen } from '@testing-library/react';
import { toast } from 'sonner';
import { afterEach, describe, expect, it } from 'vitest';

import { I18nProvider } from '@/i18n/I18nProvider';
import { useSettingsStore } from '@/stores/settingsStore';
import { fixtureAppSettings } from '@/test/fixtures';

import { Toaster } from './Toaster';

function spanish() {
  useSettingsStore.setState({
    status: 'ready',
    settings: {
      ...fixtureAppSettings,
      app: { ...fixtureAppSettings.app, locale: 'es-ES' },
    },
    error: undefined,
  });
}

describe('Toaster', () => {
  afterEach(() => {
    toast.dismiss();
    useSettingsStore.setState({ status: 'idle', settings: null });
  });

  it('FR-092: the notification region and the close button follow the UI language', async () => {
    spanish();
    render(
      <I18nProvider>
        <Toaster theme="light" />
      </I18nProvider>,
    );
    act(() => {
      toast('Hola');
    });

    expect(
      await screen.findByRole('button', { name: 'Cerrar notificación' }),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Close toast' })).toBeNull();
    expect(
      screen.getByRole('region', { name: /^Notificaciones/ }),
    ).toBeInTheDocument();
  });
});
