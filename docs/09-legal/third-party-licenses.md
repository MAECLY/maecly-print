# Third-Party Licenses

> **TL;DR.** MaeclyPrint bundles SumatraPDF (GPLv3, same license as ours) and depends on Tauri/Rust/React (all permissive licenses, GPL-compatible). External tools we invoke as subprocesses (LibreOffice, Pandoc, Chromium) are not embedded; per FSF guidance, subprocess invocation does not create a derivative work, so their licenses do not constrain our license.

This document inventories every third-party component MaeclyPrint depends on, with its license, our usage pattern (embedded vs subprocess), and the GPLv3-compatibility analysis. This is the source of truth for legal compliance.

If you find a discrepancy or a missing component, file an issue immediately.

## Master inventory

| Component | Version (typical) | License | Used for | Embedded? | GPLv3 compatible? |
|---|---|---|---|---|---|
| Tauri | 2.x | MIT / Apache 2.0 | App framework | Yes (linked) | Yes |
| Rust standard library | edition 2024 | MIT / Apache 2.0 | Language runtime | Yes (linked) | Yes |
| React | 18.x | MIT | Frontend UI | Yes (bundled) | Yes |
| TypeScript runtime helpers | 5.x | Apache 2.0 | Frontend lang | Yes (bundled) | Yes |
| Vite | 5.x | MIT | Frontend build tool | No (build-time only) | N/A |
| Tailwind CSS | 3.x | MIT | Styling | Yes (bundled CSS) | Yes |
| SumatraPDF | latest stable | GPLv3 | PDF printing | Yes (bundled `.exe`) | Yes (same license) |
| LibreOffice | external | MPL 2.0 | Office conversion | No (subprocess) | Yes (separate) |
| Chromium / Edge | external | BSD 3-Clause | HTML→PDF rendering | No (subprocess) | Yes (separate) |
| Pandoc | external | GPLv2-or-later | Markdown→PDF | No (subprocess) | Yes (separate) |
| ImageMagick | external | Apache 2.0 | Image preprocessing | No (subprocess) | Yes (separate) |
| qpdf | external (optional) | Apache 2.0 | PDF preprocessing | No (subprocess) | Yes (separate) |
| windows-rs | latest | MIT / Apache 2.0 | Win32 API bindings | Yes (linked) | Yes |
| tokio | 1.x | MIT | Async runtime | Yes (linked) | Yes |
| serde | 1.x | MIT / Apache 2.0 | Serialization | Yes (linked) | Yes |
| tracing | 0.1 | MIT | Logging | Yes (linked) | Yes |
| clap | 4.x | MIT / Apache 2.0 | CLI parsing | Yes (linked) | Yes |
| anyhow | 1.x | MIT / Apache 2.0 | Error handling (app) | Yes (linked) | Yes |
| thiserror | 1.x | MIT / Apache 2.0 | Error handling (lib) | Yes (linked) | Yes |
| reqwest | 0.12.x | MIT / Apache 2.0 | HTTPS (updater) | Yes (linked) | Yes |
| tauri-plugin-updater | 2.x | MIT / Apache 2.0 | Auto-update | Yes (linked) | Yes |
| zustand | 4.x | MIT | Frontend state | Yes (bundled) | Yes |
| react-i18next | 14.x | MIT | Frontend i18n | Yes (bundled) | Yes |
| WebView2 (loader stub) | latest | MIT (loader); Edge engine system-installed | Webview rendering | Loader linked; engine system | Yes (loader); engine N/A |
| WebKit / WKWebView (macOS) | system | Apple SLA | Webview rendering | No (system framework) | N/A (system) |
| Inno Setup | 6.x | "Free even for commercial use" | Installer build (Windows) | No (build-time only) | N/A |
| create-dmg | external | MIT | DMG build (macOS) | No (build-time only) | N/A |

For each entry below: the license text source, how we use it, and any compliance notes.

## Embedded components (linked into our binary)

These are statically or dynamically linked into `maeclyprint.exe`/`maeclyprint` and form a combined work with our code.

