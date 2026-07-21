import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';

import { MPSidebar, type ScreenId } from '@/components/AppShell/MPSidebar';
import { MPAppShell, useAppShortcuts } from '@/components/AppShell/MPAppShell';
import { Toaster } from '@/components/AppShell/Toaster';
import { ConfirmDialog, type ConfirmState } from '@/components/ConfirmDialog';
import { useDensity, useTheme, type ResolvedTheme } from '@/lib/theme';
import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  findProfileIn,
} from '@/lib/profiles';
import {
  isTauri,
  openExternal,
  pauseQueue,
  printFiles,
  takePendingSelections,
  type Selection,
} from '@/lib/ipc';
import { addPathsToJob } from '@/lib/jobFiles';
import { events } from '@/ipc/bindings';
import { useProfilesStore } from '@/stores/profilesStore';
import { useQueueStore } from '@/stores/queueStore';
import { useSettingsStore } from '@/stores/settingsStore';
import { useT } from '@/i18n/I18nProvider';
import type { UiFile } from '@/types/domain';

import { HomeScreen, PRINTABLE_EXTENSIONS } from '@/screens/HomeScreen';
import {
  PrintJobScreen,
  type PrintJobOverrides,
} from '@/screens/PrintJobScreen';
import { QueueScreen, type QueueView } from '@/screens/QueueScreen';
import { PrintersScreen } from '@/screens/PrintersScreen';
import { ProfilesScreen } from '@/screens/ProfilesScreen';
import { SettingsScreen } from '@/screens/SettingsScreen';
import { AboutScreen } from '@/screens/AboutScreen';
import { OnboardingFlow } from '@/screens/auxiliary/OnboardingFlow';

const ONBOARD_KEY = 'maeclyprint:onboarding-complete';

/**
 * How long the "an update is available" toast stays up. Long enough to be read
 * by someone who was not looking at the moment it appeared (the check fires 30
 * seconds after launch, unprompted), short enough that it still goes away on
 * its own — the news also waits on the About screen, so nothing is lost by
 * missing it.
 */
const UPDATE_TOAST_MS = 15_000;

function readOnboardingDone() {
  try {
    return window.localStorage.getItem(ONBOARD_KEY) === '1';
  } catch {
    return false;
  }
}

