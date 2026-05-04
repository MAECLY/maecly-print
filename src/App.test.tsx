import { render, screen } from '@testing-library/react';
import { describe, it, expect, beforeEach } from 'vitest';
import App from './App';

describe('App', () => {
  beforeEach(() => {
    // Skip onboarding modal so the main shell is visible.
    window.localStorage.setItem('maeclyprint:onboarding-complete', '1');
  });

  it('renders the sidebar nav items', () => {
    render(<App />);
    // Sidebar items live inside <nav>; pick that scope to avoid false
    // positives from screen content that mentions the same words.
    const nav = screen.getByRole('navigation');
    expect(nav).toBeInTheDocument();
    // "Print Job" is unique to the sidebar; a sanity check for the rest.
    expect(screen.getByText('Print Job')).toBeInTheDocument();
    expect(screen.getByText('Profiles')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });

  it('starts on the Home screen with the drop zone visible', () => {
    render(<App />);
    expect(
      screen.getByText(/Drop files or folders to print/i),
    ).toBeInTheDocument();
  });
});
