import { useState, type ReactNode } from 'react';
import {
  MPInput,
  MPSectionLabel,
  MPSegmented,
  MPSelect,
  MPSwitch,
  MPTopBar,
} from '@/components/ui';
import { MP_PROFILES } from '@/lib/mockData';
import type { ResolvedTheme, Density } from '@/lib/theme';

type Channel = 'stable' | 'beta' | 'nightly';

interface SettingsScreenProps {
  theme: ResolvedTheme;
  setTheme: (t: ResolvedTheme) => void;
  density: Density;
  setDensity: (d: Density) => void;
}

interface RowProps {
  label: string;
  hint?: string;
  children: ReactNode;
}

function Row({ label, hint, children }: RowProps) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '1fr auto',
        gap: 20,
        padding: '14px 0',
        borderBottom: '1px solid var(--mp-border)',
        alignItems: 'center',
      }}
    >
      <div>
        <div style={{ fontSize: 13, fontWeight: 500 }}>{label}</div>
        {hint && (
          <div
            style={{
              fontSize: 11.5,
              color: 'var(--mp-textMuted)',
              marginTop: 2,
            }}
          >
            {hint}
          </div>
        )}
      </div>
      <div>{children}</div>
    </div>
  );
}

export function SettingsScreen({
  theme,
  setTheme,
  density,
  setDensity,
}: SettingsScreenProps) {
  const [shellExt, setShellExt] = useState(true);
  const [channel, setChannel] = useState<Channel>('stable');

  return (
    <>
      <MPTopBar title="Settings" />
      <div
        className="mp-scroll"
        style={{
          flex: 1,
          overflow: 'auto',
          padding: '18px 24px',
          maxWidth: 760,
        }}
      >
        <MPSectionLabel>Appearance</MPSectionLabel>
        <Row label="Theme" hint="Follows OS by default.">
          <MPSegmented<ResolvedTheme>
            value={theme}
            onChange={setTheme}
            options={[
              { value: 'light', icon: 'sun', label: 'Light' },
              { value: 'dark', icon: 'moon', label: 'Dark' },
            ]}
          />
        </Row>
        <Row
          label="Density"
          hint="Comfortable for reading, compact for big batches."
        >
          <MPSegmented<Density>
            value={density}
            onChange={setDensity}
            options={[
              { value: 'comfortable', label: 'Comfortable' },
              { value: 'compact', label: 'Compact' },
            ]}
          />
        </Row>

        <div style={{ height: 24 }} />
        <MPSectionLabel>Shell integration</MPSectionLabel>
        <Row
          label="Right-click menu"
          hint="Adds 'Print with MaeclyPrint' to Windows 11 modern menu."
        >
          <MPSwitch
            checked={shellExt}
            onChange={() => setShellExt(!shellExt)}
          />
        </Row>
        <Row
          label="Filter unsupported files"
          hint="Hide menu when no printable files are selected."
        >
          <MPSwitch checked={true} onChange={() => {}} />
        </Row>
        <Row
          label="Default profile"
          hint="Used when invoking from context menu directly."
        >
          <MPSelect
            value="p2"
            options={MP_PROFILES.map((p) => ({
              value: p.id,
              label: p.name,
            }))}
          />
        </Row>

        <div style={{ height: 24 }} />
        <MPSectionLabel>Engine</MPSectionLabel>
        <Row
          label="Concurrent jobs"
          hint="Higher values can saturate the printer queue."
        >
          <MPInput mono value="2" suffix="jobs" />
        </Row>
        <Row
          label="Conversion timeout"
          hint="Per-file LibreOffice/Chromium conversion budget."
        >
          <MPInput mono value="60" suffix="seconds" />
        </Row>
        <Row
          label="Max file size"
          hint="Files larger than this prompt for confirmation."
        >
          <MPInput mono value="200" suffix="MiB" />
        </Row>

        <div style={{ height: 24 }} />
        <MPSectionLabel>Updates</MPSectionLabel>
        <Row label="Channel">
          <MPSegmented<Channel>
            value={channel}
            onChange={setChannel}
            options={[
              { value: 'stable', label: 'Stable' },
              { value: 'beta', label: 'Beta' },
              { value: 'nightly', label: 'Nightly' },
            ]}
          />
        </Row>
        <Row
          label="Check on startup"
          hint="Delayed by 30 seconds to keep launch fast."
        >
          <MPSwitch checked={true} onChange={() => {}} />
        </Row>

        <div style={{ height: 24 }} />
        <MPSectionLabel>Privacy</MPSectionLabel>
        <Row
          label="Crash reports"
          hint="Off by default. Sent only when toggled on."
        >
          <MPSwitch checked={false} onChange={() => {}} />
        </Row>
        <Row
          label="Anonymous usage metrics"
          hint="MaeclyPrint never enables this without consent."
        >
          <MPSwitch checked={false} onChange={() => {}} />
        </Row>
      </div>
    </>
  );
}
