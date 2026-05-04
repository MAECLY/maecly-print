# Trademark Policy

> **TL;DR.** "MaeclyPrint" and the MaeclyPrint logo are unregistered trademarks of the MaeclyPrint Project. Refer to us, link to us, write about us — that's all welcome and protected as fair use. Do not use the name or logo to imply our endorsement of your product or service. Forks must use a different name.

This policy explains how the MaeclyPrint name, logo, and visual identity may and may not be used. It is independent of the [GPLv3 license](./license.md) on the source code: copyright and trademark are different rights.

## Why a trademark policy

GPLv3 protects the **source code** but says nothing about names and logos. Without an explicit trademark policy:

- Someone could ship a malware-laced fork branded "MaeclyPrint" and damage user trust.
- Companies could imply we endorse their products by using our logo on packaging.
- Confusion in the market could fragment the user community.

This policy exists to prevent those harms while keeping the door wide open for legitimate use (writing about MaeclyPrint, linking to it, integrating with it).

## What's covered

| Asset | Status |
|---|---|
| The word **"MaeclyPrint"** | Unregistered trademark, asserted by the MaeclyPrint Project |
| The MaeclyPrint logo (icon and wordmark) | Unregistered trademark, copyright held by the project |
| The colors and typography defined in our brand guide | Visual identity, not a registered mark, but covered by trade dress conventions |
| Domain names: `maecly.com`, `updates.maecly.com` | Owned by the project |

We have not formally registered "MaeclyPrint" with the USPTO or other trademark offices. We may do so as the project matures. Even unregistered, common-law trademark protections apply.

## Permitted uses (no permission needed)

These uses are **fine** without asking us:

- **Referring to MaeclyPrint by name** in articles, blog posts, books, videos, social media, comparison reviews, and other editorial content.
- **Linking to** the MaeclyPrint website or GitHub repository, including using our logo in the link if it's clearly a link (not a product affiliation).
- **Using the name in URLs** for non-confusing purposes: `mysite.com/blog/maeclyprint-review` is fine.
- **Saying "uses MaeclyPrint"** if your product genuinely uses MaeclyPrint, in a way that's clearly descriptive (not an endorsement claim). Example: "PrintBatch Pro is built on MaeclyPrint" is OK; "Endorsed by MaeclyPrint" is not (unless we actually endorsed you).
- **Distributing unmodified MaeclyPrint** (binaries we released) using our name and logo as we shipped them.
- **Educational use**: training material that teaches users how to use MaeclyPrint, mentioning the name and using screenshots.

These are forms of **nominative fair use**: using the trademark to refer to the actual thing it identifies, without implying false sponsorship.

## Uses requiring permission

These uses require written permission (open an issue tagged `trademark` or email):

- **Branded merchandise** (t-shirts, mugs, stickers featuring the MaeclyPrint logo).
- **Conference materials** that prominently feature the MaeclyPrint logo as if it's a sponsor.
- **Certification programs** ("MaeclyPrint Certified Partner") — we don't currently offer these but may in the future.
- **Domain names** that contain "maeclyprint" and could be confused with official ones (`maeclyprint.io`, `maeclyprint-pro.com`). If you want to register one as a fan site or community resource, ask us first; we usually say yes if the intent is clear.
- **Use in product names** of commercial products that are not MaeclyPrint itself ("MaeclyPrint Plus" sold by a third party). We will generally decline these to prevent market confusion.

## Forbidden uses

These uses are **not permitted** under any circumstances:

- **Naming a fork "MaeclyPrint"** or anything confusingly similar (e.g. "MaeclyPrint Lite", "MaeclyPrint X", "Maecly-Print"). Forks must rebrand.
- **Using the name or logo to imply official endorsement** of a third-party product or service we have not endorsed.
- **Using the logo on commercial packaging** of a product that includes MaeclyPrint, in a way that suggests we co-released the product.
- **Using the name to mislead users** about the source or sponsorship of a download (e.g. a domain `maeclyprint-download.com` distributing modified or malicious binaries).
- **Misusing the logo** (recoloring it to something hostile, distorting it, combining it with marks of other entities to create confusion).
- **Using the logo as part of your own logo** (compositing our icon into yours).

## Forks

If you fork MaeclyPrint (which GPLv3 explicitly permits) and **distribute** the fork:

- **Rename it.** Use a clearly different name. Examples of acceptable rebrand: "PrinterStack" (totally different), "MyPrinter" (clearly distinct).
- **Remove or replace our logo.** Use your own visual identity.
- **Be clear about the relationship** in your README: "This is a fork of MaeclyPrint v0.5.0. Original project: https://maecly.com." (Linking back is encouraged.)
- **Don't use the original copyright notices alone** — add your own to indicate authorship of changes.