### Tauri (MIT / Apache 2.0 dual-license)

- **Used for:** the application framework (main process, IPC, plugins, updater).
- **License text:** [https://github.com/tauri-apps/tauri/blob/dev/LICENSE_MIT](https://github.com/tauri-apps/tauri/blob/dev/LICENSE_MIT) and [https://github.com/tauri-apps/tauri/blob/dev/LICENSE_APACHE-2.0](https://github.com/tauri-apps/tauri/blob/dev/LICENSE_APACHE-2.0).
- **Compatibility:** MIT and Apache 2.0 are both permissive and explicitly GPLv3-compatible. We can include them in a GPLv3 work freely.
- **Compliance:** Tauri's notices are included in our `Resources/third-party/LICENSES.txt` distributed with each release.

### Rust standard library + crates ecosystem

- **Used for:** language runtime + dozens of utility crates (`tokio`, `serde`, `clap`, `anyhow`, `thiserror`, `tracing`, etc.).
- **License:** Mostly MIT/Apache 2.0 dual.
- **Compatibility:** all GPLv3-compatible.
- **Audit:** we run `cargo about generate` in CI to produce a license manifest of every transitive dep. It's checked into `src-tauri/about.toml` and updated per release.

### React + TypeScript ecosystem

- **Used for:** the frontend. React (MIT), Zustand (MIT), react-i18next (MIT), Tailwind CSS (MIT).
- **License:** all MIT.
- **Compatibility:** GPLv3-compatible.
- **Audit:** `pnpm licenses list` produces an inventory; we check this into `LICENSES-frontend.txt` per release.

### `windows-rs`

- **Used for:** Win32 API bindings (printer enumeration, GDI+, COM).
- **License:** MIT / Apache 2.0 dual.
- **Compatibility:** GPLv3-compatible.

### WebView2 (Windows webview)

- **Loader stub:** distributed with our app (`WebView2Loader.dll`). Licensed MIT.
- **Edge engine:** system-installed (preinstalled on Win 11; bootstrap on Win 10). Microsoft EULA governs it. We do not redistribute the engine itself.
- **Compatibility:** the loader stub being MIT is fine. The engine being a system component (we link to it dynamically at runtime) is analogous to linking against the OS itself; this is the [system library exception](https://www.gnu.org/licenses/gpl-faq.en.html#WindowsRuntimeAndGPL) under GPLv3.

### WKWebView (macOS webview)

- **Provided by macOS** as a system framework. We link against it dynamically.
- **Compatibility:** system library; same exception applies.

## Embedded data (bundled binaries we ship)

These are not linked but shipped as separate files in our installer.

### SumatraPDF (GPLv3)

- **Used for:** PDF rendering and silent printing on Windows. Invoked as `SumatraPDF.exe -print-to <printer> file.pdf`.
- **License:** [GPLv3](https://www.gnu.org/licenses/gpl-3.0.html). License text bundled at `installers/windows/SumatraPDF/LICENSE.txt` and copied into the install directory.
- **Source code:** [https://github.com/sumatrapdfreader/sumatrapdf](https://github.com/sumatrapdfreader/sumatrapdf). Our installer's About page links to this repository.
- **Compatibility:** SumatraPDF is GPLv3, MaeclyPrint is GPLv3. Identical license; the combined distribution is unambiguously GPLv3.
- **Compliance:** because we redistribute SumatraPDF as a binary, we must offer its source code per GPLv3 section 6. We do this by linking to the upstream repository in our About page, third-party notices file, and installer's "more information" link.

### Future bundled components

If we ever bundle Pandoc (GPLv2-or-later) or another GPL'd binary, the same compliance pattern applies: bundle license text, link to source.

## External components (subprocess invocation)

These are tools we call out to via `Command::spawn` / `subprocess.run` / `lpr`. We never bundle them; they must be installed by the user separately. **Per the [FSF's guidance](https://www.gnu.org/licenses/gpl-faq.html#GPLPlugins) and [Mere Aggregation FAQ](https://www.gnu.org/licenses/gpl-faq.en.html#MereAggregation), subprocess invocation does not create a combined work, so the subprocess's license does not infect ours.**

Quoting the FSF directly:

> "If two programs run at the same time and communicate using exec, fork, or pipes, then they are normally separate programs."

This is the legal foundation for using LibreOffice (MPL 2.0) and Chromium (BSD 3-Clause) in a GPLv3 project without those licenses contaminating ours.

### LibreOffice (MPL 2.0)

- **Used for:** Office document → PDF conversion. Invoked as `soffice --headless --convert-to pdf <file>`.
- **License:** [Mozilla Public License 2.0](https://www.mozilla.org/MPL/2.0/).
- **Embedded?** No. Subprocess only.
- **Compatibility:** MPL 2.0 is a weak copyleft. Even if we did link against LibreOffice (we don't), MPL 2.0 has explicit GPL compatibility through the secondary license clause. As subprocess invocation, this is moot.
- **Compliance:** none required from us. We document the dependency; users install separately.

### Chromium / Microsoft Edge (BSD 3-Clause)

- **Used for:** HTML → PDF rendering. Invoked as `msedge --headless --print-to-pdf=<output> <input>`.
- **License:** [BSD 3-Clause](https://opensource.org/licenses/BSD-3-Clause) (Chromium core; Edge adds Microsoft proprietary code we don't redistribute).
- **Embedded?** No. Subprocess only. We use the system Edge installation.
- **Compatibility:** BSD 3-Clause is permissive; subprocess invocation makes this irrelevant anyway.

### Pandoc (GPLv2-or-later)

- **Used for:** Markdown → PDF.
- **License:** [GPLv2-or-later](https://www.gnu.org/licenses/gpl-2.0.html). Pandoc itself is GPL.
- **Embedded?** No. Subprocess only.
- **Compatibility:** GPLv2-or-later is GPLv3-compatible (the "or later" clause permits combination). Subprocess invocation makes this moot.

### ImageMagick (Apache 2.0)

- **Used for:** Image preprocessing (resize, format normalize). Invoked as `magick convert <input> <output>`.
- **License:** [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0) (recent ImageMagick v7+ is Apache; older v6 is "ImageMagick License" which is similar but not identical — we require v7+).
- **Embedded?** No. Subprocess only.
- **Compatibility:** Apache 2.0 is GPLv3-compatible. Subprocess invocation makes this moot.

### qpdf (Apache 2.0)

- **Used for:** Optional PDF preprocessing (linearize, decrypt empty-password PDFs). Invoked as `qpdf <input> <output>`.
- **License:** Apache 2.0.
- **Embedded?** No. Subprocess only.
- **Compatibility:** Apache 2.0 is GPLv3-compatible. Subprocess invocation makes this moot.

## Build-time only (not distributed)

These tools are used to build MaeclyPrint but are not part of the distributed binary.

### Vite (MIT)

- **Used for:** frontend bundling.
- **Distributed?** No. Vite produces output JS/CSS/HTML; only the output is distributed.
- **Compliance:** none required.

### Inno Setup (Free for commercial use)

- **Used for:** building the Windows installer `.exe`.
- **License:** [Modified BSD-style](https://jrsoftware.org/files/is/license.txt) ("free even for commercial use").
- **Distributed?** No. Inno Setup produces the installer; only the installer is distributed.
- **Compliance:** none required from license terms.

### create-dmg (MIT)

- **Used for:** building macOS `.dmg`.
- **Distributed?** No. Output `.dmg` is distributed; create-dmg itself isn't.
- **Compliance:** none required.

### MakeAppx, signtool, codesign, notarytool, pkgbuild, productbuild

- **Used for:** packaging and signing.
- **Distributed?** No. These are platform-vendor SDKs (Microsoft, Apple).
- **Compliance:** governed by Microsoft / Apple SDK terms, which permit normal developer use.

## Compliance artifacts we ship

For each release, we include in the install directory:

```
<install-dir>/
├── LICENSE.txt                # GPLv3 (full text)
├── third-party/
│   └── LICENSES.txt           # All third-party licenses concatenated
└── sumatrapdf/
    ├── SumatraPDF.exe
    └── LICENSE.txt            # SumatraPDF's GPLv3 notice
```

The `LICENSES.txt` file is generated at build time by:

```bash
# Rust deps
cargo about generate templates/about.hbs > LICENSES-rust.txt

# Frontend deps
pnpm licenses list --long --recursive > LICENSES-frontend.txt

# Concatenate with a header
cat templates/header.txt LICENSES-rust.txt LICENSES-frontend.txt > Resources/third-party/LICENSES.txt
```

This file is reviewed during release prep (see [release-checklist.md](../08-development/release-checklist.md)) to catch new dependencies that don't have a license declared.

## Compatibility check process

When adding a new dependency:

1. Identify its license (`cargo metadata` for Rust deps; `npm view <pkg> license` for frontend).
2. Cross-reference against the [FSF compatibility list](https://www.gnu.org/licenses/license-list.html) or [SPDX license list](https://spdx.org/licenses/).
3. If GPLv3-compatible: proceed.
4. If GPLv3-incompatible (e.g. CDDL, EPL, MPL 1.x without dual-license): **do not add**. Either find a replacement, refactor to subprocess invocation, or skip the feature.

In CI, we use `cargo deny` to enforce a license allowlist:

```toml
# deny.toml
[licenses]
allow = [
  "MIT",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Unicode-DFS-2016",
  "Zlib",
  "GPL-3.0",
  "MPL-2.0",
]
deny = [
  "CDDL-1.0",
  "EPL-1.0",
  "EPL-2.0",
  "GPL-2.0",  # GPL-2.0-only is incompatible with GPLv3
  "AGPL-3.0",  # We're GPL, not AGPL
]
```

A PR that introduces a denied license will fail CI.

## "But is X legally a derivative work?"

The case for subprocess invocation **not** being a derivative work has been argued and accepted in many contexts. Beyond the FSF FAQ:

- [Software Freedom Law Center on subprocess invocation](https://www.softwarefreedom.org/resources/2009/the-fsf-policy-on-derivatives.html): "Programs running as separate processes are not combined into a single derivative work."
- [Linux Kernel position](https://www.kernel.org/doc/Documentation/process/license-rules.rst): the syscall boundary is treated as a license boundary.
- [LibreOffice's own FAQ](https://www.libreoffice.org/about-us/licenses/) accepts subprocess invocation as a non-combining use pattern.

We are conservative: when there is doubt, we use subprocess invocation rather than embedding. The clearest example is LibreOffice (MPL 2.0) — even though MPL 2.0 has GPL-compat clauses, the subprocess pattern removes any ambiguity about whether a combined work exists.

## Trademark notices for components we mention

- "Tauri" is a trademark of the Tauri Programme.
- "React" is a trademark of Meta Platforms, Inc.
- "TypeScript" is a trademark of Microsoft Corporation.
- "Rust" and the Rust logo are trademarks of the Rust Foundation.
- "LibreOffice" is a trademark of The Document Foundation.
- "Microsoft Edge", "Microsoft Print to PDF", "WebView2" are trademarks of Microsoft Corporation.
- "Apple", "macOS", "Apple Silicon", "Xcode", "Finder" are trademarks of Apple Inc.
- "Pandoc" is a trademark of John MacFarlane.
- "ImageMagick" is a trademark of ImageMagick Studio LLC.
- "SumatraPDF" is the work of Krzysztof Kowalczyk and contributors.

We use these names nominatively (to refer to the components themselves), not in a way that implies endorsement.

## Updating this document

This document is updated:

- Whenever a new dependency is added or a major version is bumped.
- Quarterly via a maintainer audit of `cargo about` and `pnpm licenses list` output against this table.
- Before each MAJOR or MINOR release, as part of the release checklist.

If you spot something missing or stale, file an issue tagged `legal`.

## See also

- [license.md](./license.md)
- [trademark.md](./trademark.md)
- [privacy.md](./privacy.md)
