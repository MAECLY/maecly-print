# Timeline

> **TL;DR.** With one developer at a sustained pace, MaeclyPrint reaches Windows 1.0 in roughly 14–18 weeks and macOS 1.1 another 4–6 weeks after that. All numbers below carry a 20% buffer and assume nobody else helps.

This document is a planning aid, not a contract. Estimates are honest but not promises. If a milestone slips, the buffer absorbs the first 20%; beyond that, the slip is visible and we replan rather than crunch. See [milestones.md](./milestones.md) for the scope each estimate covers and [release-strategy.md](./release-strategy.md) for how internal estimates relate to public version commitments.

## Assumptions

These assumptions condition every number below. Change one of them and the estimate moves.

| Assumption | Value |
|---|---|
| Developer count | 1 (single maintainer) |
| Hours per week | ~25 sustained (part-time, evenings + weekends) |
| Buffer | +20% on every milestone |
| Reference date | 2026-05-03 |
| Holiday/PTO budget | ~3 weeks per year, distributed |
| External blockers (cert procurement, notarization queue) | Counted explicitly where applicable |

If full-time (~40h/week sustainable): divide all single-dev numbers by ~1.5 for a rough re-estimate. We do not use the full-time numbers as the plan because we want a calendar that survives life.

## Per-milestone duration

Estimates are in **calendar weeks**, not engineering-weeks. A "3 week" milestone at 25h/week is roughly 75 hours of focused work.

| Milestone | Raw estimate | With 20% buffer | Notes |
|---|---|---|---|
| M0 — Foundation | 1 week | 1.2 weeks | Mostly docs (this work). |
| M1 — Print Core (Win) | 3–4 weeks | 3.6–4.8 weeks | Tokio queue + SumatraPDF integration is the long pole. |
| M2 — Shell Integration (Win) | 3–4 weeks | 3.6–4.8 weeks | C++ COM DLL + Sparse MSIX learning curve. |
| M3 — Multi-format (Win) | 2–3 weeks | 2.4–3.6 weeks | Each adapter is small; integration tests dominate. |
| M4 — Auto-update + Polish (Win) | 2 weeks | 2.4 weeks | EV cert procurement (3 weeks lead) runs in parallel. |
| M5 — macOS Port | 4–6 weeks | 4.8–7.2 weeks | Notarization + Finder Sync Extension are unknowns. |

**Totals:**

- **Windows 1.0 GA (M0 → M4):** 11–14 weeks raw, 13.2–16.8 weeks with buffer → call it **14–18 weeks** for planning (~3.5–4.5 months).
- **macOS 1.1 GA (+ M5):** add 4.8–7.2 weeks → **18–25 weeks total** (~4.5–6 months).

## ASCII Gantt

Each `#` represents one calendar week. Buffer weeks shown as `:`. Times are sequential (one milestone at a time, single dev).

```
Week:    1   5   10  15  20  25
M0:      #
M1:      :####
M2:          :####
M3:               :###
M4:                  :##
                      ▲ EV cert, SmartScreen seed (parallel)
M5:                    :######
                            ▲ Apple notarization slack
```

Translated to calendar dates assuming a 2026-05-04 (Monday) start of M1:

| Milestone | Target start | Target finish | Public artifact |
|---|---|---|---|
| M0 | 2026-04-26 | 2026-05-03 | Repo browsable (no release) |
| M1 | 2026-05-04 | 2026-06-07 | `0.1.0-alpha` (internal) |
| M2 | 2026-06-08 | 2026-07-12 | `0.1.0-beta` (public) |
| M3 | 2026-07-13 | 2026-08-09 | `0.2.0` (public) |
| M4 | 2026-08-10 | 2026-08-30 | `1.0.0` Windows GA |
| M5 | 2026-08-31 | 2026-10-18 | `1.1.0` macOS GA |

These dates carry the 20% buffer baked in. They are upper bounds; if M1 finishes early, M2 starts early.

## Public vs internal milestones

Not every milestone produces a release the public sees:

| Milestone | Public-facing? | What we say externally |
|---|---|---|
| M0 | No | "Repo is up; docs are landing." Status: pre-alpha. |
| M1 | No | Internal alpha only. Distributed to <10 friendly testers via direct download link. No GitHub Release announcement. |
| M2 | Yes (beta) | First public download. Marked clearly as beta. SmartScreen warning expected (self-signed). |
| M3 | Yes (stable-track) | First "release" stable users get. Still pre-1.0, semver allows breaking changes. |
| M4 | Yes (GA) | The 1.0.0 announcement. Press list, social, project landing page update. |
| M5 | Yes (Mac GA) | macOS launch. Coordinated with the Mac-using community. |

## Buffer policy

- **First 20% slip:** absorbed by the milestone buffer. No replan, no public communication.
- **20–40% slip:** the milestone buffer is exhausted but the next milestone's buffer can cover. We acknowledge internally, no external comms.
- **>40% slip:** explicit replan. Update this document, update milestones, post a status update. We do not silently push dates.

We do not use buffer to add scope. Buffer absorbs unknowns; if it's free, the milestone ends early.

## What can compress the timeline

Listed in rough order of impact. None are assumed in the baseline plan.

1. **Second contributor** on the C++ shell extension (M2). Could save 1–2 weeks because that's where most "unfamiliar territory" lives for a Rust-focused dev.
2. **Pre-purchased EV certificate** before M3 starts. Saves zero days if we're already buying it at M3 start, but mitigates the risk of a 4-week procurement delay blocking M4.
3. **Skip Spanish translation in 1.0**, defer to 1.0.1. Saves ~3-5 days.
4. **Skip macOS Universal binary**, ship Apple Silicon only first. Saves ~3-5 days in M5 but cuts addressable user base by ~30% on that platform.

## What can stretch the timeline

Listed in rough order of probability.

1. **EV cert procurement delay.** CAs occasionally take 4 weeks instead of 1–2 for vetting. Mitigation: order at start of M3.
2. **Apple notarization unexpected entitlement issues.** Often takes 1–2 days per round-trip; can absorb 1–2 weeks if multiple rounds are needed. Mitigation: prototype the pipeline in M5 week 1.
3. **SumatraPDF or LibreOffice major version bump** in the middle of a milestone. Changes CLI flags, breaks integration tests. Mitigation: pinned versions, scheduled bumps.
4. **A printer driver that misbehaves** in ways that require Win32 voodoo. Mitigation: support matrix is explicitly limited (M3 doc); exotic printers go to backlog.
5. **Burnout / life events.** Single-dev plans fail this way more often than they fail technically. Mitigation: 25h/week (not 40), buffer on every milestone, monthly self-checkpoint.

## How we report progress

- **Weekly:** internal note (private) on what shipped this week, blockers, next-week plan.
- **Per milestone:** GitHub milestone closes, brief retrospective in `docs/06-roadmap/retros/` (created post-M0).
- **Public:** GitHub Releases for every public artifact; CHANGELOG.md updated atomically with the release tag; project README banner updated for major status changes.

## Re-estimation cadence

This document is updated:

- At the close of every milestone (actuals captured, next milestones re-estimated if needed).
- Whenever a >40% slip is forecast (per buffer policy above).
- Annually on the anniversary of M0 close, regardless of state.

If the dates in the table above are more than 4 weeks past the current date with no update, treat them as stale and ask the maintainer for a refresh.
