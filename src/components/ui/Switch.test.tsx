import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { MPSwitch } from './Switch';

/** Controlled harness: the real screens own the state, so the tests do too. */
function Harness({ initial = false }: { initial?: boolean }) {
  const [on, setOn] = useState(initial);
  return (
    <>
      <button type="button">before</button>
      <MPSwitch
        checked={on}
        onChange={(e) => setOn(e.target.checked)}
        aria-label="Crash reports"
      />
    </>
  );
}

describe('MPSwitch', () => {
  it('NFR-044: exposes switch semantics with its checked state', () => {
    render(<MPSwitch checked onChange={() => {}} aria-label="Crash reports" />);
    const control = screen.getByRole('switch', { name: 'Crash reports' });
    expect(control).toBeChecked();
  });

  it('NFR-043: takes its accessible name from a visible label when given one', () => {
    render(
      <MPSwitch checked={false} onChange={() => {}} label="Check on startup" />,
    );
    expect(
      screen.getByRole('switch', { name: 'Check on startup' }),
    ).toBeInTheDocument();
  });

  it('NFR-041: is reachable with Tab and toggles with Space', async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.tab();
    expect(screen.getByRole('button', { name: 'before' })).toHaveFocus();

    await user.tab();
    const control = screen.getByRole('switch', { name: 'Crash reports' });
    expect(control).toHaveFocus();

    await user.keyboard(' ');
    expect(control).toBeChecked();

    await user.keyboard(' ');
    expect(control).not.toBeChecked();
  });

  it('NFR-041: clicking the label still toggles the switch', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <MPSwitch checked={false} onChange={onChange} label="Check on startup" />,
    );

    await user.click(screen.getByText('Check on startup'));
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('NFR-042: paints a focus ring on the track while the input has focus', async () => {
    const user = userEvent.setup();
    const { container } = render(
      <MPSwitch
        checked={false}
        onChange={() => {}}
        aria-label="Crash reports"
      />,
    );
    const track = container.querySelector('span[aria-hidden="true"]');

    expect(track).toHaveStyle({ outline: 'none' });
    await user.tab();
    expect(screen.getByRole('switch')).toHaveFocus();
    expect(track).toHaveStyle({ outline: '2px solid var(--mp-focus)' });
  });
});
