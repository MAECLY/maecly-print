# ADR-0014: The SVG worker process is the resource boundary

## Status

Accepted

## Date

2026-10-10

## Context

[ADR-0012](./0012-system-web-view-renderer.md) moved SVG conversion (`usvg` + `svg2pdf`) into a hidden worker process (`pluryprint --worker svg`) so that a parser panic kills the worker, not the app. Memory and time were still bounded only by checks run *before* usvg: a size cap, an entity check, a node limit and a walk that estimated what usvg would expand the document to.

Three security reviews in a row (security notes 4 and 8, rounds 4 to 6) found a way past those checks. Each time the estimate and usvg disagreed on something: masks and clip paths with `objectBoundingBox` units, which usvg converts again for every reference (a 3.7 KB drawing built a million masks in 156 s); style-sheet `url()` references, which made the walk itself do unbounded work; embedded SVGs, which usvg parses with its own uncapped loader. Rasterising a filter region is another such cost: svg2pdf allocates the region's whole pixmap. A pre-check can only model usvg and svg2pdf, so a new version or a case nobody modelled will slip through again. HTML reached the same conclusion in round 5 (security note 7): pre-scanning cannot be the boundary.

## Decision

The worker process is the boundary for memory and time. The pre-checks stay as defence in depth.

- **Memory cap, enforced by the OS.** The parent starts the worker through `engine::convert::process::run_capped` with a 1 GiB cap (`svg::WORKER_MEMORY_CAP`).
  - **Windows:** a Job Object with `JOB_OBJECT_LIMIT_PROCESS_MEMORY`, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and an active-process limit of 1. Its completion port reports when the limit is hit. The worker waits until it is inside that job before it reads its input.
  - **macOS and Linux:** the worker lowers `RLIMIT_DATA` itself. macOS enforces neither `RLIMIT_DATA` nor `RLIMIT_AS` on `mmap`, so the parent also samples the worker's memory every 20 ms (its physical footprint on macOS, its resident set on Linux) and kills it past the cap.
- **Time:** the conversion timeout kills the worker as before.
- **Output:** the PDF is capped at 512 MB.
- **Reasons:** a worker stopped for memory ends the job with the terminal `svg_too_large`, which names the cap. One stopped for time ends with the terminal `tool_timed_out`.
- **References:** every external reference is decided in one place, usvg's `ImageHrefResolver`, on the exact string usvg parsed. Only `data:` and files inside the drawing's folder are allowed, judged by the shared `local_ref` predicate. An embedded SVG goes through the same caps as the drawing.
- **Pre-checks:** they must cost bounded work. The expansion walk charges every step it takes to one budget. They exist only so that an oversized drawing fails fast with the right reason.

## Alternatives Considered

- **Keep tightening the pre-checks.** Rejected. Each round closed one gap and the next review found another, because the checks model usvg's behaviour instead of measuring it.
- **Measure on usvg's own tree after it is built.** That covers the drawing usvg built (filter regions are checked this way), but the explosion happens while usvg builds the tree, so it cannot be the only defence.
- **A counting global allocator in the app.** Portable and precise, but it would put an atomic operation on every allocation of the whole app to protect one worker, and it does not cover memory a library maps directly.
- **`RLIMIT_AS` on macOS.** It is not enforced for `mmap`, which is where large allocations go.

## Consequences

### Positive

- A drawing that needs more memory than the cap can no longer exhaust the machine, whatever new expansion trick it uses. It ends as `svg_too_large`.
- The pre-checks can stay simple and over-estimate, because a miss costs at most the worker.

### Negative

- On macOS and Linux the cap is a sampling watchdog. A burst between two samples can go past the cap for up to 20 ms before the worker is killed.
- When no worker executable sits next to the current one (unit tests), the conversion runs on a thread without the OS cap.
- `libc` becomes a direct dependency on Unix, and the `windows` crate gains the Job Object, IO and Threading features.

### Risks / Open Questions

- ~~The render worker (HTML, Markdown and text) does not run under a memory cap yet.~~ Closed in round 7, see the amendment below.
- 1 GiB is a constant. A legitimate drawing that needs more (a very large filter region near the 64 Mpx pre-check) fails as `svg_too_large`.

## Amendment: round 7

- **The render worker is capped too.** HTML, Markdown and text (and RTF on macOS) render through `run_capped` with `CapScope::Tree` and a 2 GiB cap (`html::RENDER_WORKER_MEMORY_CAP`) for the worker and every helper it starts together. The SVG worker keeps `CapScope::Process`.
  - **Windows:** the WebView2 browser, renderer and GPU processes are the worker's children, so the job cannot limit them to one process. The tree job uses `JOB_OBJECT_LIMIT_JOB_MEMORY` (all processes together) with kill-on-close, and its completion port reports `JOB_OBJECT_MSG_JOB_MEMORY_LIMIT`. The worker waits until it is inside a job with a limit at or under the cap before it reads the page.
  - **macOS:** the parent's 20 ms watchdog sums the footprint of the worker's process group. WebKit's web-content and networking processes are XPC services that launchd starts, so they are outside that group and outside the cap. WebKit's own memory-pressure handling is all that bounds them.
  - A render worker stopped for memory ends the job with the new terminal reason `page_too_large`, which names the cap in MB.
- **Clip paths and masks are charged per use.** usvg shares the `userSpaceOnUse` ones, but svg2pdf writes them out again for every element that uses them, so a 3.6 KB nest of default-unit clip paths became a 78 MB PDF after 16 s. The expansion walk now charges every reference to a mask, clip path or filter in full, whatever its units. It re-uses the cost of the first walk of each one, so the check itself stays cheap. The worker already refused a PDF over 512 MB (`MAX_PDF_BYTES`) before writing it.

## Related

- [ADR-0012](./0012-system-web-view-renderer.md): workers for untrusted input.
- [print-engine.md §3.5](../04-architecture/print-engine.md) and [security-model.md](../04-architecture/security-model.md).
