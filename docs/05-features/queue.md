# Queue

> **TL;DR** — The **Queue** screen shows every print job MaeclyPrint is tracking — active, pending, and recently finished — with live status and per-job controls (pause, resume, retry, cancel). It is a thin, event-driven view over the backend dispatcher: the queue store subscribes to a `queue-updated` event that carries the whole queue on every change, so the UI never polls.

This document is the source of truth for the **content and behavior** of the Queue screen (`src/screens/QueueScreen.tsx` + `src/screens/QueueViews/*`) and the queue store (`src/stores/queueStore.ts`). The dispatcher state machine that produces these states lives in the [print engine](../04-architecture/print-engine.md) and the queue module; the on-disk shape is in the [data model](../04-architecture/data-model.md) §4; the wire contract is the [IPC & CLI doc](../04-architecture/ipc-and-cli.md).

## Screen layout

A single scrolling body with three interchangeable views (List / Board / Timeline), chosen with a segmented control in the top bar.

```
┌─ Queue ──────────────────  2 active · 1 pending · 5 done   [List|Board|Timeline] [⏸ Pause queue] [✕ Cancel all] [🗑 Clear done] ┐
├───────────────────────────────────────────────────────────────────────────────────────────────────┤
│ ACTIVE · 2                                                                                          │
│ 📄 report.pdf     PRINTING  ▓▓▓▓▓▓▓░░  10:42:03            [⏸]      [✕]                             │
│ 🖼 chart.png      CONVERTING ▓▓▓░░░░░  10:42:04            [⏸]      [✕]                             │
│ PENDING · 1                                                                                          │
│ 📄 cover.pdf      QUEUED    ░░░░░░░░                       [⏸]      [✕]                             │
│ RECENT · 5                                                                                           │
│ 📄 invoice.pdf    DONE      ▓▓▓▓▓▓▓▓  10:41:00 → 10:41:06                                            │
│ 📄 memo.pdf       FAILED    ▓▓▓▓▓▓▓▓  · print_failed: …    [⟳ retry]                                 │
└───────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Top bar** — title `Queue`, a subtitle that in List view reads `{active} active · {pending} pending · {done} done` (Board/Timeline show a view description instead), and — **only in List view** — a **Pause queue / Resume queue** toggle, a **Cancel all** button (disabled when nothing is unfinished; confirms first) and a **Clear done** button (disabled when there is no finished job; confirms before clearing).
- **List view** — jobs grouped into **Active** (printing / converting / converted), **Pending** (queued / paused), and **Recent** (done / failed / cancelled). Each row shows a file glyph, name, `profile · printer` (plus a note or error tail), a status dot + label, a progress bar, a `started → finished` clock, and the per-state action buttons.
- **Board view** — a kanban across the job phases (read-only visualization).
- **Timeline view** — jobs in chronological order with durations (read-only visualization).

## The queue store

`useQueueStore` (Zustand) is the UI's single source of truth. `init()` (idempotent):

1. Fetches the initial `queue_status` snapshot.
2. Subscribes to **`queue-updated`** — the authoritative full [`QueueSnapshot`], which **replaces** store state wholesale on every backend mutation (FR-030), so the UI can never drift from the backend.
3. Subscribes to **`job-progress-event`** — advisory per-job progress, merged into the matching job by id (ignored for unknown ids). It is UI sugar on top of the snapshots, not persisted.

Load status is `idle | loading | ready | unavailable | error`; outside the desktop host the screen shows a "Queue unavailable" notice (the queue is owned by the desktop app).

Derived views are recomputed once per snapshot: `counts` (active/pending/done), `queuePaused`, and `recentDone` (the last five terminal jobs, newest first — consumed by Home's *Recent activity*).

## Job states

A job moves through the pipeline `Queued → Converting → Converted → Printing → Done`, with `Paused`, `Failed`, and `Cancelled` as off-ramps. The wire enum (`JobState`, lowercase) and the UI grouping:

| State        | Group   | Meaning                                                        |
| ------------ | ------- | ------------------------------------------------------------- |
| `queued`     | Pending | Created, not yet started.                                     |
| `converting` | Active  | A converter is producing the cache PDF.                      |
| `converted`  | Active  | Cache PDF ready; waiting for a free printer ("Ready to print"). |
| `printing`   | Active  | Handed to the OS spooler.                                    |
| `paused`     | Pending | User-paused, or interrupted by shutdown (resumable).         |
| `done`       | Recent  | Printer reported completion (terminal).                     |
| `failed`     | Recent  | Terminal error after retries; `error` carries a code + message. |
| `cancelled`  | Recent  | User cancelled (terminal).                                   |

Terminal states are `done`, `failed`, `cancelled`. A job paused by shutdown recovery carries the `interrupted_by_shutdown` error code and renders as *"Interrupted by shutdown · resume to continue"* rather than a hard failure.

## Dispatcher semantics (what the UI reflects)

The backend dispatcher is the single writer; the UI only issues commands and renders snapshots. The behavior worth knowing as a user:

- **Concurrency** — up to **2 conversions** run at once (configurable, applied without a restart); **exactly one** job prints at a time (spoolers serialize anyway, and it keeps ordering legible).
- **`done` means printed, not spooled** — after `lp` / SumatraPDF hands the PDF to the OS, the job stays `printing` and the dispatcher follows the OS job (CUPS through `ipptool get-job-attributes`, falling back to `lpstat -W`; Windows through the spooler's `GetJob`), polling after 1 s, 2 s, 4 s, 8 s and then every 10 s. `completed` (or the job leaving the queue) → `done`; cancelled in the system queue → `cancelled` (`os_cancelled`); aborted by the spooler → `failed` (`print_aborted`, not retried automatically). A job the OS queue is **holding** stays `printing` with the note *"Held in the system print queue"*; a printer that needs attention (paper out, offline) shows *"The printer needs attention"*. The print slot stays taken until the OS job is gone, so the next job never reaches the spooler early.
- **Print timeout** — the whole print (spooling plus that wait) is bounded by **Settings → Print timeout** (default 300 s, `0` turns it off). Running out fails the job with `print_timeout` but **leaves the OS job in place** (the printer may still print it), and the message says so. If the host cannot report job state at all (no `ipptool`/`lpstat`, or the spooled job could not be identified on Windows), the job is marked `done` on hand-off, as before.
- **Pause queue** gates *starting new work only*. A job already converting/printing runs to completion; a conversion that finishes while the queue is paused reaches `converted` but won't advance to `printing` until you resume.
- **Retry & failure** — transient conversion/print failures are retried automatically **3 times** (after **1 s, 5 s and 25 s**), then the job is marked `failed`. A print failure returns the job to `converted` and **keeps its cached PDF** (no re-conversion); a conversion failure returns it to `queued`.
- **User-fixable errors don't auto-retry** — a requested setting the driver can't honor (`unsupported_setting`: duplex on a non-duplex printer, or a paper size the printer does not list), a missing dependency (`dependency_missing`, e.g. SumatraPDF not found on Windows), a printer that is gone (`printer_unavailable`) or one disabled in MaeclyPrint (`printer_disabled`) **pauses** the job with the error so you can fix it and resume. Resuming a job paused on its printer re-resolves the printer first, off the dispatcher loop. If the printer is back, the job goes there. Only when the printer is **gone** from the OS does the job follow the preferences instead (file-type route, app default, OS default), and the queue then shows the new target. A printer that is still installed but cannot be used as named (two queues share its name, or it is disabled in MaeclyPrint) is never swapped for another one: Resume is refused with what to fix (`resume_printer_ambiguous`, `resume_printer_disabled`).
- **Pause/Resume** — you can pause a `queued`, `converting`, or `converted` job; pausing a `converting` job aborts the in-flight conversion and discards its partial cache (it re-converts on resume). Pausing a `printing` job is refused — the OS already owns it. Resume returns a job to `converted` if its cached PDF still exists, otherwise to `queued`, always clearing the error.
- **Cancel** — cancels any non-terminal job. A `printing` job is marked `cancelled` immediately. If its OS job is already known, it is cancelled at the spooler right away and the printer is free for the next job. If it is still spooling, the OS job is cancelled (and the cache deleted) when the spool returns, and until then the next job waits, so a cancelled job never shares the printer with the next one. The List view asks for confirmation before cancelling a printing job.

Full state-machine and retry details are in the [data model](../04-architecture/data-model.md#4-queuejson--live-print-queue) and [print engine](../04-architecture/print-engine.md).

## Persistence & recovery

The queue is persisted to `queue.json` (atomic write on every transition), capped at the 200 most-recent terminal jobs. On launch, any job left `converting`/`printing` when the app closed is moved to `paused` with `interrupted_by_shutdown`, and orphaned cache PDFs are swept. See [data model §4](../04-architecture/data-model.md#4-queuejson--live-print-queue).

## Actions

| Action           | Where                | Behavior                                                                 |
| ---------------- | -------------------- | ------------------------------------------------------------------------ |
| **Pause queue**  | top bar (List)       | Stops starting new work; in-flight jobs finish. Toggles to **Resume queue**. |
| **Cancel all**   | top bar (List)       | Cancels every unfinished job (`cancel_all`) after a confirm that states the count; disabled when nothing is unfinished. No undo — the backend cannot un-cancel, so the confirm is the safety net. |
| **Clear done**   | top bar (List)       | Hides all terminal jobs at once and calls `clear_done` 5 s later; the toast's **Undo** restores them inside that window. When the window closes the toast is dismissed, and an Undo that still arrives too late says so instead of claiming the jobs were kept (NFR-032). |
| **Pause**        | job row              | Pauses a queued/converting/converted job.                                |
| **Resume**       | job row              | Returns a paused job to the pipeline.                                     |
| **Retry**        | job row              | Re-runs a failed job from the start (attempts reset).                     |
| **Cancel**       | job row              | Cancels a non-terminal job (confirms if it is printing).                  |

Row actions are disabled while a command for that job is in flight (`pendingIds`), so a double click cannot send it twice. A cancel the backend refuses because the job finished (or vanished) in the meantime is reported as a translated "had already finished" note rather than the raw `InvalidState` message.

Batch completion toasts (FR-070) count every job the batch had this session, including finished jobs a **Clear done** already removed, so a batch of three is never announced as "1 of 1".

## Per-platform differences

| Concern              | Windows                                   | macOS / Linux                               |
| -------------------- | ----------------------------------------- | ------------------------------------------- |
| Print backend        | SumatraPDF (synchronous; no OS job id)    | CUPS `lp` (returns a job id)                |
| Cancel a printing job| Marked cancelled in-app (no OS un-spool)  | Best-effort `cancel <id>` at CUPS + in-app  |

Because SumatraPDF reports no spooler job id, a Windows job that is cancelled during the brief print window is reconciled in-app only; on CUPS the recorded job id lets MaeclyPrint also cancel it at the spooler.

## IPC mapping

| UI surface              | IPC command / event                          | Notes                                       |
| ----------------------- | -------------------------------------------- | ------------------------------------------- |
| Initial paint           | `queue_status() => QueueSnapshot`            | One-shot snapshot for first render          |
| Live updates            | `queue-updated` event (`QueueSnapshot`)      | Full snapshot on every mutation; replaces state |
| Progress bars           | `job-progress-event` (`{id,phase,percent}`)  | Advisory, throttled ≥ 100 ms, not persisted |
| Pause / Resume / Retry / Cancel (row) | `pause_job` / `resume_job` / `retry_job` / `cancel_job` | Per-id; return `JobNotFound` / `InvalidState` on bad transitions |
| Pause queue / Resume queue | `pause_queue()` / `resume_queue()`        | Whole-queue gate                            |
| Clear done              | `clear_done() => number`                     | Returns count removed                       |
| Cancel all              | `cancel_all() => number`                     | Returns count cancelled                     |

See [ipc-and-cli.md](../04-architecture/ipc-and-cli.md) (§ jobs/queue) for the full command and event tables.

## Implementation status

- **M0** — front-end only, mock jobs (superseded).
- **M1 (current)** — **real, event-driven queue, no mock data.** The store consumes real `queue_status` + `queue-updated` + `job-progress-event`; the List view drives real `pause/resume/retry/cancel` and the top bar drives `pause_queue`/`resume_queue`/`clear_done`. Rust: `src-tauri/src/queue/{mod,job,persist,dispatcher}.rs`, `src-tauri/src/ipc/{jobs,events}.rs`. Frontend: `queueStore.ts`, `QueueScreen.tsx`, `QueueViews/*`.
- **Follow-up** — richer Board/Timeline (currently read-only visualizations), a per-job detail drawer, and a CLI `queue` subcommand (the CLI is documented but not yet implemented — see [ipc-and-cli.md](../04-architecture/ipc-and-cli.md) § CLI).
