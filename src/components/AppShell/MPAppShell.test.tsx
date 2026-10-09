import { useState } from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { ConfirmDialog } from '@/components/ConfirmDialog';

import { isMacPlatform, useAppShortcuts } from './MPAppShell';

/** Fire the platform's Ctrl/⌘ + `key` on `target`. */
function press(key: string, target: Element | Document = document) {
  const mac = isMacPlatform();
  fireEvent.keyDown(target, { key, metaKey: mac, ctrlKey: !mac });
}

function Harness({ onNew }: { onNew: () => void }) {
  const [open, setOpen] = useState(false);
  useAppShortcuts({ newJob: onNew });
  return (
    <>
      <input aria-label="Name" />
      <button type="button" onClick={() => setOpen(true)}>
        Ask
      </button>
      {open && (
        <ConfirmDialog
          state={{
            title: 'Discard changes?',
            message: 'Unsaved edits are lost.',
            confirmLabel: 'Discard',
            run: () => {},
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}

describe('useAppShortcuts', () => {
  it('NFR-034: fires the handler for its key', () => {
    const onNew = vi.fn();
    render(<Harness onNew={onNew} />);
    press('n');
    expect(onNew).toHaveBeenCalledTimes(1);
  });

  it('NFR-034: stays quiet while focus is in an editable field', () => {
    const onNew = vi.fn();
    render(<Harness onNew={onNew} />);
    press('n', screen.getByRole('textbox', { name: 'Name' }));
    expect(onNew).not.toHaveBeenCalled();
  });

  it('NFR-034: stands down while any dialog is open, and resumes after', () => {
    const onNew = vi.fn();
    render(<Harness onNew={onNew} />);

    fireEvent.click(screen.getByRole('button', { name: 'Ask' }));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    press('n');
    expect(onNew).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    press('n');
    expect(onNew).toHaveBeenCalledTimes(1);
  });
});
