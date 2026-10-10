import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { I18nProvider } from '@/i18n/I18nProvider';
import { useSettingsStore } from '@/stores/settingsStore';
import { fixtureAppSettings } from '@/test/fixtures';

import { ConfirmDialog, type ConfirmState } from './ConfirmDialog';
import { useAnyModalOpen } from './useModal';

const STATE: ConfirmState = {
  title: 'Delete profile',
  message: 'This cannot be undone.',
  confirmLabel: 'Delete',
  run: () => {},
};

/** Mirrors how the screens use the dialog: a trigger owns the open state. */
function Harness({ run = () => {} }: { run?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Remove
      </button>
      {open && (
        <ConfirmDialog
          state={{ ...STATE, run }}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}

describe('ConfirmDialog', () => {
  it('NFR-044: announces its title and body through the dialog role', () => {
    render(<ConfirmDialog state={STATE} onClose={() => {}} />);
    const dialog = screen.getByRole('dialog', { name: 'Delete profile' });

    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAccessibleDescription('This cannot be undone.');
  });

  it('NFR-041: focuses the least destructive action on open', () => {
    render(<ConfirmDialog state={STATE} onClose={() => {}} />);
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
  });

  it('NFR-040: traps Tab and Shift+Tab inside the dialog', async () => {
    const user = userEvent.setup();
    render(<ConfirmDialog state={STATE} onClose={() => {}} />);
    const cancel = screen.getByRole('button', { name: 'Cancel' });
    const confirm = screen.getByRole('button', { name: 'Delete' });

    await user.tab();
    expect(confirm).toHaveFocus();

    await user.tab();
    expect(cancel).toHaveFocus();

    await user.tab({ shift: true });
    expect(confirm).toHaveFocus();
  });

  it('NFR-041: closes with Escape without running the action', async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    render(<Harness run={run} />);

    await user.click(screen.getByRole('button', { name: 'Remove' }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(run).not.toHaveBeenCalled();
  });

  it('NFR-041: returns focus to the trigger once it closes', async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const trigger = screen.getByRole('button', { name: 'Remove' });

    await user.click(trigger);
    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(trigger).toHaveFocus();
  });

  it('runs the action when the confirm button is activated with the keyboard', async () => {
    const user = userEvent.setup();
    const run = vi.fn();
    render(<Harness run={run} />);

    await user.click(screen.getByRole('button', { name: 'Remove' }));
    await user.tab();
    await user.keyboard('{Enter}');

    expect(run).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('FR-092: the Cancel button follows the UI language', () => {
    useSettingsStore.setState({
      status: 'ready',
      settings: {
        ...fixtureAppSettings,
        app: { ...fixtureAppSettings.app, locale: 'es-ES' },
      },
      error: undefined,
    });
    try {
      render(
        <I18nProvider>
          <ConfirmDialog state={STATE} onClose={() => {}} />
        </I18nProvider>,
      );
      expect(
        screen.getByRole('button', { name: 'Cancelar' }),
      ).toBeInTheDocument();
    } finally {
      useSettingsStore.setState({ status: 'idle', settings: null });
    }
  });

  it('NFR-040: covers the whole window and makes the app behind it inert', () => {
    const root = document.createElement('div');
    root.id = 'root';
    document.body.appendChild(root);
    try {
      const { unmount } = render(
        <ConfirmDialog state={STATE} onClose={() => {}} />,
        { container: root.appendChild(document.createElement('div')) },
      );
      const dialog = screen.getByRole('dialog');
      // Portalled out of the screen that rendered it…
      expect(root.contains(dialog)).toBe(false);
      expect(dialog.parentElement).toHaveStyle({ position: 'fixed' });
      // …and everything behind it is out of reach.
      expect(root).toHaveAttribute('inert');
      unmount();
      expect(root).not.toHaveAttribute('inert');
    } finally {
      root.remove();
    }
  });

  it('NFR-034: reports an open modal so app shortcuts can stand down', () => {
    function Probe() {
      return (
        <output data-testid="modal-open">{String(useAnyModalOpen())}</output>
      );
    }
    const open = () => screen.getByTestId('modal-open').textContent;
    const { rerender } = render(<Probe />);
    expect(open()).toBe('false');
    rerender(
      <>
        <Probe />
        <ConfirmDialog state={STATE} onClose={() => {}} />
      </>,
    );
    expect(open()).toBe('true');
    rerender(<Probe />);
    expect(open()).toBe('false');
  });

  it('NFR-041: one Escape closes only the topmost of two stacked dialogs', async () => {
    const user = userEvent.setup();
    const closeBottom = vi.fn();
    const closeTop = vi.fn();
    render(
      <>
        <ConfirmDialog state={STATE} onClose={closeBottom} />
        <ConfirmDialog
          state={{ ...STATE, title: 'Second' }}
          onClose={closeTop}
        />
      </>,
    );
    await user.keyboard('{Escape}');
    expect(closeTop).toHaveBeenCalledTimes(1);
    expect(closeBottom).not.toHaveBeenCalled();
  });
});
