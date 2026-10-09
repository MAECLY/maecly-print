# Print Settings

> **TL;DR** — Every printing setting MaeclyPrint exposes, mapped to the underlying engine flag (SumatraPDF on Windows, CUPS **`lp -o`** on macOS/Linux), with default value, implementation status, and driver-dependency notes. Eleven settings reach the spooler today, `quality`, `tray` and `collate` included (SumatraPDF cannot express `quality` or `collate`, and the job says so); per-printer tray/paper defaults from the Printers screen fill whatever a job leaves open; the rest are unimplemented.

A print setting is anything that can be different per job: the printer, paper, duplex, color, copies, range, etc. Some settings are universally supported by the OS spooler; others depend on the specific printer driver; a few would have to be synthesized via PDF preprocessing.

## Quick reference table

`JobSettings` (`src-tauri/src/queue/job.rs`) is the authoritative struct: `printer`, `duplex`, `color_mode`, `copies`, `paper_size`, `orientation`, `scaling`, `page_ranges`, `quality`, `tray`, `collate`. Anything not in it does not reach the engine.

| Setting | Type | Default | SumatraPDF (Win) | CUPS `lp` (macOS) | Status | Driver-dependent? |
|---|---|---|---|---|---|---|
| `printer` | string | system default | `-print-to "<name>"` | `-d <name>` | **Shipped** | no |
| `duplex` | enum: none / long / short | **none** | `simplex` / `duplex` / `duplexshort` | `-o sides=one-sided \| two-sided-long-edge \| two-sided-short-edge` | **Shipped** | yes (printer must support) |
| `color_mode` | enum: color / monochrome | color | `color` / `monochrome` | `-o ColorModel=Gray` for mono; colour sends `-o ColorModel=RGB` **only when the printer lists `RGB`**, else nothing (driver default) | **Shipped** | yes |
| `copies` | int 1–999 | 1 | `<N>x`, e.g. `3x` (only when N > 1) | `-n N` (only when N > 1) | **Shipped** | no |
| `page_ranges` | string `1-5,10` | all | bare `1-5,10`, prepended, no key | `-o page-ranges=1-5,10` | **Shipped** | no |
| `paper_size` | string: any size the printer reports (`A4`, `Letter`, `A6`, `4x6`, `Tabloid`…), or `auto` | A4 | `paper=<A2…A6, letter, legal, tabloid, statement>`; any other size is **not applied** and the preview says so; `auto` emits nothing | `-o media=<the printer's own keyword>`; `auto` emits nothing | **Shipped** | yes — validated against the printer's list when it reports one (`unsupported_setting` pause otherwise) |
| `orientation` | enum: portrait / landscape / auto | auto | `landscape` / `portrait`; **auto emits nothing** | `-o orientation-requested=3` (portrait) / `=4` (landscape); **auto emits nothing** | **Shipped** | no |
| `scaling` | enum: fit / shrink / none | fit | `fit` / `shrink` / `noscale` | `-o fit-to-page` for fit **and** shrink; none emits nothing | **Shipped** | no |
| `quality` | enum: draft / normal / high | normal | **not expressible**; the job carries a note saying so | `-o print-quality=3\|4\|5 -o cupsPrintQuality=Draft\|Normal\|High` | **Shipped** (CUPS); reported, not applied (Windows) | yes |
| `tray` | string | the printer's override, else driver default | `bin=<name>`; `auto` emits nothing | `-o InputSlot=<bin> -o media-source=<bin>`; `auto` emits nothing | **Shipped** | yes |
| `paper_size_custom` | width × height (mm) | — | would need preprocessing | `-o media=Custom.WxHmm` | Not implemented | yes |
| `n_up` | enum: 1 / 2 / 4 / 6 / 9 | 1 | would need `qpdf`/`pdfjam` preprocessing | `-o number-up=N` | Not implemented | yes |
| `collate` | bool | true | **not expressible** — a `# note:` says the driver's collation is used | `-o collate=true\|false` (only when copies > 1) | **Shipped** (CUPS) | yes |
| `margins` | object (mm) | driver default | would need preprocessing | not directly | Not implemented | partial |
| `watermark` | object | none | would need a preprocessing step | same | Not implemented | no (synthesized) |
| `separator_pages` | bool | false | would need concatenation | same | Not implemented | no (synthesized) |

