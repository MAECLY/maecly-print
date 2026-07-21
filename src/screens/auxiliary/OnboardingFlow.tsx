import { useState } from 'react';
import { Icon, type IconName } from '@/components/Icon';
import { MPAppIcon } from '@/components/Brand';
import { shortcutLabel } from '@/components/AppShell/MPAppShell';
import { MPButton, MPCard } from '@/components/ui';
import { useT } from '@/i18n/I18nProvider';
import { findProfileIn } from '@/lib/profiles';
import { useProfilesStore } from '@/stores/profilesStore';

/**
 * First-run tour.
 *
 * Every step must describe something this build actually does: the tour is the
 * first thing a user reads, and a promise it cannot keep (a right-click entry
 * that does not exist, a Settings switch that was never shipped) is a broken
 * feature as far as they are concerned. The shell integration is named as
 * planned, in the future tense, and the default profile is read from the real
 * profile list instead of being retyped here.
 */
const STEPS = [
  { key: 'welcome', icon: 'maecly' },
  { key: 'printers', icon: 'printer' },
  { key: 'cli', icon: 'cmd' },
  { key: 'profiles', icon: 'sliders' },
  { key: 'ready', icon: 'check' },
] as const satisfies readonly { key: string; icon: IconName }[];

interface OnboardingFlowProps {
  onComplete: () => void;
}

export function OnboardingFlow({ onComplete }: OnboardingFlowProps) {
  const t = useT('home');
  const profiles = useProfilesStore((s) => s.profiles);
  const [step, setStep] = useState(0);
  const isLast = step === STEPS.length - 1;
  // Non-null: `step` starts at 0 and the Back/Continue handlers below keep it
  // within [0, STEPS.length - 1].
  const cur = STEPS[step]!;
  const params = {
    name: findProfileIn(profiles, null).name,
    command: 'maeclyprint print --files "*.pdf"',
    shortcut: shortcutLabel('newJob'),
  };

  return (
    <div
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(15, 14, 11, 0.45)',
        backdropFilter: 'blur(4px)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 50,
        animation: 'mp-fadein .3s ease',
      }}
    >
      <MPCard
        padding={28}
        style={{
          width: 520,
          maxWidth: '90vw',
          boxShadow: '0 8px 24px rgba(0,0,0,0.18), 0 0 0 1px var(--mp-border)',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 14,
            marginBottom: 18,
          }}
        >
          {step === 0 ? (
            <MPAppIcon size={48} />
          ) : (
            <div
              style={{
                width: 48,
                height: 48,
                borderRadius: 10,
                background: 'var(--mp-accentSoft)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
              }}
            >
              <Icon name={cur.icon} size={22} color="var(--mp-accent)" />
            </div>
          )}
          <div style={{ flex: 1 }}>
            <div
              style={{
                fontSize: 16,
                fontWeight: 600,
                letterSpacing: '-0.01em',
              }}
            >
              {t(`onboarding.${cur.key}.title`)}
            </div>
            <div
              style={{
                fontSize: 12.5,
                color: 'var(--mp-textMuted)',
                marginTop: 4,
                lineHeight: 1.5,
              }}
            >
              {t(`onboarding.${cur.key}.body`, params)}
            </div>
          </div>
        </div>

        {/* Progress dots — decorative, so the step is also stated in text for
            anyone who cannot see them. */}
        <div
          role="status"
          aria-label={t('onboarding.progress', {
            step: step + 1,
            total: STEPS.length,
          })}
          style={{
            display: 'flex',
            justifyContent: 'center',
            gap: 6,
            marginBottom: 20,
          }}
        >
          {STEPS.map((s, i) => (
            <span
              key={s.key}
              style={{
                width: i === step ? 22 : 6,
                height: 6,
                borderRadius: 3,
                background:
                  i === step
                    ? 'var(--mp-accent)'
                    : i < step
                      ? 'var(--mp-borderStrong)'
                      : 'var(--mp-border)',
                transition: 'all .25s',
              }}
            />
          ))}
        </div>

        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            gap: 10,
          }}
        >
          <MPButton variant="ghost" size="sm" onClick={() => onComplete()}>
            {t('onboarding.skip')}
          </MPButton>
          <div style={{ display: 'flex', gap: 8 }}>
            {step > 0 && (
              <MPButton
                variant="secondary"
                icon="arrowl"
                size="sm"
                onClick={() => setStep(step - 1)}
              >
                {t('onboarding.back')}
              </MPButton>
            )}
            <MPButton
              variant="primary"
              iconRight={isLast ? 'check' : 'arrowr'}
              size="sm"
              onClick={() => {
                if (isLast) onComplete();
                else setStep(step + 1);
              }}
            >
              {isLast ? t('onboarding.start') : t('onboarding.continue')}
            </MPButton>
          </div>
        </div>
      </MPCard>
    </div>
  );
}
