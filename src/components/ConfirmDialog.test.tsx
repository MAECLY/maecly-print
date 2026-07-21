import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { ConfirmDialog, type ConfirmState } from './ConfirmDialog';

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
});
