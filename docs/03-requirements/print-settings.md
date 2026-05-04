# Print Settings

> **TL;DR** — Every printing setting MaeclyPrint exposes, mapped to the underlying engine flag (SumatraPDF for Windows, CUPS `lpr -o` for macOS), with default value, MVP/v1/future status, and driver-dependency notes.

A print setting is anything that can be different per job: the printer, paper, duplex, color, copies, range, etc. Some settings are universally supported by the OS spooler; others depend on the specific printer driver; a few we synthesize via PDF preprocessing.

## Quick reference table

| Setting | Type | Default | SumatraPDF (Win) | CUPS (macOS) | MVP | v1 | Driver-dependent? |
|---|---|---|---|---|---|---|---|
| `printer` | string | system default | `-print-to "<name>"` | `-P <name>` | ✓ | — | no |
| `duplex` | enum: none / long / short | none | `-print-settings duplex` | `-o sides=two-sided-long-edge \| short-edge` | ✓ | — | yes (printer must support) |
| `color_mode` | enum: color / monochrome | color | `-print-settings color \| monochrome` | `-o ColorModel=CMYK \| Gray` | ✓ | — | yes |
| `copies` | int ≥ 1 | 1 | `-print-settings copies=N` | `-o copies=N` | ✓ | — | no |
| `page_ranges` | string `1-5,10` | all | `-print-settings <range>` | `-o page-ranges=1-5,10` | ✓ | — | no |
| `paper_size` | enum: A4 / Letter / Legal / A3 | A4 | `-print-settings paper=A4` | `-o media=A4` | ✓ | — | yes |
| `paper_size_custom` | width × height (mm) | — | not supported (use preprocessing) | `-o media=Custom.WxHmm` | ✗ | ✓ | yes |
| `orientation` | enum: portrait / landscape / auto | auto | `-print-settings landscape` | `-o landscape` | ✓ | — | no |
| `scaling` | enum: fit / shrink / none | fit | `-print-settings fit \| shrink` | `-o fit-to-page` | ✓ | — | no |
| `tray` | int (bin id) | driver default | `-print-settings bin=N` | `-o InputSlot=Tray2` | ✗ | ✓ | yes |
| `n_up` | enum: 1 / 2 / 4 / 6 / 9 | 1 | not supported (use `qpdf`/`pdfjam` preprocessing) | `-o number-up=N` | ✗ | ✓ | yes |
| `collate` | bool | true | implicit (depends on copies handling) | `-o Collate=true` | ✗ | ✓ | yes |
| `quality` | enum: draft / normal / high | normal | (driver default) | `-o print-quality=3 \| 4 \| 5` | ✗ | ✓ | yes |
| `margins` | object (top/right/bottom/left in mm) | driver default | not supported (preprocess with `qpdf`) | not directly | ✗ | ✗ | partial |
| `watermark` | object (text/image, opacity, position) | none | preprocess with watermark step | preprocess | ✗ | ✓ | no (synthesized) |
| `separator_pages` | bool | false | preprocess (concatenate dividers) | preprocess | ✗ | ✓ | no (synthesized) |

✓ = supported in this milestone, ✗ = not yet, — = already in earlier milestone.

## Setting details

### `printer`

The destination printer. Special value `"<default>"` resolves to the OS default printer at print time. Use `maeclyprint printers list` to enumerate available printers.

**Behavior on missing printer.** If a profile names a printer that no longer exists (e.g., user uninstalled it), the job is paused, not failed, and the user is prompted to choose a replacement.

### `duplex`

| Value | Meaning |
|---|---|
| `none` | Single-sided. |
| `long` (long-edge) | Duplex flip on long edge — typical for portrait documents. |
| `short` (short-edge) | Duplex flip on short edge — typical for landscape or notepad-style. |

If the printer does not support duplex, the job MUST fail explicitly with `DuplexNotSupported` rather than silently fall back to single-sided.

### `color_mode`

| Value | Meaning |
|---|---|
| `color` | Print in color (or color-emulated grayscale on B&W printers). |
| `monochrome` | Force grayscale; saves ink/toner. |

For mixed jobs (some pages color, some B&W), use `auto` (v1.0+) which lets the driver decide.

### `copies` and `collate`

`copies` sets the number of copies of the **whole job**. When `copies > 1`:

- `collate = true` (default): full document repeated. (1,2,3 / 1,2,3)
- `collate = false`: each page repeated. (1,1 / 2,2 / 3,3)

Most modern printers handle collation in firmware. The application sets the flag and trusts the driver.

### `page_ranges`

A comma-separated range string: `"1-5,10,20-25"`. Whitespace ignored. Negative numbers count from the end (`-1` = last page) — supported in v1.

Validation: ranges must be positive, ascending within a span, and within document bounds. Invalid ranges fail at validation time, not at print time.

