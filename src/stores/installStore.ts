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
 * When the install ends, the fresh dependency report goes to **every** screen
 * that asked about it — the one that started it and every dialog opened on
 * it since — so the Queue resumes its waiting jobs even when Print Job's
 * dialog was the last one open. A dialog watching it shows the result
 * itself; with none open, the result is announced (a toast) once.
 *
 * The backend also says which install holds its slot
 * (`dependency_install_status`). The store asks on startup (`hydrate`), so a
 * reloaded window finds a running install again, Stop included; and after an
 * end the installer may have outlived (`installerMayContinue`), it follows
 * the slot until the backend frees it (`finishing`), so no dialog offers an
 * Install the backend would refuse meanwhile.
 */

import { create } from 'zustand';

import type {
  Dependency,
  DependencyInstallState,
  DependencyReport,
  InstallMethod,
  InstallOutcome,
} from '@/ipc/bindings';

/** How many output lines are kept. */
export const MAX_INSTALL_LINES = 8;

/** How often the backend's install slot is asked about while followed. */
export const INSTALL_POLL_MS = 2000;

/** The install that is running. */
export interface ActiveInstall {
  id: Dependency;
  /** The dependency's display name, for "… is being installed". */
  name: string;
  method: InstallMethod;
}

/**
 * An install that ended while its installer may still be running: the
 * backend keeps the slot until the installer is gone.
 */
export interface FinishingInstall {
  id: Dependency;
  name: string;
}

/** How an install ended: an outcome, or a failure message. */
export type InstallEnd = { id: Dependency; name: string; seq: number } & (
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
  /**
   * Which install holds the backend's slot. Without it the store does not
   * follow an installer that may outlive its end.
   */
  status?: () => Promise<DependencyInstallState>;
  /** Poll interval for `status`; `INSTALL_POLL_MS` by default. */
  pollMs?: number;
}

/** What `hydrate` needs beyond the backend: who speaks for a lost window. */
export interface HydrateOptions {
  /** Announces the end of an install found running at startup. */
  announce: (end: InstallEnd) => void;
  /** The sentence for an install whose end the backend could not report. */
  unknownEnd: (name: string) => string;
}

/** True when the installer may still be running after `o`. */
export function installerMayContinue(o: InstallOutcome): boolean {
  return (
    o.status === 'cancel-requested-installer-may-continue' ||
    o.status === 'timed-out-installer-may-continue'
  );
}

interface InstallState {
  active: ActiveInstall | null;
  /** The running install's last output lines. */
  lines: string[];
  /** Stop was pressed and the install has not ended yet. */
  stopping: boolean;
  /** How the last install ended; `seq` grows with every end. */
  last: InstallEnd | null;
  /** An ended install whose installer may still be running. */
  finishing: FinishingInstall | null;
  /**
   * Start `method` for `report`. Refused (false) while another install runs
   * or finishes: the dialog never offers it then.
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
   * `onResolved` hears the end too. Returns the call that undoes it.
   */
  watch: (id: Dependency, handlers: InstallHandlers) => () => void;
  /**
   * Ask the backend which install holds its slot and pick it up: a running
   * one becomes `active` (and is followed to its end), one that ended while
   * its installer may continue becomes `finishing`. A no-op while this
   * window already runs one.
   */
  hydrate: (backend: InstallBackend, options: HydrateOptions) => Promise<void>;
}

/** Open dialogs per dependency (not rendered state, so outside the store). */
const open = new Map<Dependency, InstallHandlers[]>();
/** Everyone the running install reports its fresh report to. */
let subscribers = new Set<InstallHandlers>();
/** Speaks for an install picked up by `hydrate` when no dialog is open. */
let fallbackAnnounce: ((end: InstallEnd) => void) | null = null;
/** Bumped by every start/hydrate/reset: a stale follow-up loop stops. */
let generation = 0;
let seq = 0;