If your fork is small (a personal patchset, not distributed publicly) and you don't redistribute it widely, no rename is required.

## Logo usage guidelines (when permitted)

When you have permission (or are using the logo in an explicitly permitted way like linking to us), follow these conventions:

### Files

Logo files live at `docs/brand/`:

```
docs/brand/
├── logo-mark.svg              # The icon (square, 1:1)
├── logo-wordmark.svg          # The wordmark (horizontal)
├── logo-lockup.svg            # Mark + wordmark together
├── logo-mark-light-bg.png     # Raster, for light backgrounds
├── logo-mark-dark-bg.png      # Raster, for dark backgrounds
├── logo-wordmark-light-bg.png
├── logo-wordmark-dark-bg.png
└── README.md                  # Usage instructions
```

Use the SVG files when possible (they scale cleanly). Use PNG when SVG isn't supported.

### Color palette

| Color | Hex | Use |
|---|---|---|
| Primary | `#2D5BFF` | Logo mark, primary buttons, links |
| Primary Dark | `#1939B8` | Pressed states, dark mode primary |
| Neutral 900 | `#0F172A` | Main text on light backgrounds |
| Neutral 100 | `#F1F5F9` | Backgrounds, subtle borders |
| Accent (Warning) | `#F59E0B` | Warning states (amber) |
| Accent (Error) | `#DC2626` | Error states (red) |

When pairing the logo with text, use the Neutral 900 color or the user's text color.

### Clearspace

Around the logo, leave clearspace equal to the height of the "M" in the wordmark (or 1/4 the height of the mark). Don't crowd the logo with adjacent text or graphics.

### Minimum size

| Asset | Minimum |
|---|---|
| Logo mark only | 16 × 16 px (favicon size); 24 × 24 for general UI use |
| Logo wordmark | 80 px wide |
| Logo lockup | 100 px wide |

Below these sizes, the logo loses legibility.

### Do

- Use the logo at its original aspect ratio.
- Use the version (light-bg or dark-bg) appropriate for the background.
- Place the logo with adequate clearspace.
- Use SVG when possible.

### Don't

- Distort the logo (no stretching, squishing, rotating).
- Recolor the logo to non-brand colors (the only permitted recoloring is changing the wordmark to your text color in editorial content).
- Add effects (shadows, glows, outlines, gradients).
- Combine the logo with other marks or symbols.
- Use the logo as a watermark or background pattern.
- Use the logo on a background that lacks contrast.
- Animate the logo unless it's a brief, tasteful transition.

## Typography

Our wordmark uses **Inter** (SIL Open Font License). When pairing UI text with the wordmark, Inter is preferred but not required; system fonts (San Francisco on macOS, Segoe UI on Windows) are acceptable substitutes.

We do not have a strict typography policy beyond the wordmark. Editorial content about MaeclyPrint should use whatever your house style dictates.

## Domain names

We own and operate:

- `maecly.com` — primary website
- `updates.maecly.com` — auto-update manifest endpoint

If you want to register a domain that includes "maeclyprint" for community purposes (a fan site, a translation site, a guide repository), please:

1. Open an issue tagged `trademark` describing your intent.
2. We'll respond within 5 business days.
3. We typically permit non-commercial community use; we usually decline commercial uses to prevent confusion.

We will pursue takedown of domains used for malicious purposes (phishing, malware distribution under our name).

## Press, packaging, and merchandising

For press kits: we do not have a formal press kit yet. When we do, it will be at `maecly.com/press`. In the meantime, the brand assets in `docs/brand/` may be used for editorial press coverage under the nominative fair use described above.

For commercial merchandising (selling t-shirts, mugs with our logo): contact us. We will likely permit small-scale community merchandising at cost; large-scale commercial merchandising requires a formal arrangement.

## Reporting trademark issues

If you see misuse of the MaeclyPrint name or logo:

- **Imitation product or fork using our name:** open an issue tagged `trademark`. We'll evaluate and respond.
- **Phishing site impersonating MaeclyPrint:** email the security address in `SECURITY.md` immediately.
- **Confusing branding in the wild:** open an issue or send a friendly note to the offender first; most issues resolve through a polite request to rename/rebrand.

## Reservation of rights

We reserve the right to update this policy. Substantive changes will be announced on our communication channels and through a CHANGELOG entry.

We also reserve the right to enforce our trademarks. We aim to do so proportionately: a friendly cease-and-desist for honest mistakes, and stronger action only against bad-faith infringement.

## See also

- [license.md](./license.md) — copyright on the source code (separate from trademark)
- [third-party-licenses.md](./third-party-licenses.md) — trademarks of components we use
- [privacy.md](./privacy.md)
