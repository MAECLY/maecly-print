import type { JobState } from '@/types/domain';

type Status =
  | JobState
  | 'queued'
  | 'converting'
  | 'converted'
  | 'printing'
  | 'done'
  | 'failed'
  | 'paused'
  | 'cancelled';

interface StatusDotProps {
  status: Status;
}

const CFG: Record<string, { c: string; pulse?: boolean }> = {
  queued: { c: 'var(--mp-textDim)' },
  converting: { c: 'var(--mp-info)', pulse: true },
  converted: { c: 'var(--mp-info)' },
  printing: { c: 'var(--mp-accent)', pulse: true },
  done: { c: 'var(--mp-success)' },
  failed: { c: 'var(--mp-error)' },
  paused: { c: 'var(--mp-warn)' },
  cancelled: { c: 'var(--mp-textDim)' },
};

export function StatusDot({ status }: StatusDotProps) {
  const cfg = CFG[status] ?? { c: 'var(--mp-textDim)' };
  return (
    <span
      style={{
        display: 'inline-block',
        width: 7,
        height: 7,
        borderRadius: '50%',
        background: cfg.c,
        flexShrink: 0,
        animation: cfg.pulse ? 'mp-pulse 1.4s ease-in-out infinite' : 'none',
        boxShadow: cfg.pulse ? `0 0 0 3px ${cfg.c}22` : 'none',
      }}
    />
  );
}
