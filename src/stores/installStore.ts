/**
 * The one dependency install that may be running (FR-058, D13).
 *
 * The backend runs at most one install at a time, and it outlives the dialog
 * that started it: "Run in background" closes the dialog and the install goes
 * on. So the install's state lives here, not in the dialog. A dialog reopened
 * for the same dependency shows it still running, with its output and
 * **Stop installation**; a dialog for another dependency says which one is
 * busy instead of offering an Install the backend would refuse.
 *
 * When the install ends, a dialog watching it shows the result itself; with
 * none open, the result is announced (a toast) by the callback the last dialog
 * registered. The fresh dependency report always goes to the last registered
 * `onResolved`, so the screen that asked can resume what it unblocks.
 */

import { create } from 'zustand';

import type {
  Dependency,
  DependencyReport,
  InstallMethod,
  InstallOutcome,
} from '@/ipc/bindings';

/** How many output lines are kept. */
export const MAX_INSTALL_LINES = 8;

/** The install that is running. */
export interface ActiveInstall {
  id: Dependency;
  /** The dependency's display name, for "… is being installed". */
  name: string;
  method: InstallMethod;
}

/** How an install ended: an outcome, or a failure message. */
export type InstallEnd = { id: Dependency; seq: number } & (
  | { outcome: InstallOutcome; failure?: undefined }
  | { outcome?: undefined; failure: string }
);

/** What a dialog hands the store so an install can report back. */
export interface InstallHandlers {
  /** Receives the dependency probed again after the install. */
  onResolved: (report: DependencyReport) => void;
  /** Announces an end nobody is watching (a toast). */
  announce: (end: InstallEnd) => void;
}

/** The backend calls the store makes, injected so tests can drive them. */
export interface InstallBackend {
  run: (id: Dependency, method: InstallMethod) => Promise<InstallOutcome>;
  progress: (
    onLine: (id: Dependency, line: string) => void,
  ) => Promise<() => void>;
  /** Turns a rejection into a sentence in the user's language. */
  describe: (e: unknown) => string;
}

interface InstallState {
  active: ActiveInstall | null;
  /** The running install's last output lines. */
  lines: string[];
  /** Stop was pressed and the install has not ended yet. */
  stopping: boolean;
  /** How the last install ended; `seq` grows with every end. */
  last: InstallEnd | null;
  /**
   * Start `method` for `report`. Refused (false) while another install runs:
   * the dialog never offers it then.
   */
  start: (
    report: DependencyReport,
    method: InstallMethod,
    backend: InstallBackend,
    handlers: InstallHandlers,
  ) => boolean;
  /** Ask the running install to stop; rejects when the request failed. */
  stop: (cancel: () => Promise<boolean>) => Promise<void>;
  /**
   * A dialog for `id` is open: while it is, it shows the end itself, and its
   * handlers replace the starter's. Returns the call that undoes it.
   */
  watch: (id: Dependency, handlers: InstallHandlers) => () => void;
}

/** Open dialogs per dependency (not rendered state, so outside the store). */
const watchers = new Map<Dependency, number>();
/** The handlers the running install reports to. */
let handlers: InstallHandlers | null = null;
let seq = 0;

export const useInstallStore = create<InstallState>()((set, get) => ({
  active: null,
  lines: [],
  stopping: false,
  last: null,

  start: (report, method, backend, own) => {
    if (get().active) return false;
    const id = report.id;
    handlers = own;
    set({
      active: { id, name: report.name, method },
      lines: [],
      stopping: false,
    });

    let unlisten: (() => void) | undefined;
    let listening = true;
    backend
      .progress((lineId, line) => {
        if (lineId !== id) return;
        set((s) => ({ lines: [...s.lines, line].slice(-MAX_INSTALL_LINES) }));
      })
      .then((stop) => {
        if (listening) unlisten = stop;
        else stop();
      })
      .catch(() => {});

    const finish = (end: InstallEnd) => {
      listening = false;
      unlisten?.();
      const to = handlers;
      handlers = null;
      set({ active: null, stopping: false, last: end });
      if (end.outcome) to?.onResolved(end.outcome.report);
      if (!watchers.get(id)) to?.announce(end);
    };
    backend.run(id, method).then(
      (outcome) => finish({ id, seq: ++seq, outcome }),
      (e: unknown) => finish({ id, seq: ++seq, failure: backend.describe(e) }),
    );
    return true;
  },

  stop: async (cancel) => {
    if (!get().active) return;
    set({ stopping: true });
    try {
      await cancel();
    } catch (e) {
      set({ stopping: false });
      throw e;
    }
  },

  watch: (id, own) => {
    watchers.set(id, (watchers.get(id) ?? 0) + 1);
    if (get().active?.id === id) handlers = own;
    return () => {
      const n = (watchers.get(id) ?? 1) - 1;
      if (n > 0) watchers.set(id, n);
      else watchers.delete(id);
    };
  },
}));

/** Forget every install (tests only). */
export function resetInstallStore(): void {
  watchers.clear();
  handlers = null;
  useInstallStore.setState({
    active: null,
    lines: [],
    stopping: false,
    last: null,
  });
}
