import { useEffect, useLayoutEffect } from 'react';
import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/react';

import { useLatest } from './useLatest';

describe('useLatest', () => {
  it('holds the first value from the first render', () => {
    let seen: string | undefined;
    function Probe() {
      const latest = useLatest('first');
      useEffect(() => {
        seen = latest.current;
      });
      return null;
    }
    render(<Probe />);
    expect(seen).toBe('first');
  });

  it('is current in the effects of the render that changed it', () => {
    const layout: number[] = [];
    const passive: number[] = [];
    function Probe({ n }: { n: number }) {
      const latest = useLatest(n);
      useLayoutEffect(() => {
        layout.push(latest.current);
      });
      useEffect(() => {
        passive.push(latest.current);
      });
      return null;
    }
    const { rerender } = render(<Probe n={1} />);
    rerender(<Probe n={2} />);
    expect(layout).toEqual([1, 2]);
    expect(passive).toEqual([1, 2]);
  });

  it('is current for a child layout effect reading its parent value', () => {
    const seen: string[] = [];
    function Child({ read }: { read: () => string }) {
      useLayoutEffect(() => {
        seen.push(read());
      });
      return null;
    }
    function Parent({ label }: { label: string }) {
      const latest = useLatest(label);
      return <Child read={() => latest.current} />;
    }
    const { rerender } = render(<Parent label="a" />);
    rerender(<Parent label="b" />);
    expect(seen).toEqual(['a', 'b']);
  });

  it('keeps the same ref object across renders', () => {
    const refs: object[] = [];
    function Probe({ n }: { n: number }) {
      const latest = useLatest(n);
      useEffect(() => {
        refs.push(latest);
      });
      return null;
    }
    const { rerender } = render(<Probe n={1} />);
    rerender(<Probe n={2} />);
    expect(refs[0]).toBe(refs[1]);
  });
});