Three things in that table contradict earlier drafts and are worth stating plainly:

- **`duplex` defaults to `none`, not to anything inherited.** `sides=` is emitted on every CUPS invocation precisely so an explicit "none" beats a driver default that duplexes.
- **Per-printer defaults fill gaps, never override choices.** The Printers screen's tray and paper defaults (`PrinterPrefs::overrides`) apply to a job only where it leaves the value open — no tray, or paper **Printer default** (`auto`). Precedence is explicit job/profile value → printer override → the printer's own default (nothing sent). The dispatcher, the CLI and the honesty preview all apply them the same way (`engine::print::apply_printer_overrides`).
- **`quality` and `tray` travel with every job.** `toJobSettings` (`src/lib/profiles.ts`) sends both from the profile. CUPS applies both; SumatraPDF applies the tray and states in the job that it cannot apply the quality. The tray is the driver's own bin name, passed verbatim, so a typo reaches the driver as typed. The Print Job screen offers no per-job override for either (see FR-041).
- **CUPS cannot distinguish `fit` from `shrink`.** It has a single `fit-to-page` toggle, so both map to it. Only SumatraPDF honours the distinction (`fit` vs `shrink`).

### `paper_size`

Any media name the printer reports can be chosen — the Print Job paper menu lists the classic four, then every other size the selected printer reports, plus **Printer default** (`auto`). The wire form is backward compatible: `a4`, `letter`, `legal` and `a3` still serialize lowercase, so stored profiles and `queue.json` load unchanged; any other name travels verbatim and is matched against the printer case-insensitively (`A4` also matches a Windows `A4 210 x 297 mm`). When the printer reports a list and the size is not on it, the job pauses with `unsupported_setting` rather than letting the driver substitute another size in silence. `auto` sends no media flag, so the printer's override from the Printers screen, else its own default, applies.

## Setting details

### `printer`

The destination printer. On a profile, the sentinel `"<default>"` defers to the default at print time; on `JobSettings` the same meaning is carried by `printer: None`. Resolution order, defined once in `ipc::printers::resolve_app_printer` and used by the queue, the CLI and the command preview: an explicit printer, then the file-type routing rule, then the org-policy default (`policy.json` `default_printer`), then the app default, then the OS default. Use `maeclyprint printers list` to enumerate available printers.

**Identity.** A printer may be named by its spooler **queue id** or by its **display name** (CUPS `printer-info`); both resolve, through `ipc::printers::match_installed`, to the queue id, and the queue id is what is spooled. A display name shared by several queues is treated as unresolved rather than guessed.

**Behavior on missing printer.** When the OS enumerates printers and none matches (or the name is ambiguous), the queue **pauses** the job with `PrinterMissing` so the user can pick a replacement, and the CLI exits with the `Printer` code. A failed or empty enumeration is not treated as proof of absence: the name is then handed to the spooler as is.

### `duplex`

| Value | Meaning |
|---|---|
| `none` | Single-sided. |
| `long` (long-edge) | Duplex flip on long edge — typical for portrait documents. |
| `short` (short-edge) | Duplex flip on short edge — typical for landscape or notepad-style. |

If the printer does not support duplex, the job fails explicitly with `EngineError::UnsupportedSetting { setting: "duplex", printer }` rather than silently falling back to single-sided. (The error is the generic `UnsupportedSetting`, not a dedicated `DuplexNotSupported` variant — duplex is simply the only setting that currently reaches it.)

### `color_mode`

| Value | Meaning |
|---|---|
| `color` | Print in color (or color-emulated grayscale on B&W printers). |
| `monochrome` | Force grayscale; saves ink/toner. |

There is no `auto` value. On CUPS, `color` emits no flag at all — the driver's own default is the colour path — and only `monochrome` forces `ColorModel=Gray`.

### `copies` and `collate`

