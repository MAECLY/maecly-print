import { useEffect, useState } from 'react';
import { toast } from 'sonner';

import { MPSidebar, type ScreenId } from '@/components/AppShell/MPSidebar';
import { MPAppShell } from '@/components/AppShell/MPAppShell';
import { Toaster } from '@/components/AppShell/Toaster';
import { useDensity, useTheme, type ResolvedTheme } from '@/lib/theme';
import { MP_FILES_SAMPLE, MP_QUEUE_SAMPLE, MP_PROFILES } from '@/lib/mockData';

import { HomeScreen } from '@/screens/HomeScreen';
import {
  PrintJobScreen,
  type PrintJobOverrides,
} from '@/screens/PrintJobScreen';
import { QueueScreen, type QueueView } from '@/screens/QueueScreen';
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
  const [files, setFiles] = useState(MP_FILES_SAMPLE);
  const [profile, setProfile] = useState(MP_PROFILES[1].id); // Duplex BW default
  const [overrides, setOverrides] = useState<PrintJobOverrides>({});

  // Queue state
  const [jobs, setJobs] = useState(MP_QUEUE_SAMPLE);
  const [queueView, setQueueView] = useState<QueueView>('list');

  // Profile selection
  const [selectedProfileId, setSelectedProfileId] = useState(MP_PROFILES[0].id);

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

  const completeOnboarding = () => {
    try {
      window.localStorage.setItem(ONBOARD_KEY, '1');
    } catch {
      /* noop */
    }
    setOnboardingDone(true);
  };

  const handlePrint = () => {
    const id = toast.loading(`Sending ${files.length} files to printer…`);
    setActive('queue');
    setTimeout(() => {
      toast.success('Job submitted', {
        id,
        description: `${files.length} files queued · ${MP_PROFILES.find((p) => p.id === profile)?.name}`,
      });
    }, 1200);
  };

  const queueCount = jobs.filter((j) =>
    ['queued', 'paused', 'converting', 'printing'].includes(j.status),
  ).length;

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
              <QueueScreen
                jobs={jobs}
                setJobs={setJobs}
                view={queueView}
                setView={setQueueView}
              />
            )}
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
