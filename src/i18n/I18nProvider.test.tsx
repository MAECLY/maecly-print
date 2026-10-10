import { render, screen, act, waitFor } from '@testing-library/react';
import { describe, it, expect, beforeEach, vi } from 'vitest';

import { fixtureAppSettings } from '@/test/fixtures';
import { useSettingsStore } from '@/stores/settingsStore';

import { I18nProvider, useT, useLocale } from './I18nProvider';
import { getActiveLocale, type Locale } from './index';

/** Point `navigator.language` at `tag` for the duration of a test. */
function setBrowserLanguage(tag: string) {
  Object.defineProperty(window.navigator, 'language', {
    configurable: true,
    value: tag,
  });
}

function Probe() {
  const t = useT('common');
  const locale = useLocale();
  return (
    <div>
      <span data-testid="locale">{locale}</span>
      <span data-testid="cancel">{t('actions.cancel')}</span>
    </div>
  );
}

/** Settings as the store holds them once `get_app_settings` resolved. */
function loaded(locale: string) {
  useSettingsStore.setState({
    status: 'ready',
    settings: {
      ...fixtureAppSettings,
      app: { ...fixtureAppSettings.app, locale },
    },
    error: undefined,
  });
}

describe('I18nProvider', () => {
  beforeEach(() => {
    window.localStorage.removeItem('pluryprint:locale-detected');
    setBrowserLanguage('en-US');
    useSettingsStore.setState({
      status: 'idle',
      settings: null,
      error: undefined,
    });
  });

  it('FR-094: re-renders in Spanish when settings.app.locale changes', async () => {
    loaded('en-US');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getByTestId('cancel')).toHaveTextContent('Cancel');

    act(() => loaded('es-ES'));
    await waitFor(() =>
      expect(screen.getByTestId('cancel')).toHaveTextContent('Cancelar'),
    );
    expect(screen.getByTestId('locale')).toHaveTextContent('es-ES');
  });

  it('FR-094: keeps <html lang> and the non-React locale in step with the UI', async () => {
    loaded('en-US');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await waitFor(() => expect(document.documentElement.lang).toBe('en-US'));

    act(() => loaded('es-ES'));
    await waitFor(() => expect(document.documentElement.lang).toBe('es-ES'));
    expect(getActiveLocale()).toBe('es-ES');
  });

  it('FR-093: uses the OS language while no config is loaded', () => {
    setBrowserLanguage('es-MX');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getByTestId('locale')).toHaveTextContent('es-ES');
    expect(screen.getByTestId('cancel')).toHaveTextContent('Cancelar');
  });

  it('FR-093: an unsupported OS language falls back to English', () => {
    setBrowserLanguage('de-DE');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getByTestId('locale')).toHaveTextContent('en-US');
  });

  it('FR-093: first run persists the detected locale over the backend default', async () => {
    const patch = vi.fn(async () => true);
    setBrowserLanguage('es-ES');
    useSettingsStore.setState({ patch });
    loaded('en-US');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await waitFor(() =>
      expect(patch).toHaveBeenCalledWith({ app: { locale: 'es-ES' } }),
    );
  });

  it('FR-093: a refused first-run write leaves detection to run again', async () => {
    const patch = vi.fn(async () => false);
    setBrowserLanguage('es-ES');
    useSettingsStore.setState({ patch });
    loaded('en-US');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await waitFor(() => expect(patch).toHaveBeenCalledTimes(1));
    await act(async () => {
      await Promise.resolve();
    });
    expect(
      window.localStorage.getItem('pluryprint:locale-detected'),
    ).toBeNull();
  });

  it('FR-093: a saved first-run write marks detection as done', async () => {
    const patch = vi.fn(async () => true);
    setBrowserLanguage('es-ES');
    useSettingsStore.setState({ patch });
    loaded('en-US');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await waitFor(() =>
      expect(window.localStorage.getItem('pluryprint:locale-detected')).toBe(
        '1',
      ),
    );
  });

  it('FR-093: no detection write while config.toml is unreadable', async () => {
    const patch = vi.fn(async () => true);
    setBrowserLanguage('es-ES');
    useSettingsStore.setState({
      patch,
      status: 'ready',
      settings: {
        ...fixtureAppSettings,
        meta: {
          config_unreadable: true,
          config_path: '/tmp/config.toml',
          limits: {
            max_concurrent_conversions: {
              min: 1,
              max: 8,
              default: 2,
              zero: 'clamp',
            },
            conversion_timeout_seconds: {
              min: 10,
              max: 3600,
              default: 60,
              zero: 'disables',
            },
            max_file_size_mb: {
              min: 1,
              max: 4096,
              default: 200,
              zero: 'default',
            },
            print_timeout_seconds: {
              min: 10,
              max: 3600,
              default: 300,
              zero: 'disables',
            },
          },
        },
      },
    });
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(patch).not.toHaveBeenCalled();
    expect(
      window.localStorage.getItem('pluryprint:locale-detected'),
    ).toBeNull();
  });

  it('FR-094: a stored locale is never overwritten by the OS language', async () => {
    const patch = vi.fn(async () => true);
    setBrowserLanguage('es-ES');
    useSettingsStore.setState({ patch });
    // The user explicitly chose English in Settings; detection must stand down.
    loaded('en-US');
    window.localStorage.setItem('pluryprint:locale-detected', '1');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await waitFor(() =>
      expect(screen.getByTestId('locale')).toHaveTextContent('en-US'),
    );
    expect(patch).not.toHaveBeenCalled();
  });

  it('FR-092: an unsupported stored locale falls back rather than breaking', () => {
    setBrowserLanguage('en-US');
    loaded('fr-FR');
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    const locale: Locale = 'en-US';
    expect(screen.getByTestId('locale')).toHaveTextContent(locale);
  });
});
