import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { MPInput } from './Input';
import { MPSegmented } from './Segmented';
import { MPSelect } from './Select';
import { MPSwitch } from './Switch';

describe('MPInput', () => {
  it('NFR-043: takes an accessible name and ties its error to the field', () => {
    render(
      <>
        <MPInput aria-label="Copies" aria-describedby="err" error value="0" />
        <div id="err">Copies must be between 1 and 999.</div>
      </>,
    );
    const field = screen.getByRole('textbox', { name: 'Copies' });
    expect(field).toHaveAttribute('aria-invalid', 'true');
    expect(field).toHaveAccessibleDescription(
      'Copies must be between 1 and 999.',
    );
  });

  it('is not marked invalid without an error', () => {
    render(<MPInput aria-label="Host" value="" />);
    expect(screen.getByRole('textbox', { name: 'Host' })).not.toHaveAttribute(
      'aria-invalid',
    );
  });
});

describe('disabled controls (NFR-041)', () => {
  it('a disabled input is natively disabled', () => {
    render(<MPInput aria-label="Timeout" value="30" disabled />);
    expect(screen.getByRole('textbox', { name: 'Timeout' })).toBeDisabled();
  });

  it('a disabled segmented control ignores clicks and arrow keys', () => {
    const onChange = vi.fn();
    render(
      <MPSegmented
        aria-label="Theme"
        value="light"
        onChange={onChange}
        disabled
        options={[
          { value: 'light', label: 'Light' },
          { value: 'dark', label: 'Dark' },
        ]}
      />,
    );
    const dark = screen.getByRole('radio', { name: 'Dark' });
    expect(dark).toBeDisabled();
    fireEvent.click(dark);
    fireEvent.keyDown(screen.getByRole('radio', { name: 'Light' }), {
      key: 'ArrowRight',
    });
    expect(onChange).not.toHaveBeenCalled();
  });

  it('a disabled switch and select are natively disabled', () => {
    render(
      <>
        <MPSwitch aria-label="Check on startup" checked={false} disabled />
        <MPSelect
          aria-label="Channel"
          disabled
          options={[{ value: 'stable', label: 'Stable' }]}
        />
      </>,
    );
    expect(
      screen.getByRole('switch', { name: 'Check on startup' }),
    ).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'Channel' })).toBeDisabled();
  });
});
