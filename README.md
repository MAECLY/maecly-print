# MaeclyPrint

> **Open source batch file printing for Windows and macOS.** Right-click → print everything, with no 15-file limit, no Adobe required, and no $149 license fee.

[![Status](https://img.shields.io/badge/status-pre--alpha-orange)](./docs/06-roadmap/milestones.md)
[![License](https://img.shields.io/badge/license-GPLv3-blue)](./LICENSE)
[![Stack](https://img.shields.io/badge/stack-Tauri%202%20%2B%20Rust%20%2B%20React-success)](./docs/04-architecture/tech-stack.md)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20%E2%86%92%20macOS-lightgrey)](./docs/06-roadmap/milestones.md)

---

## The problem

Windows hides the **Print** verb from the right-click menu when you select more than ~15 files. Adobe Acrobat doesn't help: it has the same effective limit and is heavyweight. The commercial alternatives that fix this — Print Conductor ($149), 2Printer ($149), Batch & Print Pro ($79.95) — are closed-source and pricey. The few open-source options are abandoned (last release in 2015), depend on Adobe Reader, or are virtual printers, not real batch printers.

So most people end up printing in tedious batches of 15-20 files, manually, again and again.

## What MaeclyPrint does

MaeclyPrint adds a native entry to the **modern Windows 11 right-click menu**:

```
Right-click on any number of files → MaeclyPrint ▸
                                       ├─ Print with default profile
                                       ├─ Print with profile…
                                       └─ Configure print job…
```

- Select **5, 50, or 500** files. They print.
- Mix PDFs, images, Word docs, Excel sheets, HTML, Markdown — MaeclyPrint routes each to the right engine.
- Use **profiles** (built-in or custom) to lock in your favorite settings: duplex BW, photo color, A3 simplex, whatever.
- All **local**: no cloud, no telemetry, no account.
- Auto-update built in: just click "Check for updates" — no GitHub Releases hunting.

## Status

**Pre-alpha.** This repository currently contains the project's documentation and design. Implementation has not begun.

The locked decisions (stack, license, scope) are stable. See the [roadmap](./docs/06-roadmap/milestones.md) for the milestones from `M0 — Foundation` through `M5 — macOS Port`.

## Documentation

All documentation lives under [`docs/`](./docs/README.md). Recommended starting points:

- [Product Vision](./docs/01-vision/product-vision.md)
- [Value Proposition](./docs/01-vision/value-proposition.md)
- [System Architecture](./docs/04-architecture/system-architecture.md)
- [MVP Features](./docs/05-features/mvp-features.md)
- [Roadmap & Milestones](./docs/06-roadmap/milestones.md)
- [Architecture Decision Records](./docs/adr/README.md)

## Stack

| Layer | Technology |
|---|---|
| Application framework | [Tauri 2](https://tauri.app) |
| Backend | Rust |
| Frontend | React + TypeScript + Vite + Tailwind CSS |
| Print engine (Win) | [SumatraPDF](https://www.sumatrapdfreader.org/) (CLI) |
| Print engine (macOS, future) | CUPS (`lpr`) |
| Office conversion | [LibreOffice](https://www.libreoffice.org/) (headless) |
| HTML conversion | Chromium (headless) |
| Markdown conversion | [Pandoc](https://pandoc.org/) |
| Image preprocessing | [ImageMagick](https://imagemagick.org/) |
| Shell extension (Win) | C++ COM DLL implementing `IExplorerCommand` + Sparse MSIX |
| Installer (Win) | [Inno Setup](https://jrsoftware.org/isinfo.php) |
| Auto-updater | Tauri Updater plugin |

Justification in [tech-stack.md](./docs/04-architecture/tech-stack.md) and the [ADRs](./docs/adr/README.md).

## License

[GPLv3](./LICENSE). MaeclyPrint is and will remain free software. See [docs/09-legal/license.md](./docs/09-legal/license.md) for what that means in practice.

## Contributing

Contributions are welcome — see [CONTRIBUTING](./docs/08-development/contributing.md). Before opening a PR for non-trivial changes, please file an issue or start a discussion.

If you want to contribute a translation, see the [internationalization section](./docs/08-development/contributing.md#translations).

## Project name and trademark

"MaeclyPrint" is a trademark of the project. Forks must rebrand. See [trademark policy](./docs/09-legal/trademark.md).

## Privacy

No telemetry by default. No cloud. Your files never leave your machine. See [privacy](./docs/09-legal/privacy.md).

## Acknowledgements

MaeclyPrint stands on the shoulders of these excellent open-source projects:

- [Tauri](https://tauri.app), [Rust](https://www.rust-lang.org/), [React](https://react.dev/)
- [SumatraPDF](https://www.sumatrapdfreader.org/) for fast, reliable PDF printing on Windows
- [LibreOffice](https://www.libreoffice.org/) for Office and ODF conversion
- [Chromium](https://www.chromium.org/), [Pandoc](https://pandoc.org/), [ImageMagick](https://imagemagick.org/), [qpdf](https://github.com/qpdf/qpdf)

Full list and licenses in [third-party-licenses.md](./docs/09-legal/third-party-licenses.md).
