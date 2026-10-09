/**
 * Layout contract of the shared top bar and badge at the 880 px minimum
 * window width (tauri.conf.json `minWidth`). jsdom does not lay out, so these
 * pin the CSS that keeps long (Spanish) labels from running off the window;
 * the visual check is done in the browser E2E harness.
 */

import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { MPBadge } from './Badge';
import { MPTopBar } from './TopBar';

describe('MPTopBar', () => {
  it('NFR-053: the actions wrap under the title instead of overflowing', () => {
    render(
      <MPTopBar
        title="Cola"
        subtitle="3 activos · 3 pendientes · 3 terminados"
        right={<button>Limpiar terminados</button>}
      />,
    );
    const subtitle = screen.getByText(
      '3 activos · 3 pendientes · 3 terminados',
    );
    const bar = subtitle.parentElement!.parentElement!;
    const actions = screen.getByRole('button').parentElement!;

    expect(bar.style.flexWrap).toBe('wrap');
    expect(actions.style.flexWrap).toBe('wrap');
    expect(actions.style.marginLeft).toBe('auto');
    // One line with an ellipsis, never a word per line.
    expect(subtitle.style.whiteSpace).toBe('nowrap');
    expect(subtitle.style.textOverflow).toBe('ellipsis');
    expect(subtitle.parentElement!.style.minWidth).toBe('0');
  });
});

describe('MPBadge', () => {
  it('NFR-053: a badge never shrinks under its word or spills over neighbours', () => {
    render(<MPBadge>predeterminada</MPBadge>);
    const badge = screen.getByText('predeterminada');
    expect(badge.style.whiteSpace).toBe('nowrap');
    expect(badge.style.flexShrink).toBe('0');
  });
});