function App() {
  const t = useT('common');
  // Shortcut feedback and the cancel confirmation are documented on the Home
  // screen, so they share its namespace.
  const tHome = useT('home');
  // The launch-time update toast is the same news the About screen's update
  // card shows, so it reuses that screen's strings verbatim.
  const tAbout = useT('about');
  const { mode, resolved, setMode } = useTheme();
  const { density, setDensity } = useDensity();
  const [active, setActive] = useState<ScreenId>('home');
  const [onboardingDone, setOnboardingDone] = useState(readOnboardingDone);

  // Print job state. `profileChoice` is the user's explicit selection; while
  // null the job follows the app's default profile, so changing the default in
  // Settings/Profiles retargets future jobs until the user picks one manually.
  const [files, setFiles] = useState<UiFile[]>([]);
  const [profileChoice, setProfileChoice] = useState<string | null>(null);
  const [overrides, setOverrides] = useState<PrintJobOverrides>({});
  const profiles = useProfilesStore((s) => s.profiles);
  const profilesStatus = useProfilesStore((s) => s.status);
  const initProfiles = useProfilesStore((s) => s.init);
  const effectiveProfileId = findProfileIn(profiles, profileChoice).id;

  // Queue state (jobs live in the queue store; App owns only the view toggle)
  const [queueView, setQueueView] = useState<QueueView>('list');
  const initQueue = useQueueStore((s) => s.init);
  const queueCount = useQueueStore((s) => s.counts.active + s.counts.pending);
  const uiJobs = useQueueStore((s) => s.uiJobs);
  const cancelJob = useQueueStore((s) => s.cancelJob);

  // Destructive shortcuts confirm first (never `window.confirm`, which is a
  // no-op in WKWebView).
  const [confirm, setConfirm] = useState<ConfirmState | null>(null);

  // App settings (backend config.toml): loaded once; appearance reconciles below.
  const initSettings = useSettingsStore((s) => s.init);
  const settings = useSettingsStore((s) => s.settings);

  const [selectedProfileId, setSelectedProfileId] = useState(
    BUILT_IN_PROFILES[0]!.id,
  );

  // Fallback so the document is never class-less before useTheme's first apply.
  useEffect(() => {
    if (
      typeof document !== 'undefined' &&
      !document.documentElement.classList.contains('mp-light') &&
      !document.documentElement.classList.contains('mp-dark')
    ) {
      document.documentElement.classList.add('mp-light');
    }
  }, []);

  // Load the queue snapshot and subscribe to live updates (idempotent).
  useEffect(() => {
    void initQueue();
  }, [initQueue]);

  // Load profiles and app settings once (both idempotent, retryable).
  useEffect(() => {
    void initProfiles();
    void initSettings();
  }, [initProfiles, initSettings]);

  // Appearance reconcile: localStorage applied instantly at boot (no flash);
  // once the backend config arrives it is the source of truth and overwrites
  // a diverging local value (the setters keep localStorage in sync).
  useEffect(() => {
    if (!settings) return;
    if (settings.app.theme !== mode) setMode(settings.app.theme);
    if (settings.app.density && settings.app.density !== density) {
      setDensity(settings.app.density);
    }
    // Reconcile only when the backend value changes, not on local edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings?.app.theme, settings?.app.density]);

  // A deleted custom profile silently falls back to the app default. Only act
  // when the store is authoritative ('ready'); while it is loading/error/
  // unavailable the list can be transiently empty or stale, and clearing an
  // explicit choice then would wipe the user's selection for no reason.
  useEffect(() => {
    if (profilesStatus !== 'ready') return;
    if (profileChoice && !profiles.some((p) => p.id === profileChoice)) {
      setProfileChoice(null);
    }
  }, [profiles, profileChoice, profilesStatus]);

  // App-level drag & drop (FR-007): the Home screen advertises "drag them
  // here", so drops must work from any screen, not only while the Print Job
  // screen (which owns its own listener + overlay) is mounted. When the job
  // screen is active its listener handles the drop, so this one stands down to
  // avoid double-adding.
  const activeRef = useRef(active);
  activeRef.current = active;
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (event.payload.type !== 'drop') return;
          if (activeRef.current === 'job') return;
          void addPathsToJob(event.payload.paths, setFiles);
          setActive('job');
        }),
      )
      .then((un) => {
        if (cancelled) un();
        else unlisten = un;
      })
      .catch((e) => {
        console.error('app drag-drop listener failed', e);
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // FR-080: the backend runs the launch-time update check (delayed, throttled
  // to once per 24h) and pushes this event only when there is something the
  // user should hear about — a strictly newer release they have not skipped
  // (FR-084). All of that policy lives in Rust, so there is nothing to filter
  // here: receiving the event *is* the decision to say something.
  //
  // The toast deliberately stops at the release page. This build cannot verify
  // or install a download (FR-082/FR-083 arrive with the signed auto-updater),
  // and the About screen says so next to its own check; a toast that offered
  // to "update" would be a promise the app cannot keep. Skipping a version
  // also stays on that screen, where the full context is visible.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    events.updateAvailable
      .listen(({ payload }) => {
        const version = payload.latest;
        if (!version) return;
        const url = payload.url;
        toast.info(tAbout('updates.available.title', { version }), {
          description: tAbout('updates.available.detail', {
            current: payload.current,
          }),
          duration: UPDATE_TOAST_MS,
          action: url
            ? {
                label: tAbout('updates.available.releaseNotes'),
                onClick: () => void openExternal(url),
              }
            : undefined,
        });
      })
      .then((un) => {
        if (cancelled) un();
        else unlisten = un;
      })
      .catch((e: unknown) => {
        console.error('update-available listener failed', e);
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [tAbout]);

  const completeOnboarding = () => {
    try {
      window.localStorage.setItem(ONBOARD_KEY, '1');
    } catch {
      /* noop */
    }
    setOnboardingDone(true);
  };

  // Guards against a double-click on Print submitting the batch twice while
  // the first print_files round-trip is still in flight.
  const submittingRef = useRef(false);

  /**
   * Spool `list` as one batch and move to the Queue.
   *
   * Takes the files explicitly rather than reading the `files` state so a
   * context-menu selection can print the batch it just ingested, without
   * waiting for a render to make it readable here (FR-001).
   *
   * `hold` covers the context menu's "add to the queue" entry: the files must
   * land in the queue *without starting*, so the queue is paused first. That is
   * a deliberate global pause — the user asked for a queue they will start
   * themselves, and the Queue screen's start button is where they do it. The
   * pause is awaited inside the try, so a queue we failed to stop means the
   * batch is not submitted at all: printing something the user only asked to
   * enqueue is the one outcome worth erroring out over.
   */
  const submitFiles = async (
    list: UiFile[],
    profileId: string | null,
    hold: boolean,
  ) => {
    if (!isTauri()) {
      toast.error(t('errors.desktopOnly'));
      return;
    }
    if (list.length === 0 || submittingRef.current) return;
    submittingRef.current = true;
    const profileObj = findProfileIn(
      useProfilesStore.getState().profiles,
      profileId,
    );
    const req = buildPrintRequest(
      list.map((f) => f.path),
      profileObj,
      overrides,
    );
    const id = toast.loading(t('print.sending', { count: list.length }));
    try {
      if (hold) await pauseQueue();
      const batch = await printFiles(req);
      const n = batch.job_ids.length;
      toast.success(t('print.queued', { count: n }), {
        id,
        description: profileObj.name,
      });
      setFiles([]);
      setActive('queue');
    } catch (e) {
      // Keep the files so the user can fix and resubmit.
      toast.error(e instanceof Error ? e.message : String(e), { id });
    } finally {
      submittingRef.current = false;
    }
  };

  const handlePrint = () => submitFiles(files, profileChoice, false);

  /**
   * Act on one selection the OS context menu handed us (FR-001/FR-003).
   *
   * The paths go through the same `addPathsToJob` pipeline a drop or the file
   * picker uses — same inspection, same refusal toasts, same FR-004 folder
   * question — so a right-click behaves exactly like dragging the selection in.
   * What the integration additionally says is `action`:
   *
   * - `configure` — open the Print Job screen and let the user decide;
   * - `print-default` — print straight away with the requested (or default)
   *   profile, which is the whole point of the "Print with MaeclyPrint" entry;
   * - `queue` — enqueue without starting, for review in the Queue screen.
   *
   * Either way the app lands on the job screen first, so a selection whose
   * folders still need the recurse-or-not answer has somewhere to ask it — and
   * nothing is printed when the selection resolved to no printable file.
   */
  const applySelection = async (selection: Selection) => {
    if (selection.paths.length === 0) return;
    setActive('job');
    // An id the profile list does not know falls back to the app default
    // inside `findProfileIn`, so an integration naming a deleted profile
    // still prints.
    if (selection.profile_id) setProfileChoice(selection.profile_id);
    const added = await addPathsToJob(selection.paths, setFiles);
    if (selection.action === 'configure' || added.length === 0) return;
    await submitFiles(
      added,
      selection.profile_id,
      selection.action === 'queue',
    );
  };

  // Kept in a ref so the subscription below can stay mounted for the life of
  // the window while still calling the current closure (which reads `overrides`
  // and the profile list).
  const applySelectionRef = useRef(applySelection);
  applySelectionRef.current = applySelection;

  // FR-001/NFR-014: pick up the context-menu selections the backend parked.
  //
  // Two routes, one delivery point. A selection can be on the command line of
  // the process that opened this window (the shell extension launched us), or
  // it can arrive later — a second launch the single-instance plugin forwarded,
  // or a macOS Finder service message — while the window is already up. The
  // backend parks *both* in the same store and additionally emits
  // `selection-forwarded` for the second kind.
  //
  // So the event is treated as a doorbell, not as a payload: every route ends
  // in `take_pending_selections`, whose drain is atomic and destructive. That
  // is what makes a hot selection impossible to act on twice (once from the
  // event, once from the next drain) and impossible to miss.
  //
  // Subscribing *before* the first drain closes the other side of that race: a
  // selection parked between the two is picked up by the drain, and one parked
  // after it rings the doorbell we are already listening to.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const drain = async () => {
      let selections: Selection[];
      try {
        selections = await takePendingSelections();
      } catch (e) {
        console.error('pending selections could not be read', e);
        return;
      }
      // Deliberately not guarded by `cancelled`: the backend has already
      // handed these over, so dropping them here would lose the user's files
      // for good. Serial, so two selections cannot interleave their ingestion.
      for (const selection of selections) {
        await applySelectionRef.current(selection);
      }
    };

    events.selectionForwarded
      .listen(() => void drain())
      .then((un) => {
        if (cancelled) {
          un();
          return;
        }
        unlisten = un;
        void drain();
      })
      .catch((e: unknown) => {
        console.error('selection-forwarded listener failed', e);
        // A launch-time selection is still waiting; a missing subscription
        // only costs us the hot ones.
        void drain();
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  /**
   * Open the OS picker and feed the selection into the job pipeline.
   *
   * Files and directories are two mutually exclusive OS dialogs, so the caller
   * says which one it wants. Either way the paths land in `addPathsToJob`, so a
   * folder gets exactly the same FR-004 recurse-or-not question a drop would,
   * and the app moves to the job screen where that question is asked.
   */
  const pickPaths = async (directory: boolean) => {
    if (!isTauri()) {
      toast.error(t('errors.desktopOnly'));
      return;
    }
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = directory
        ? await open({ multiple: true, directory: true })
        : await open({
            multiple: true,
            filters: [
              {
                name: 'Printable files',
                extensions: [...PRINTABLE_EXTENSIONS],
              },
            ],
          });
      if (!picked) return;
      setActive('job');
      await addPathsToJob(Array.isArray(picked) ? picked : [picked], setFiles);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  /** Move the job to the next profile in the list (NFR-034 "switch profile"). */
  const switchProfile = () => {
    if (profiles.length === 0) return;
    const index = profiles.findIndex((p) => p.id === effectiveProfileId);
    // `-1` (the current profile vanished) restarts at the first entry.
    const next = profiles[(index + 1) % profiles.length] ?? profiles[0]!;
    setProfileChoice(next.id);
    toast.info(tHome('shortcuts.profileSwitched', { name: next.name }));
  };

  /**
   * Cancel the job the user would mean by "cancel": the one on the printer, or
   * the next one in line when nothing is running yet.
   */
  const cancelCurrentJob = () => {
    const target =
      uiJobs.find(
        (j) =>
          j.status === 'printing' ||
          j.status === 'converting' ||
          j.status === 'converted',
      ) ?? uiJobs.find((j) => j.status === 'queued' || j.status === 'paused');
    if (!target) {
      toast.info(tHome('shortcuts.nothingToCancel'));
      return;
    }
    setConfirm({
      title: tHome('cancelConfirm.title'),
      message: tHome('cancelConfirm.message', { file: target.file }),
      confirmLabel: tHome('cancelConfirm.action'),
      run: () => cancelJob(target.id),
    });
  };

  // NFR-034: every primary action has a shortcut, and every shortcut the UI
  // advertises does something. Suspended while a modal owns the keyboard.
  useAppShortcuts(
    {
      newJob: () => setActive('job'),
      printFolder: () => void pickPaths(true),
      print: () => {
        if (files.length === 0) {
          setActive('job');
          toast.info(tHome('shortcuts.printNeedsFiles'));
          return;
        }
        void handlePrint();
      },
      switchProfile,
      openQueue: () => setActive('queue'),
      cancel: cancelCurrentJob,
    },
    onboardingDone && confirm === null,
  );

  return (
    <>
      <MPAppShell>
        <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
          <MPSidebar
            active={active}
            onChange={setActive}
            queueCount={queueCount}
          />
          <main
            style={{
              flex: 1,
              display: 'flex',
              flexDirection: 'column',
              minWidth: 0,
              minHeight: 0,
            }}
          >
            {active === 'home' && (
              <HomeScreen
                onAction={(a) => {
                  if (a === 'newjob') setActive('job');
                  else if (a === 'queue') setActive('queue');
                  else void pickPaths(a === 'folder');
                }}
              />
            )}
            {active === 'job' && (
              <PrintJobScreen
                files={files}
                setFiles={setFiles}
                profile={effectiveProfileId}
                setProfile={setProfileChoice}
                overrides={overrides}
                setOverrides={setOverrides}
                density={density}
                onPrint={handlePrint}
              />
            )}
            {active === 'queue' && (
              <QueueScreen view={queueView} setView={setQueueView} />
            )}
            {active === 'printers' && <PrintersScreen />}
            {active === 'profiles' && (
              <ProfilesScreen
                selectedId={selectedProfileId}
                onSelect={setSelectedProfileId}
              />
            )}
            {active === 'settings' && (
              <SettingsScreen
                theme={resolved}
                setTheme={(t: ResolvedTheme) => setMode(t)}
                mode={mode}
                setMode={setMode}
                density={density}
                setDensity={setDensity}
              />
            )}
            {active === 'about' && <AboutScreen />}
          </main>
        </div>
        {confirm && (
          <ConfirmDialog state={confirm} onClose={() => setConfirm(null)} />
        )}
      </MPAppShell>
      <Toaster theme={resolved} />
      {!onboardingDone && <OnboardingFlow onComplete={completeOnboarding} />}
    </>
  );
}

export default App;