export const useInstallStore = create<InstallState>()((set, get) => {
  /** Listen to the install's output lines until `stop` is called. */
  function listen(
    id: Dependency,
    backend: Pick<InstallBackend, 'progress'>,
  ): () => void {
    let unlisten: (() => void) | undefined;
    let listening = true;
    backend
      .progress((lineId, line) => {
        if (lineId !== id || !listening) return;
        set((s) => ({ lines: [...s.lines, line].slice(-MAX_INSTALL_LINES) }));
      })
      .then((stop) => {
        if (listening) unlisten = stop;
        else stop();
      })
      .catch(() => {});
    return () => {
      listening = false;
      unlisten?.();
    };
  }

  /** The install ended: tell everyone who asked, once. */
  function finish(end: InstallEnd): void {
    const watching = open.get(end.id) ?? [];
    const recipients = [...new Set([...subscribers, ...watching])];
    subscribers = new Set();
    set({ active: null, stopping: false, last: end });
    if (end.outcome) {
      for (const r of recipients) r.onResolved(end.outcome.report);
    }
    if (watching.length === 0) {
      const speaker = recipients.at(-1)?.announce ?? fallbackAnnounce;
      speaker?.(end);
    }
  }

  /**
   * Ask the backend about its slot every `pollMs` until `done` says the
   * answer settles it. A failed ask ends the follow-up (nothing is known).
   */
  function follow(
    backend: InstallBackend,
    done: (s: DependencyInstallState | null) => boolean,
  ): void {
    const status = backend.status;
    if (!status) {
      done(null);
      return;
    }
    const mine = generation;
    const tick = () => {
      if (mine !== generation) return;
      status().then(
        (s) => {
          if (mine !== generation) return;
          if (!done(s)) setTimeout(tick, backend.pollMs ?? INSTALL_POLL_MS);
        },
        () => {
          if (mine === generation) done(null);
        },
      );
    };
    setTimeout(tick, backend.pollMs ?? INSTALL_POLL_MS);
  }

  /** Hold `finishing` until the backend frees the slot it keeps. */
  function followFinishing(
    finishing: FinishingInstall,
    backend: InstallBackend,
  ): void {
    set({ finishing });
    follow(backend, (s) => {
      if (s?.running?.may_continue && s.running.id === finishing.id) {
        return false;
      }
      if (get().finishing === finishing) set({ finishing: null });
      return true;
    });
  }

  return {
    active: null,
    lines: [],
    stopping: false,
    last: null,
    finishing: null,

    start: (report, method, backend, own) => {
      if (get().active || get().finishing) return false;
      const id = report.id;
      const name = report.name;
      generation++;
      subscribers = new Set([own]);
      set({ active: { id, name, method }, lines: [], stopping: false });

      const unlisten = listen(id, backend);
      const ended = (end: InstallEnd) => {
        unlisten();
        finish(end);
        if (end.outcome && installerMayContinue(end.outcome)) {
          followFinishing({ id, name }, backend);
        }
      };
      backend.run(id, method).then(
        (outcome) => ended({ id, name, seq: ++seq, outcome }),
        (e: unknown) =>
          ended({ id, name, seq: ++seq, failure: backend.describe(e) }),
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
      open.set(id, [...(open.get(id) ?? []), own]);
      if (get().active?.id === id) subscribers.add(own);
      return () => {
        const left = (open.get(id) ?? []).filter((h) => h !== own);
        if (left.length > 0) open.set(id, left);
        else open.delete(id);
      };
    },

    hydrate: async (backend, options) => {
      if (!backend.status || get().active || get().finishing) return;
      let state: DependencyInstallState;
      try {
        state = await backend.status();
      } catch {
        return;
      }
      const running = state.running;
      // This window started one meanwhile: it knows better.
      if (!running || get().active || get().finishing) return;
      const { id, name } = running;
      if (running.may_continue) {
        generation++;
        followFinishing({ id, name }, backend);
        return;
      }

      generation++;
      fallbackAnnounce = options.announce;
      subscribers = new Set(open.get(id) ?? []);
      set({
        active: { id, name, method: running.method },
        lines: [],
        stopping: false,
      });
      const unlisten = listen(id, backend);
      follow(backend, (s) => {
        if (s?.running?.seq === running.seq && !s.running.may_continue) {
          return false;
        }
        unlisten();
        const ended = s?.last?.seq === running.seq ? s.last.outcome : null;
        if (ended) finish({ id, name, seq: ++seq, outcome: ended });
        else
          finish({ id, name, seq: ++seq, failure: options.unknownEnd(name) });
        if (s?.running?.seq === running.seq && s.running.may_continue) {
          followFinishing({ id, name }, backend);
        }
        return true;
      });
    },
  };
});

/** Forget every install (tests only). */
export function resetInstallStore(): void {
  open.clear();
  subscribers = new Set();
  fallbackAnnounce = null;
  generation++;
  useInstallStore.setState({
    active: null,
    lines: [],
    stopping: false,
    last: null,
    finishing: null,
  });
}
