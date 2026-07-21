import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect } from 'vitest';

import { MPCheckbox } from './Checkbox';

function Harness() {
  const [on, setOn] = useState(false);
  return (
    <>
      <button type="button">before</button>
      <MPCheckbox
        checked={on}
        onChange={(e) => setOn(e.target.checked)}
        label="Office HP"
      />
    </>
  );
}

describe('MPCheckbox', () => {
  it('NFR-044: exposes checkbox semantics named by its label', () => {
    render(<MPCheckbox checked onChange={() => {}} label="Office HP" />);
    expect(screen.getByRole('checkbox', { name: 'Office HP' })).toBeChecked();
  });

  it('NFR-043: accepts an accessible name when no visible label is rendered', () => {
    render(
      <MPCheckbox
        checked={false}
        onChange={() => {}}
        aria-label="Enable printer"
      />,
    );
    expect(
      screen.getByRole('checkbox', { name: 'Enable printer' }),
    ).toBeInTheDocument();
  });

  it('NFR-041: is reachable with Tab and toggles with Space', async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.tab();
    await user.tab();
    const box = screen.getByRole('checkbox', { name: 'Office HP' });
    expect(box).toHaveFocus();

    await user.keyboard(' ');
    expect(box).toBeChecked();
  });

  it('NFR-045: reports the indeterminate state as mixed, not only by colour', () => {
    render(
      <MPCheckbox
        checked={false}
        onChange={() => {}}
        indeterminate
        label="All jobs"
      />,
    );
    const box = screen.getByRole('checkbox', { name: 'All jobs' });
    expect(box).toBePartiallyChecked();
  });

  it('NFR-042: paints a focus ring on the box while the input has focus', async () => {
    const user = userEvent.setup();
    const { container } = render(
      <MPCheckbox checked={false} onChange={() => {}} label="Office HP" />,
    );
    const box = container.querySelector('span[aria-hidden="true"]');

    expect(box).toHaveStyle({ outline: 'none' });
    await user.tab();
    expect(box).toHaveStyle({ outline: '2px solid var(--mp-focus)' });
  });
});
