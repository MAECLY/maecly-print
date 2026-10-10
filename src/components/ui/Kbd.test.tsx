import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';

import { MPKbd } from './Kbd';

describe('MPKbd', () => {
  it('NFR-044: renders a real <kbd> and no interactive role', () => {
    const { container } = render(<MPKbd>Ctrl+P</MPKbd>);
    const key = container.querySelector('kbd');

    expect(key).toHaveTextContent('Ctrl+P');
    expect(key).not.toHaveAttribute('role');
    expect(key).not.toHaveAttribute('tabindex');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('NFR-043: accepts a spoken alternative for glyph-only keys', () => {
    render(<MPKbd aria-label="Up and down arrows">↑↓</MPKbd>);
    expect(screen.getByLabelText('Up and down arrows')).toHaveTextContent('↑↓');
  });
});