`copies` is validated as `1..=999` by `JobSettings::validate` and sets the number of copies of the **whole job**. It is only emitted when greater than 1 (`<N>x` for SumatraPDF, `-n N` for `lp`).

`collate` is on `JobSettings` and on profiles (default `true`), set from the Print Job **Collate copies** switch or `--collate` / `--no-collate`:

- `collate = true` (default): full document repeated. (1,2,3 / 1,2,3)
- `collate = false`: each page repeated. (1,1 / 2,2 / 3,3)

CUPS receives `-o collate=true|false` whenever more than one copy is printed (one copy has nothing to collate, so nothing is sent). SumatraPDF's `-print-settings` has no collate keyword; with several copies the preview carries a `# note:` saying the driver's own collation is used, in the same spirit as the quality gap.

### `page_ranges`

A comma-separated range string: `"1-5,10,20-25"`. Whitespace is stripped before the string reaches SumatraPDF. Negative "count from the end" indices are **not** supported.

Validation is split deliberately. `JobSettings::validate` owns only what that layer can know — it rejects an empty range string so the UI can show an inline error early (FR-042). The range *grammar* is parsed by `engine::pagerange`, which owns it. Document-bounds checking happens at neither layer today: a range past the last page is passed to the spooler.

### `paper_size` and `paper_size_custom`

Four sizes are supported: A4 (default), Letter, Legal, A3. Custom sizes are **not implemented** on either platform; when they are, CUPS can express them (`-o media=Custom.WxHmm`) and Windows will need preprocessing.

If the printer's tray does not have the requested paper, behavior is driver-dependent (most prompt the user at the device).

### `orientation`

| Value | Meaning |
|---|---|
| `portrait` | Force portrait. |
| `landscape` | Force landscape. |
| `auto` (default) | Use the document's intrinsic orientation per page. |

### `scaling`

| Value | Meaning |
|---|---|
| `fit` (default) | Scale up or down so the page fits the paper, preserving aspect ratio. |
| `shrink` | Scale down only if oversize; never enlarge. |
| `none` | Print at 100%, may clip. |

### `tray`

> **Shipped.** `tray` is on `JobSettings`. `auto` (or blank) emits nothing, so the printer's default bin wins. Any other value is the driver's bin name, passed verbatim: CUPS gets `-o InputSlot=<bin> -o media-source=<bin>`, SumatraPDF gets `bin=<bin>` (a bin name containing a comma is dropped and the drop is logged).

An identifier for the printer's input bin, as the driver names it (Windows `DC_BINNAMES`, CUPS `InputSlot`) — useful for offices with letterhead in tray 2 and plain paper in tray 1. A blank name means "the printer's default bin".

- **SumatraPDF (Windows):** `bin=<name>`. When the job names no tray, the printer's saved default tray (Printers → *Defaults for this printer*) is used, as [printers-management](../05-features/printers-management.md) promises; the honesty preview shows the same `bin=`. A name containing a comma cannot be expressed in the comma-separated `-print-settings` list, so it is dropped and logged rather than split into wrong tokens.
- **CUPS:** `-o InputSlot=<name> -o media-source=<name>` for the job's tray, else the printer's saved default tray (the queue, the CLI and the preview fill it in through `apply_printer_overrides`).

### `n_up` (pages-per-sheet)

> **Not implemented.**

Sheets are subdivided into a 1×1, 1×2, 2×2, 2×3, or 3×3 grid. Sheet count = ceil(pages / n).