### `paper_size` and `paper_size_custom`

MVP supports the four common sizes (A4, Letter, Legal, A3). v1 adds custom sizes (width × height in millimeters). Custom sizes only work on macOS in v1; Windows requires more involved preprocessing and is deferred to v2.

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

Integer index of the printer's input bin. The list of available trays is queried from the driver. Most users leave this at default; useful for offices with letterhead in tray 2, plain paper in tray 1.

### `n_up` (pages-per-sheet)

Sheets are subdivided into a 1×1, 1×2, 2×2, 2×3, or 3×3 grid. Sheet count = ceil(pages / n).

CUPS implements n-up natively (`number-up`). On Windows, SumatraPDF does not; we implement it by pre-processing the PDF with [`pdfjam`](https://github.com/rrthomas/pdfjam) (a Perl wrapper for `pdfpages`) or `qpdf` and then printing the result. This adds a few seconds and one temp file per job.

### `quality`

| Value | DPI hint |
|---|---|
| `draft` | ~150 dpi |
| `normal` (default) | ~300 dpi |
| `high` | ~600 dpi or more |

The actual DPI is the printer's choice; we pass a hint via the spooler.

### `margins`

Margin overrides (top/right/bottom/left in mm). Implemented by preprocessing the PDF: rendering each page onto a new page with the desired margins. Lossless if margins are larger than the original; centered if smaller.

This is **deferred to v2** because it interacts poorly with `scaling=fit` and `n_up`. Most users want the document's intrinsic margins.

### `watermark`

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

When printing N files in one job, insert a divider page between each file. The divider shows the next file's name and page count.

Implemented by generating a small PDF on the fly and concatenating into the job stream.

## How a setting becomes a real flag

Pseudocode for the resolution chain:

```
final_settings = merge(
    OS_defaults,
    profile.settings,
    cli_overrides_or_gui_overrides,
    per_file_overrides   // future
)
validate(final_settings)
flag_string = render_for_engine(final_settings, target_engine)
spawn(engine, ..flag_string)
```

The `render_for_engine` step is the only place that knows about engine-specific flag syntax. Adding a new engine means adding one renderer module, not touching the rest of the system.

## Driver dependencies

Some settings (duplex, color, tray, paper sizes beyond standard) only work if the printer's driver supports them. The application:

1. Queries driver capabilities at print time via OS APIs.
2. If a profile requests an unsupported setting, the job pauses with `UnsupportedSetting { setting, printer }`.
3. The user is prompted to override or skip the setting.

This is more honest than silently falling back. Users who want silent fallback can opt in via `--strict=false`.

## Per-engine flag matrices

Quick reference for developers writing the renderer:

### SumatraPDF `-print-settings` syntax

Comma-separated keywords inside one quoted string:

```
"duplex,monochrome,fit,paper=A4,copies=2,1-5"
```

| Setting | Token |
|---|---|
| Duplex | `duplex` (long-edge) or `duplexshort` |
| Color | `color` |
| Monochrome | `monochrome` |
| Fit | `fit` |
| Shrink | `shrink` |
| Paper size | `paper=A4` (also `Letter`, `Legal`, `A3`) |
| Copies | `copies=N` |
| Tray (bin) | `bin=N` |
| Orientation | `landscape` or `portrait` |
| Page range | `1-5,10` (loose, no key) |

### CUPS `lpr -o` flags

| Setting | Flag |
|---|---|
| Sides (duplex) | `-o sides=one-sided \| two-sided-long-edge \| two-sided-short-edge` |
| Paper | `-o media=A4 \| Letter \| Legal \| A3` |
| N-up | `-o number-up=N` |
| Landscape | `-o landscape` |
| Fit-to-page | `-o fit-to-page` |
| Copies | `-o copies=N` |
| Page ranges | `-o page-ranges=1-5,10` |
| Collate | `-o Collate=true \| false` |
| Tray | `-o InputSlot=Tray2` |
| Color model | `-o ColorModel=CMYK \| Gray` |
| Quality | `-o print-quality=3 \| 4 \| 5` |

## Settings that we deliberately do not expose

| Setting | Why omitted |
|---|---|
| Per-page settings (different paper per page) | Vanishingly rare for batch jobs; complex UI |
| Booklet imposition | Niche; deferred to v2 if demand emerges |
| Punching/stapling | Deeply driver-specific; doable but large UI surface for narrow audience |
| Job priority in spooler | Operating-system-level concern |
| Banner pages (system) | Use our own separator pages instead |

## Round-tripping settings

A profile's settings are serialized as TOML, validated, and rendered to engine flags at print time. There is no information loss between TOML and engine flags for any MVP setting.

A v1 feature is "Save current job settings as profile" which captures the user's overrides into a new profile.
