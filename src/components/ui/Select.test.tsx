import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { MPSelect } from './Select';

const OPTIONS = [
  { value: 'a4', label: 'A4' },
  { value: 'letter', label: 'Letter' },
];

describe('MPSelect', () => {
  it('NFR-043: takes an accessible name from aria-label', () => {
    render(
      <MPSelect
        value="a4"
        options={OPTIONS}
        onChange={() => {}}
        aria-label="Paper size"
      />,
    );
    expect(
      screen.getByRole('combobox', { name: 'Paper size' }),
    ).toBeInTheDocument();
  });

  it('NFR-043: can be named by an external caption', () => {
    render(
      <>
        <span id="paper-caption">Paper size</span>
        <MPSelect
          value="a4"
          options={OPTIONS}
          aria-labelledby="paper-caption"
        />
      </>,
    );
    expect(
      screen.getByRole('combobox', { name: 'Paper size' }),
    ).toBeInTheDocument();
  });

  it('NFR-042: keeps a visible focus indicator instead of suppressing the outline', () => {
    render(
      <MPSelect
        value="a4"
        options={OPTIONS}
        onChange={() => {}}
        aria-label="Paper size"
      />,
    );
    const select = screen.getByRole('combobox', { name: 'Paper size' });

    expect(select).toHaveClass('mp-focus');
    // An inline `outline: none` would beat the stylesheet's focus ring.
    expect(select.style.outline).toBe('');
  });

  it('NFR-041: is reachable with Tab and changes with the keyboard', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <MPSelect
        value="a4"
        options={OPTIONS}
        onChange={onChange}
        aria-label="Paper size"
      />,
    );
    const select = screen.getByRole('combobox', { name: 'Paper size' });

    await user.tab();
    expect(select).toHaveFocus();

    await user.selectOptions(select, 'letter');
    expect(onChange).toHaveBeenCalledTimes(1);
  });
});