CUPS implements n-up natively (`number-up`); nothing emits that flag today. On Windows, SumatraPDF does not support it at all, so it would have to be implemented by pre-processing the PDF with [`pdfjam`](https://github.com/rrthomas/pdfjam) (a Perl wrapper for `pdfpages`) or `qpdf` and then printing the result. This adds a few seconds and one temp file per job.

### `quality`

> **Shipped on CUPS, reported on Windows.** `quality` is on `JobSettings` (`Draft`/`Normal`/`High`, defaulting to `Normal`). CUPS gets `-o print-quality=3|4|5` plus `-o cupsPrintQuality=Draft|Normal|High`. SumatraPDF has no quality switch, so a non-normal preset is not applied and the job carries a note saying so.

| Value | DPI hint |
|---|---|
| `draft` | ~150 dpi |
| `normal` (default) | ~300 dpi |
| `high` | ~600 dpi or more |

The actual DPI is the printer's choice; we pass a hint via the spooler.

### `margins`

> **Not implemented.**

Margin overrides (top/right/bottom/left in mm). Implemented by preprocessing the PDF: rendering each page onto a new page with the desired margins. Lossless if margins are larger than the original; centered if smaller.

This is **deferred to v2** because it interacts poorly with `scaling=fit` and `n_up`. Most users want the document's intrinsic margins.

### `watermark`

> **Not implemented.** There is no `[profile.watermark]` table in `profiles.toml`; the schema below is a proposal.

A textual or image overlay on every printed page. Configured per-profile.

```toml
[profile.watermark]
type = "text"           # or "image"
text = "CONFIDENTIAL"
opacity = 0.15
position = "diagonal"   # diagonal | header | footer | center
font_size = 72
color = "#ff0000"
```

Implemented by preprocessing with `qpdf` overlay. Image watermarks accept PNG with alpha channel.

### `separator_pages`

> **Not implemented.**

When printing N files in one job, insert a divider page between each file. The divider shows the next file's name and page count.

Implemented by generating a small PDF on the fly and concatenating into the job stream.

## How a setting becomes a real flag

Pseudocode for the resolution chain:

```
JobSettings  =  profile defaults, overridden by CLI/GUI flags
                ↓
JobSettings::validate      // copies in 1..=999, page_ranges non-empty
                ↓
engine::print::check_supported(settings, printer, duplex_supported, color_supported)
                ↓
sumatra::print_settings_string(settings)   |   cups::lp_opts(settings)
                ↓
spool(...)                                 // the only step that spawns anything
```

There are no "OS defaults" and no per-file overrides. The rendering step is the only place that knows engine-specific flag syntax, and it is split into two pure functions — `sumatra::print_settings_string` and `cups::lp_opts` — that the UI's command preview calls with the same arguments the spooler will, so the preview cannot drift from what actually runs. Adding a new engine means adding one such function.

## Driver dependencies

The rule, implemented in `engine::print::check_supported`, is narrower than earlier drafts claimed, and deliberately so:

**We hard-fail only where the failure would be invisible.**

1. The dispatcher reads driver capabilities from printer enumeration and passes them in (`check_supported` itself is pure, so it is trivially testable).
2. **Duplex is gated.** A driver that cannot duplex silently prints single-sided, and the user discovers it at the paper tray. A duplex request against a non-duplex printer therefore fails with `EngineError::UnsupportedSetting { setting, printer }`.
3. **Colour is deliberately not gated.** A colour job on a mono printer prints in grey — visible, expected, and not worth blocking. `color_supported` is accepted without complaint.

Nothing else is gated, because nothing else is rendered to a flag (see the *Stored, never applied* rows above).

There is no `--strict=false` flag and no "prompt to override or skip" flow; a gated setting fails the job.

## Per-engine flag matrices

Quick reference for developers writing the renderer:

### SumatraPDF `-print-settings` syntax

Comma-separated keywords inside one quoted string. SumatraPDF is order-independent, but `print_settings_string` emits a **fixed order** so the preview shown in the UI is stable and the output is testable:

```
page ranges, duplex, colour, scaling, paper, bin, copies, orientation
```

giving, for example:

```
"1-5,duplex,monochrome,fit,paper=A4,2x,landscape"
```

Full argument vector: `-print-to <printer> -print-settings <string> -silent -exit-when-done <pdf>`.

| Setting | Token emitted |
|---|---|
| Page range | `1-5,10` — bare, no key, whitespace stripped, **first** |
| Duplex none / long / short | `simplex` / `duplex` / `duplexshort` — always emitted |
| Colour / monochrome | `color` / `monochrome` — always emitted |
| Scaling fit / shrink / none | `fit` / `shrink` / `noscale` — always emitted |
| Paper size | `paper=A4` (also `Letter`, `Legal`, `A3`, and SumatraPDF's own `A2`, `A5`, `A6`, `tabloid`, `statement`); `auto` emits nothing; any other size cannot be named here, so it is left out and a `# note:` line in the preview says it is not applied |
| Copies | `<N>x` (e.g. `3x`), **only when N > 1** — SumatraPDF silently skips the `copies=N` spelling; collation cannot be expressed (preview note) |
| Orientation | `landscape` / `portrait`; `auto` emits nothing |
| Tray (bin) | `bin=<name>` — the job's tray, else the printer's saved default tray; omitted when there is none or the name contains a comma |

`simplex` and `noscale` are worth noting: the "none" cases are stated explicitly rather than omitted, so every setting the user chose is represented in the string.

### CUPS `lp -o` flags

The command is **`lp`**, not `lpr`. Vector: `lp -d <printer> -t <title> <opts…> <pdf>`.

| Setting | Flag emitted |
|---|---|
| Printer | `-d <name>` |
| Title | `-t <title>` |
| Sides (duplex) | `-o sides=one-sided \| two-sided-long-edge \| two-sided-short-edge` — always emitted |
| Paper | `-o media=<keyword>` in the printer's own spelling (`A4`, `Letter`, `4x6`, `Tabloid`, `w288h432`…); omitted only for `auto` |
| Colour model | Monochrome: `-o ColorModel=Gray`. Colour: `-o ColorModel=RGB` in the printer's spelling **only when the printer lists an `RGB` model** (so a queue that defaults to `Gray` still prints colour); otherwise nothing, and the driver default applies |
| Copies | `-n N` — `lp`'s own flag, **not** `-o copies=N`; only when N > 1, together with `-o collate=true\|false` |
| Orientation | `-o orientation-requested=3` portrait / `=4` landscape; `auto` emits nothing, so each page keeps its own orientation |
| Fit-to-page | `-o fit-to-page` for both `fit` and `shrink`; `none` emits nothing |
| Page ranges | `-o page-ranges=1-5,10` |
| Tray | `-o InputSlot=<bin> -o media-source=<bin>` when the job (or the printer's override) names one; `auto` emits nothing |
| Quality | `-o print-quality=3 \| 4 \| 5 -o cupsPrintQuality=Draft \| Normal \| High` — always emitted |
| N-up | *not emitted* — CUPS supports `number-up`, but no renderer produces it |

`sides` and `media` are always emitted so an explicit user choice wins over any driver default.

Every `-o name=value` value must be a single CUPS keyword. `lp` passes `-o` through `cupsParseOptions`, which splits on whitespace and honours quotes and backslashes, so a tray named `Tray1 copies=999` would quietly add a `copies=999` option. A value containing whitespace, a control character, a quote, a backslash or `=` is therefore refused, never quoted. The capability gate (`check_supported`) pauses the job as `unsupported_setting` and names the value, and `spool` repeats the check before it runs `lp`. The test page applies the same rule to the printer's saved paper and tray. Windows is unaffected: SumatraPDF bin and paper names may contain spaces.

## Settings that we deliberately do not expose

| Setting | Why omitted |
|---|---|
| Per-page settings (different paper per page) | Vanishingly rare for batch jobs; complex UI |
| Booklet imposition | Niche; deferred to v2 if demand emerges |
| Punching/stapling | Deeply driver-specific; doable but large UI surface for narrow audience |
| Job priority in spooler | Operating-system-level concern |
| Banner pages (system) | Use our own separator pages instead |

## Round-tripping settings

A profile's settings are serialized as `[[profile]]` entries in `profiles.toml`, validated, and rendered to engine flags at print time.

**There is information loss**, in two specific places, and it is worth being precise about it:

- `quality` cannot be expressed to SumatraPDF on Windows; the job reports that instead of applying it.
- `scaling` loses the `fit`/`shrink` distinction on CUPS, which has only `fit-to-page`.

For the other settings that reach a renderer on SumatraPDF, the mapping is lossless.

"Save current job settings as profile" is not implemented.
