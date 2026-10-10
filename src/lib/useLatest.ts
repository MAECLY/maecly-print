import { useInsertionEffect, useRef } from 'react';

/**
 * A ref that holds `value` as of the latest committed render.
 *
 * For callbacks that outlive the render that created them (a listener
 * installed once, an awaited promise, a timer) and must still see the current
 * closure or state rather than the one they captured. Read `.current` only
 * from such callbacks or from effects, never while rendering: during render
 * the props and state themselves are the current values.
 *
 * The ref is written in an insertion effect rather than during render, so a
 * render React throws away never leaks into it, and it is already current
 * when any layout or passive effect of the same commit runs (insertion
 * effects run before both, across the whole tree).
 */
export function useLatest<T>(value: T): { readonly current: T } {
  const ref = useRef(value);
  useInsertionEffect(() => {
    ref.current = value;
  });
  return ref;
}
