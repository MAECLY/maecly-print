import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';

import { MPSidebar, type ScreenId } from '@/components/AppShell/MPSidebar';
import { MPAppShell } from '@/components/AppShell/MPAppShell';
import { Toaster } from '@/components/AppShell/Toaster';
import { useDensity, useTheme, type ResolvedTheme } from '@/lib/theme';
import {
  BUILT_IN_PROFILES,
  buildPrintRequest,
  findProfile,
} from '@/lib/profiles';
import { isTauri, printFiles } from '@/lib/ipc';
import { addPathsToJob } from '@/lib/jobFiles';
import { useQueueStore } from '@/stores/queueStore';
import { pluralize } from '@/lib/format';
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
  const { resolved, setMode } = useTheme();
  const { density, setDensity } = useDensity();
  const [active, setActive] = useState<ScreenId>('home');
  const [onboardingDone, setOnboardingDone] = useState(readOnboardingDone);

  // Print job state
  const [files, setFiles] = useState<UiFile[]>([]);
  const [profile, setProfile] = useState(BUILT_IN_PROFILES[1]!.id); // Duplex BW default
  const [overrides, setOverrides] = useState<PrintJobOverrides>({});

  // Queue state (jobs live in the queue store; App owns only the view toggle)
  const [queueView, setQueueView] = useState<QueueView>('list');
  const initQueue = useQueueStore((s) => s.init);
  const queueCount = useQueueStore((s) => s.counts.active + s.counts.pending);

  // Profile selection
  const [selectedProfileId, setSelectedProfileId] = useState(
    BUILT_IN_PROFILES[0]!.id,
  );

  // Initial theme application
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
      toast.error('Printing requires the desktop app');
      return;
    }
    if (files.length === 0 || submittingRef.current) return;
    submittingRef.current = true;
    const req = buildPrintRequest(
      files.map((f) => f.path),
      profile,
      overrides,
    );
    const id = toast.loading(
      `Sending ${files.length} ${pluralize(files.length, 'file')} to printer…`,
    );
    try {
      const batch = await printFiles(req);
      const n = batch.job_ids.length;
      toast.success(`${n} ${pluralize(n, 'job')} queued`, {
        id,
        description: findProfile(profile).name,
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
                profile={profile}
                setProfile={setProfile}
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
