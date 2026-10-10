import { useState } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect } from 'vitest';

import { MPSegmented } from './Segmented';

const OPTIONS = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
];

function Harness({ initial = 'light' }: { initial?: string }) {
  const [value, setValue] = useState(initial);
  return (
    <>
      <button type="button">before</button>
      <MPSegmented
        value={value}
        onChange={setValue}
        options={OPTIONS}
        aria-label="Theme"
      />
    </>
  );
}

describe('MPSegmented', () => {
  it('NFR-045: exposes the selection through aria-checked, not colour alone', () => {
    render(<Harness />);
    const group = screen.getByRole('radiogroup', { name: 'Theme' });
    const options = screen.getAllByRole('radio');

    expect(group).toContainElement(options[0] ?? null);
    expect(options[0]).toBeChecked();
    expect(options[1]).not.toBeChecked();
  });

  it('NFR-041: takes a single tab stop on the selected option', async () => {
    const user = userEvent.setup();
    render(<Harness initial="dark" />);

    await user.tab();
    await user.tab();
    expect(screen.getByRole('radio', { name: 'Dark' })).toHaveFocus();
    expect(screen.getByRole('radio', { name: 'Light' })).toHaveAttribute(
      'tabindex',
      '-1',
    );
  });

  it('NFR-041: moves and selects with the arrow keys, wrapping around', async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.tab();
    await user.tab();
    await user.keyboard('{ArrowRight}');
    const dark = screen.getByRole('radio', { name: 'Dark' });
    expect(dark).toHaveFocus();
    expect(dark).toBeChecked();

    await user.keyboard('{ArrowLeft}{ArrowLeft}');
    const system = screen.getByRole('radio', { name: 'System' });
    expect(system).toHaveFocus();
    expect(system).toBeChecked();
  });

  it('NFR-041: Home and End jump to the first and last option', async () => {
    const user = userEvent.setup();
    render(<Harness initial="dark" />);

    await user.tab();
    await user.tab();
    await user.keyboard('{End}');
    expect(screen.getByRole('radio', { name: 'System' })).toBeChecked();

    await user.keyboard('{Home}');
    expect(screen.getByRole('radio', { name: 'Light' })).toBeChecked();
  });

  it('NFR-041: clicking an option still selects it', async () => {
    const user = userEvent.setup();
    render(<Harness />);

    await user.click(screen.getByRole('radio', { name: 'System' }));
    expect(screen.getByRole('radio', { name: 'System' })).toBeChecked();
  });

  it('NFR-053: fullWidth shares the width and lets long labels shrink', () => {
    render(
      <MPSegmented
        fullWidth
        aria-label="Caras"
        value="off"
        onChange={() => {}}
        options={[
          { value: 'off', label: 'Una cara' },
          { value: 'long', label: 'Borde largo' },
          { value: 'short', label: 'Borde corto' },
        ]}
      />,
    );
    const group = screen.getByRole('radiogroup', { name: 'Caras' });
    expect(group.style.width).toBe('100%');
    expect(group.style.maxWidth).toBe('100%');
    const option = screen.getByRole('radio', { name: 'Borde corto' });
    // The CSSOM serialises a zero length as "0px", as browsers do.
    expect(option.style.minWidth).toBe('0px');
    expect(option.style.textOverflow).toBe('ellipsis');
    // An ellipsized label stays readable on hover.
    expect(option).toHaveAttribute('title', 'Borde corto');
  });
});
