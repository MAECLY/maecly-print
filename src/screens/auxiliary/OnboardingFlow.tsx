import { useState } from 'react';
import { Icon } from '@/components/Icon';
import { MPAppIcon } from '@/components/Brand';
import { MPButton, MPCard } from '@/components/ui';

const STEPS = [
  {
    title: 'Welcome to MaeclyPrint',
    body: 'Open source batch printing for Windows and Mac. No 15-file limit, no Adobe required, no $149 license fee.',
    icon: 'maecly' as const,
  },
  {
    title: 'Detect your printers',
    body: "We'll list every printer your system has installed — including 'Microsoft Print to PDF' on Windows and 'Save as PDF' on Mac.",
    icon: 'printer' as const,
  },
  {
    title: 'Add to right-click menu',
    body: 'Adds a "Print with MaeclyPrint" entry to the right-click menu in Windows Explorer and macOS Finder. You can disable it any time in Settings.',
    icon: 'openwith' as const,
  },
  {
    title: 'Pick a default profile',
    body: 'Quick (color, simplex, A4) is enabled by default. Two more built-ins: Duplex BW and Photo Color.',
    icon: 'sliders' as const,
  },
  {
    title: 'Ready to print',
    body: 'Right-click any number of files, choose MaeclyPrint, and watch them queue up.',
    icon: 'check' as const,
  },
];

interface OnboardingFlowProps {
  onComplete: () => void;
}

export function OnboardingFlow({ onComplete }: OnboardingFlowProps) {
  const [step, setStep] = useState(0);
  const isLast = step === STEPS.length - 1;
  const cur = STEPS[step];

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
              {cur.title}
            </div>
            <div
              style={{
                fontSize: 12.5,
                color: 'var(--mp-textMuted)',
                marginTop: 4,
                lineHeight: 1.5,
              }}
            >
              {cur.body}
            </div>
          </div>
        </div>

        {/* Progress dots */}
        <div
          style={{
            display: 'flex',
            justifyContent: 'center',
            gap: 6,
            marginBottom: 20,
          }}
        >
          {STEPS.map((_, i) => (
            <span
              key={i}
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
            Skip
          </MPButton>
          <div style={{ display: 'flex', gap: 8 }}>
            {step > 0 && (
              <MPButton
                variant="secondary"
                icon="arrowl"
                size="sm"
                onClick={() => setStep(step - 1)}
              >
                Back
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
              {isLast ? 'Get started' : 'Continue'}
            </MPButton>
          </div>
        </div>
      </MPCard>
    </div>
  );
}
