import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, afterEach } from 'vitest';

import { ErrorBoundary } from './ErrorBoundary';

function Boom(): JSX.Element {
  throw new Error('render exploded');
}

describe('ErrorBoundary', () => {
  afterEach(() => vi.restoreAllMocks());

  it('NFR-034: a throwing child renders the panel instead of a blank window', () => {
    // React logs the caught error itself; silence it so the run stays readable.
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(
      screen.getByText('The app hit an unexpected error'),
    ).toBeInTheDocument();
    // The thrown message is surfaced so a report can name it.
    expect(screen.getByText('render exploded')).toBeInTheDocument();
  });

  it('NFR-034: renders children untouched when nothing throws', () => {
    render(
      <ErrorBoundary>
        <span>all good</span>
      </ErrorBoundary>,
    );
    expect(screen.getByText('all good')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('NFR-034: the reload button reloads the app', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const onReload = vi.fn();
    render(
      <ErrorBoundary onReload={onReload}>
        <Boom />
      </ErrorBoundary>,
    );
    fireEvent.click(screen.getByText('Reload the app'));
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it('NFR-034: the logs-folder button is desktop-only', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    // Outside Tauri there is no logs folder to reveal, so the button is hidden.
    expect(screen.queryByText('Open the logs folder')).not.toBeInTheDocument();
  });
});
