/**
 * First-run tour tests. Hermetic: the profiles store is set directly, no IPC
 * host and no user config are involved.
 *
 * Half of these assert what the tour must NOT say. The shell integration is not
 * in this build, so a step promising it in the present tense — or a Settings
 * switch to turn it off — is a defect the moment it is written, and only a test
 * keeps it from coming back.
 */

import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, beforeEach, vi } from 'vitest';

import { OnboardingFlow } from './OnboardingFlow';
import { shortcutLabel } from '@/components/AppShell/MPAppShell';
import { BUILT_IN_PROFILES } from '@/lib/profiles';
import { useProfilesStore } from '@/stores/profilesStore';

/** Walk the tour to the last step, returning the text seen along the way. */
function walkToEnd(): string {
  let seen = document.body.textContent ?? '';
  for (let i = 0; i < 10; i += 1) {
    const next = screen.queryByRole('button', { name: /continue/i });
    if (!next) break;
    fireEvent.click(next);
    seen += document.body.textContent ?? '';
  }
  return seen;
}

describe('OnboardingFlow', () => {
  beforeEach(() => {
    useProfilesStore.setState({
      status: 'ready',
      profiles: BUILT_IN_PROFILES,
      error: undefined,
    });
  });

  it('walks forward and back through every step', () => {
    render(<OnboardingFlow onComplete={vi.fn()} />);
    expect(screen.getByText(/welcome to maeclyprint/i)).toBeInTheDocument();
    // The first step has nothing to go back to.
    expect(screen.queryByRole('button', { name: /back/i })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: /continue/i }));
    expect(screen.getByText(/detect your printers/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /back/i }));
    expect(screen.getByText(/welcome to maeclyprint/i)).toBeInTheDocument();
  });

  it('completes from the last step and from Skip', () => {
    const onComplete = vi.fn();
    const { unmount } = render(<OnboardingFlow onComplete={onComplete} />);
    walkToEnd();
    fireEvent.click(screen.getByRole('button', { name: /get started/i }));
    expect(onComplete).toHaveBeenCalledTimes(1);
    unmount();

    render(<OnboardingFlow onComplete={onComplete} />);
    fireEvent.click(screen.getByRole('button', { name: /skip/i }));
    expect(onComplete).toHaveBeenCalledTimes(2);
  });

  it('NFR-041: announces which step is showing', () => {
    render(<OnboardingFlow onComplete={vi.fn()} />);
    expect(screen.getByRole('status')).toHaveAccessibleName('Step 1 of 5');
    fireEvent.click(screen.getByRole('button', { name: /continue/i }));
    expect(screen.getByRole('status')).toHaveAccessibleName('Step 2 of 5');
  });

  it('does not promise the shell right-click menu, nor a Settings switch for it', () => {
    render(<OnboardingFlow onComplete={vi.fn()} />);
    const seen = walkToEnd();
    expect(seen).not.toMatch(/adds a "print with maeclyprint" entry/i);
    expect(seen).not.toMatch(/disable it any time in settings/i);
    expect(seen).not.toMatch(/right-click any number of files/i);
    // Naming it as planned work is fine — and is what the tour now does.
    expect(seen).toMatch(/planned for a later milestone/i);
  });

  it('FR-008: advertises the CLI, which this build really has', () => {
    render(<OnboardingFlow onComplete={vi.fn()} />);
    const seen = walkToEnd();
    expect(seen).toMatch(/maeclyprint print --files/);
  });

  it('NFR-034: points at a shortcut that exists', () => {
    render(<OnboardingFlow onComplete={vi.fn()} />);
    const seen = walkToEnd();
    expect(seen).toContain(shortcutLabel('newJob'));
  });

  it('names the real default profile instead of a hardcoded one', () => {
    useProfilesStore.setState({
      status: 'ready',
      profiles: BUILT_IN_PROFILES.map((p) => ({
        ...p,
        default: p.id === 'builtin-photo-color',
      })),
      error: undefined,
    });
    render(<OnboardingFlow onComplete={vi.fn()} />);
    const seen = walkToEnd();
    expect(seen).toMatch(/Photo Color is the default profile/);
    expect(seen).not.toMatch(/Quick \(color, simplex, A4\)/);
  });
});
