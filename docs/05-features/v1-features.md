# v1 Features (v0.2.0 → v1.0.0)

> **TL;DR** — Post-MVP additions that round MaeclyPrint into a polished v1: full Office and HTML support, more print settings, multilanguage UI, accessibility compliance, and code-signed updates with channels.

These features are sequenced into intermediate releases (`v0.2`, `v0.3`, …) before `v1.0`. The order may shift based on user feedback after MVP.

## v0.2 — Multi-format

The headline feature of v0.2 is **format breadth**. After v0.2, MaeclyPrint covers ~95% of the documents an office user needs to print.

### New formats

| Format | Converter | Notes |
|---|---|---|
| `.docx`, `.doc` | LibreOffice headless → PDF | First-run prompts user to install LibreOffice if absent |
| `.xlsx`, `.xls` | LibreOffice | |
| `.pptx`, `.ppt` | LibreOffice | |
| `.odt`, `.ods`, `.odp` | LibreOffice | |
| `.rtf`, `.txt` | LibreOffice | |
| `.html`, `.htm` | Chromium headless `--print-to-pdf` | Local files only; remote HTML deferred |
| `.md`, `.markdown` | Pandoc → PDF | Pandoc required as external dep |

### Dependency detection

The Settings page gains a **Dependencies** panel showing the status of LibreOffice / Chromium / Pandoc / ImageMagick (✓ Found / ✗ Missing) with download links and a refresh button.

### New print settings

- **Tray** (input bin) — pulled from driver capabilities at print time
- **Collate** (true/false)

### Quality of life

- The "Configure print job" dialog gains a **preview** pane for PDFs (using PDFium rendering).
- Drag-drop in the GUI accepts folders and recurses with a confirmation prompt.
- Recent files / recent jobs shown on the Home page.

## v0.3 — Watermark, separator, polish

### Watermarks

- Per-profile watermark configuration: text or image, opacity, position (diagonal / header / footer / center), font size, color.
- Implemented by preprocessing the PDF with a `qpdf` overlay step.

### Separator pages

- Optional divider page between files in a multi-file job.
- Divider shows next file's name and page count.
- Configurable from per-profile settings.

### Profile import/export

- Export a profile to `<name>.maecly-profile.toml`.
- Import via drag-drop on the Profiles page or `maeclyprint profiles import <file>`.

### Per-job overrides remembered

- "Save these overrides as a new profile" button on the Configure dialog.

## v0.4 — Internationalization

### Languages

- **Spanish (es-ES, es-MX)** lands as the second language, alongside English.
- All UI strings externalized in `i18n/{lang}.json`.
- ICU MessageFormat for plurals and gendered forms.
- Language selector in Settings; first run uses OS locale if matching.

### RTL preparedness

- Switch to logical CSS properties (`margin-inline-start`, etc.).
- No hard-coded `left`/`right` in component styles.
- RTL itself ships in v2 with Arabic/Hebrew translations.

## v0.5 — Accessibility (WCAG 2.1 AA)

- All screens pass `axe-core` with zero violations.
- Full keyboard navigation, visible focus indicators (≥3:1 contrast).
- Screen-reader compatibility verified with NVDA (Win) and VoiceOver (Mac).
- High-contrast mode follows OS preference.
- Reduce-motion preference respected for animations/transitions.
- Color is never the sole carrier of meaning (status uses icon + label + color).

## v0.6 — Update channels

- **Stable** (default), **Beta**, **Nightly** channels.
- User can switch in Settings; downgrading channel requires confirmation.
- `https://updates.maecly.com/maeclyprint/{channel}.json` per channel.
- Nightly: built every night from `main`, signed with self-signed cert.
- Beta: cut from `main` weekly, EV-signed.
- Stable: cut from `release/*` branches, EV-signed.

## v0.7 — N-up & custom paper

### N-up

- Paper subdivisions: 1×1, 1×2, 2×2, 2×3, 3×3.
- Implemented via `pdfjam` (Perl wrapper) preprocessing on Windows; `lpr -o number-up=N` natively on macOS.
- Per-profile setting.

### Custom paper sizes

- Width × height in millimeters.
- macOS only at first (CUPS supports natively); Windows needs more work and lands in v0.9.

## v0.8 — Quality, queue depth, history

- **Quality** setting: draft / normal / high (~150 / ~300 / ~600 dpi hint).
- **Queue concurrency**: configurable from 1–8 simultaneous jobs (default 2).
- **Job history** persisted indefinitely (until user clears) with full search/filter.
- Export history to CSV.

## v0.9 — Polish & hardening

- All `FR-*` requirements with `MUST` priority covered by tests.
- All `NFR-*` requirements measured in CI.
- Crash reporter (Sentry-compatible, opt-in, off by default).
- First-run wizard refined based on user testing.
- Documentation site refreshed with screenshots.

## v1.0 — Windows GA

- Code signing with EV cert in production for ≥3 months: full SmartScreen reputation accumulated.
- Public release on `maecly.com` and announcement.
- Stable channel only; beta/nightly remain available.
- 14-day rollback window if critical regression discovered.
- Issue templates and SLAs for triage published.
- Metrics:
  - 5,000+ unique downloads (target)
  - <0.5% crash rate
  - 95%+ job success rate (excluding hardware errors)
  - 10+ external contributors with merged PRs

## Cross-cutting upgrades that span v0.2 → v1.0

These don't fit a single milestone; they accrue over multiple releases.

- **Test coverage** climbs from ~50% (MVP) to ≥70% (v1.0).
- **Release automation** matures: every push to a release branch produces signed installer artifacts ready for promotion.
- **Doc site** moves from raw `docs/` to a published static site (Astro or Docusaurus) on `docs.maecly.com` — but the markdown source remains in this repo.
- **Telemetry-free crash reporting** matures: opt-in, end-to-end encrypted, self-hosted Sentry-compatible.
- **Performance tracking**: Tauri's built-in instrumentation captured in CI to detect regressions.

## Explicitly **not** in v1

| Feature | Lives in |
|---|---|
| macOS support | v1.1 (M5) |
| Linux support | v2.x if community demand |
| Hot folder | v2 |
| Multi-printer load balancing | v2 |
| Email/cloud folder monitoring | v2 |
| OCR | v2 |
| CAD/Visio/EPUB | v2 |
| Plugin API | v2 |
| REST API | v2 |
| Margins override | v2 |
| Per-page settings | never |
| Booklet imposition | v2 |

## v1.0 definition of done

A v1.0 release tag is created when:

1. All `MUST` priority FRs and NFRs covered by automated tests.
2. Manual QA matrix (formats × printers × OS versions × languages) passes 100%.
3. Documentation review complete (no broken links, all decisions reflected).
4. Code-signing keys rotated and stored in HSM.
5. `updates.maecly.com` infrastructure verified, including rollback procedure.
6. CHANGELOG.md updated with full v1.0 entry.
7. Public announcement copy ready.
8. Maintainer-on-call rotation defined for the first 14 days post-release.
