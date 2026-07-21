import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';

import { MPSidebar, type ScreenId } from '@/components/AppShell/MPSidebar';
import { MPAppShell } from '@/components/AppShell/MPAppShell';
import { Toaster } from '@/components/AppShell/Toaster';
import { useDensity, useTheme, type ResolvedTheme } from '@/lib/theme';
import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  findProfileIn,
} from '@/lib/profiles';
import { isTauri, printFiles } from '@/lib/ipc';
import { addPathsToJob } from '@/lib/jobFiles';
import { useProfilesStore } from '@/stores/profilesStore';
import { useQueueStore } from '@/stores/queueStore';
import { useSettingsStore } from '@/stores/settingsStore';
import { useT } from '@/i18n/I18nProvider';
import type { UiFile } from '@/types/domain';

import { HomeScreen } from '@/screens/HomeScreen';
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

function readOnboardingDone() {
  try {
    return window.localStorage.getItem(ONBOARD_KEY) === '1';
  } catch {
    return false;
  }
}

function App() {
  const t = useT('common');
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

  const handlePrint = async () => {
    if (!isTauri()) {
      toast.error(t('errors.desktopOnly'));
      return;
    }
    if (files.length === 0 || submittingRef.current) return;
    submittingRef.current = true;
    const profileObj = findProfileIn(
      useProfilesStore.getState().profiles,
      profileChoice,
    );
    const req = buildPrintRequest(
      files.map((f) => f.path),
      profileObj,
      overrides,
    );
    const id = toast.loading(t('print.sending', { count: files.length }));
    try {
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
                  if (a === 'queue') setActive('queue');
                }}
                onSelectJob={() => setActive('queue')}
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
      </MPAppShell>
      <Toaster theme={resolved} />
      {!onboardingDone && <OnboardingFlow onComplete={completeOnboarding} />}
    </>
  );
}

export default App;
