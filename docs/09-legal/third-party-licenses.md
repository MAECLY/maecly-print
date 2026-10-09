# Third-Party Licenses

> **TL;DR.** MaeclyPrint bundles SumatraPDF (GPLv3, same license as ours — fetched at packaging time by `scripts/fetch-sumatrapdf.*`, never committed) and depends on Tauri/Rust/React (all permissive licenses, GPL-compatible). External tools we invoke as subprocesses (LibreOffice, Pandoc, Chromium) are not embedded; per FSF guidance, subprocess invocation does not create a derivative work, so their licenses do not constrain our license.

This document inventories every third-party component MaeclyPrint depends on, with its license, our usage pattern (embedded vs subprocess), and the GPLv3-compatibility analysis. This is the source of truth for legal compliance.

If you find a discrepancy or a missing component, file an issue immediately.

## Master inventory

The tables below are derived from the **actual manifests**: `src-tauri/Cargo.toml`, `shell-ext-win/Cargo.toml` and `package.json`. Versions are the declared requirements, not resolved lockfile versions; the authoritative per-release inventory is the generated `LICENSES.txt` (see "Compliance artifacts we ship"). Licenses marked *(typical)* are the customary license of that crate and must be confirmed against the generated manifest before a release.

### Rust — `src-tauri` (direct dependencies, all linked into the binary)

| Crate | Declared version | License (typical) | Used for | GPLv3 compatible? |
|---|---|---|---|---|
| `tauri` | 2.1 | MIT / Apache-2.0 | App framework | Yes |
| `tauri-build` | 2.0 | MIT / Apache-2.0 | Build script (not distributed) | N/A |
| `tauri-plugin-dialog` | 2 | MIT / Apache-2.0 | Native file picker | Yes |
| `tauri-plugin-opener` | 2 | MIT / Apache-2.0 | Open URLs / reveal folders | Yes |
| `tauri-plugin-single-instance` | 2 | MIT / Apache-2.0 | Forward a second launch (NFR-014) | Yes |
| `serde`, `serde_json` | 1 | MIT / Apache-2.0 | Serialization | Yes |
| `toml` | 0.8 | MIT / Apache-2.0 | `config.toml`, `profiles.toml` | Yes |
| `tokio` | 1.41 | MIT | Async runtime (queue dispatcher) | Yes |
| `tracing`, `tracing-subscriber`, `tracing-appender` | 0.1 / 0.3 / 0.2 | MIT | Structured logging | Yes |
| `thiserror` | **2** | MIT / Apache-2.0 | Library error types | Yes |
| `anyhow` | 1 | MIT / Apache-2.0 | Application error wrapping | Yes |
| `directories` | 5 | MIT / Apache-2.0 | Per-OS config/data/cache/log paths | Yes |
| `tempfile` | 3 | MIT / Apache-2.0 | Atomic writes, private conversion cache | Yes |
| `walkdir` | 2 | MIT / Unlicense | Folder expansion (FR-004) | Yes |
| `clap` | 4 (`derive`) | MIT / Apache-2.0 | Headless CLI surface | Yes |
| `wait-timeout` | 0.2 | MIT / Apache-2.0 | Bounded wait on the print child process | Yes |
| `semver` | 1 | MIT / Apache-2.0 | Update-check version comparison | Yes |
| `ureq` | 2.12 (`json`) | MIT / Apache-2.0 | Blocking HTTPS for the GitHub release check | Yes |
| `percent-encoding` | 2.3 | MIT / Apache-2.0 | Prefilled "report issue" URL | Yes |
| `specta` | `=2.0.0-rc.22` | MIT / Apache-2.0 | Type reflection for the IPC bindings | Yes |
| `specta-typescript` | `=0.0.9` | MIT / Apache-2.0 | TypeScript emitter | Yes |
| `tauri-specta` | `=2.0.0-rc.21` | MIT / Apache-2.0 | Typed command/event bindings | Yes |
| `printpdf` | 0.9.1 (`default-features = false`) | MIT | PDF primitives | Yes |
| `image` | 0.25 (jpeg/png/tiff/bmp/webp only) | MIT / Apache-2.0 | **In-process** image decoding | Yes |
| `lopdf` | 0.34 | MIT | PDF parsing + hand-built image PDFs | Yes |
| `uuid` | 1 (`v7`) | MIT / Apache-2.0 | Time-sortable job ids | Yes |
| `time` | 0.3 | MIT / Apache-2.0 | RFC 3339 timestamps | Yes |
| `windows` *(Windows targets only)* | 0.58 | MIT / Apache-2.0 | winspool + GDI `DeviceCapabilities` | Yes |
| `objc2` *(macOS only)* | 0.6 | MIT | Objective-C runtime for the NSServices provider | Yes |
| `objc2-app-kit`, `objc2-foundation` *(macOS only)* | 0.3 | MIT | AppKit/Foundation bindings (feature-narrowed) | Yes |

### Rust — `shell-ext-win` (the Windows Explorer handler, a separate `cdylib`)

| Crate | Declared version | License (typical) | Used for |
|---|---|---|---|
| `serde`, `serde_json` | 1 | MIT / Apache-2.0 | The handoff payload |
| `uuid` | 1 (`v7`) | MIT / Apache-2.0 | Handoff file names |
| `time` | 0.3 | MIT / Apache-2.0 | `invoked_at` stamps |
| `directories` | 5 | MIT / Apache-2.0 | Resolving the handoff directory identically to the app |
| `windows`, `windows-core` *(Windows only)* | 0.58 | MIT / Apache-2.0 | COM / `IExplorerCommand` |
| `embed-resource` *(build-dependency)* | 3 | MIT | Embeds `res/shellext.rc`, including the side-by-side `<msix>` manifest |
| `tempfile` *(dev-dependency)* | 3 | MIT / Apache-2.0 | Tests only; not distributed |

This crate is deliberately **not** a workspace member (see [`../04-architecture/security-model.md`](../04-architecture/security-model.md) §3.1), so it carries its own lockfile and its own license surface.

### Frontend — `package.json` runtime dependencies (bundled into the app)

| Package | Declared version | License | Used for |
|---|---|---|---|
| `react`, `react-dom` | ^18.3.1 | MIT | UI runtime |
| `@tauri-apps/api` | ^2.1.1 | MIT / Apache-2.0 | IPC + event bridge |
| `@tauri-apps/plugin-dialog` | ^2.7.1 | MIT / Apache-2.0 | File picker binding |
| `zustand` | ^5.0.14 | MIT | Frontend state |
| `sonner` | ^1.7.1 | MIT | Toasts |
| JetBrains Mono (font, bundled `.woff2`) | — | SIL OFL 1.1 | Monospace typeface shipped offline |

Localization is **in-house** (`src/i18n/`), not `react-i18next`; there is no i18n runtime dependency.

### Frontend — build/dev only (not distributed)

`vite`, `typescript`, `tailwindcss`, `postcss`, `autoprefixer`, `eslint` (+ plugins), `prettier`, `vitest`, `@vitest/coverage-v8`, `jsdom`, `@testing-library/*`, `@wdio/*` (WebDriver e2e), `husky`, `lint-staged`, `@commitlint/*`, `@tauri-apps/cli` — all MIT or ISC.

Two notes: **Tailwind CSS is MIT and its generated CSS *is* distributed** in the bundle (it is listed here because the tool is build-time; the output is ours). **WebDriverIO (`@wdio/*`) is the e2e driver**, not Playwright.

### Non-crate components

| Component | License | Used for | Embedded? | GPLv3 compatible? |
|---|---|---|---|---|
| SumatraPDF **3.5.2** (pinned) | **GPLv3** | PDF printing on Windows | Yes — fetched at packaging time and bundled | Yes (same license); see the compliance note below |
| LibreOffice | MPL 2.0 | Office, OpenDocument, TXT, RTF → PDF | No (subprocess) | Yes (separate) |
| Chromium / Microsoft Edge | BSD 3-Clause (Chromium core) | HTML and Markdown → PDF (Google Chrome or Chromium also work) | No (subprocess) | Yes (separate) |
| Pandoc | GPLv2-or-later | Markdown → HTML | No (subprocess) | Yes (separate) |
| sips / Windows Imaging Component | OS components | HEIC/HEIF decoding (`sips` on macOS; WIC plus the Store *HEIF Image Extensions* on Windows) | No (system tool / OS API) | Yes (separate) |
| CUPS `lp` | Apache 2.0 (system component) | Printing on macOS/Linux | No (system binary) | Yes (separate) |
| WebView2 loader | MIT (loader); Edge engine system-installed | Webview rendering | Loader linked; engine system | Yes (loader); engine N/A |
| WKWebView (macOS) | Apple SLA | Webview rendering | No (system framework) | N/A (system) |
| NSIS (via `tauri build`) | zlib/libpng | Windows installer | No (build-time only) | N/A |

**Removed from this inventory because they are not used:** `reqwest` (replaced by `ureq`), `tauri-plugin-updater` (there is no in-app updater — see [`../04-architecture/auto-updater.md`](../04-architecture/auto-updater.md)), `tauri-plugin-cli` (the CLI is plain `clap`), `winreg`, `which`, `mime_guess`, `infer`, `rayon`, `notify`, `minisign-verify`, `react-i18next`, `@radix-ui/*`, `react-hook-form`, `@tanstack/react-query`, `lucide-react`, `clsx`, `tailwind-merge`, `playwright`, Inno Setup, create-dmg. **ImageMagick and qpdf are not invoked at all**: images are converted in-process with the `image` crate, and there is no PDF preprocessing step.

For each entry below: the license text source, how we use it, and any compliance notes.

## Embedded components (linked into our binary)

These are statically or dynamically linked into `maeclyprint.exe`/`maeclyprint` and form a combined work with our code.

### Tauri (MIT / Apache 2.0 dual-license)

- **Used for:** the application framework (main process, IPC, plugins, updater).
- **License text:** [https://github.com/tauri-apps/tauri/blob/dev/LICENSE_MIT](https://github.com/tauri-apps/tauri/blob/dev/LICENSE_MIT) and [https://github.com/tauri-apps/tauri/blob/dev/LICENSE_APACHE-2.0](https://github.com/tauri-apps/tauri/blob/dev/LICENSE_APACHE-2.0).
- **Compatibility:** MIT and Apache 2.0 are both permissive and explicitly GPLv3-compatible. We can include them in a GPLv3 work freely.
- **Compliance:** Tauri's notices are included in our `Resources/third-party/LICENSES.txt` distributed with each release.

### Rust standard library + crates ecosystem

- **Used for:** language runtime + the utility crates listed in the inventory above.
- **License:** Mostly MIT/Apache 2.0 dual.
- **Compatibility:** all GPLv3-compatible.
- **Audit — not yet automated.** `cargo about generate` is described below as the intended mechanism, but there is **no `src-tauri/about.toml` and no `deny.toml` in the repository today**, and neither `cargo about` nor `cargo deny` runs in `.github/workflows/ci.yml`. Until those land, the inventory above is maintained by hand against the manifests, which is exactly the failure mode that produced the previous version of this document.

### React + TypeScript ecosystem

- **Used for:** the frontend. React / React DOM (MIT), Zustand (MIT), Sonner (MIT), the `@tauri-apps/*` JS packages (MIT / Apache-2.0), and Tailwind CSS (MIT) whose *generated* CSS ships in the bundle.
- **Localization is in-house** (`src/i18n/`): a small provider plus JSON catalogues per screen. No i18n library is bundled.
- **Compatibility:** GPLv3-compatible.
- **Audit:** `pnpm licenses list` produces an inventory; wiring it into the release is still open (see above).

### `windows` (windows-rs)

- **Used for:** Win32 API bindings — `EnumPrinters` / `DeviceCapabilities` / `GetDefaultPrinter` in the app, and COM / `IExplorerCommand` in the shell extension.
- **License:** MIT / Apache 2.0 dual.
- **Compatibility:** GPLv3-compatible.
- **Note:** the two crates pull the same 0.58 line but different feature sets; they are separate builds because the shell extension is a separate crate.

### `objc2`, `objc2-app-kit`, `objc2-foundation` (macOS only)

- **Used for:** the "Print with MaeclyPrint" NSServices provider declared in `Info.plist` (FR-001).
- **License:** MIT.
- **Compatibility:** GPLv3-compatible.
- **Note:** versions are constrained to the ones tao/wry already resolve, so only one copy of the objc2 tree is linked.

### WebView2 (Windows webview)

- **Loader stub:** distributed with our app (`WebView2Loader.dll`). Licensed MIT.
- **Edge engine:** system-installed (preinstalled on Win 11; bootstrap on Win 10). Microsoft EULA governs it. We do not redistribute the engine itself.
- **Compatibility:** the loader stub being MIT is fine. The engine being a system component (we link to it dynamically at runtime) is analogous to linking against the OS itself; this is the [system library exception](https://www.gnu.org/licenses/gpl-faq.en.html#WindowsRuntimeAndGPL) under GPLv3.

### WKWebView (macOS webview)

- **Provided by macOS** as a system framework. We link against it dynamically.
- **Compatibility:** system library; same exception applies.

## Embedded data (bundled binaries we ship)

These are not linked but shipped as separate files in our installer.

### SumatraPDF 3.5.2 (GPLv3) — the one GPL binary we redistribute

- **Used for:** silent PDF printing on Windows (`-print-to`, `-print-settings`, `-silent`, `-exit-when-done`). Not used on macOS/Linux, which print through CUPS `lp`.
- **License:** [GPLv3](https://www.gnu.org/licenses/gpl-3.0.html).
- **How it gets into the bundle:** `scripts/fetch-sumatrapdf.ps1` (Windows) / `scripts/fetch-sumatrapdf.sh` (POSIX twin, for cross-building CI and for verifying the pin off-Windows). They download the **pinned** upstream release — version `3.5.2`, from `https://www.sumatrapdfreader.org/dl/rel/3.5.2/SumatraPDF-3.5.2-64.zip` — verify its SHA-256, extract `SumatraPDF.exe` into `src-tauri/resources/sumatrapdf/`, and **copy the archive's `LICENSE` next to it as `LICENSE.txt`**. `tauri.conf.json` then ships that directory via `bundle.resources: ["resources/sumatrapdf/*"]`. The glob deliberately matches nothing on a macOS/Linux bundle.
- **Why not committed:** ~20 MB of platform-specific artifact git would keep forever, paid for by every contributor including those who can never run it; and it is not ours to re-host — fetching the vendor's own release at packaging time keeps provenance (and these GPLv3 obligations) where they belong. A pinned URL plus a pinned checksum is a stronger guarantee than a committed blob, and it is reviewable in a diff.
- **⚠️ The checksum pin is currently empty.** `EXPECTED_SHA256` in both scripts is intentionally blank because no checksum has been verified against upstream from this repository yet; the scripts **fail closed** and demand one be passed explicitly (`SUMATRAPDF_SHA256=<hex>` / `-Sha256 <hex>`). Filling it in is a release-blocking prerequisite, not a nicety — see the hardening checklist in [`../04-architecture/security-model.md`](../04-architecture/security-model.md) §10.
- **Source code:** [https://github.com/sumatrapdfreader/sumatrapdf](https://github.com/sumatrapdfreader/sumatrapdf).
- **Compatibility:** SumatraPDF is GPLv3, MaeclyPrint is GPLv3. Identical license; the combined distribution is unambiguously GPLv3.
- **Compliance (GPLv3 §6):** because we redistribute a GPLv3 binary we must offer its corresponding source. We do so by shipping the upstream `LICENSE.txt` beside the executable and by linking to the upstream repository from the About screen and from this document. **Pin the exact upstream version in the release notes** so "corresponding source" is unambiguous.

### Future bundled components

If we ever bundle Pandoc (GPLv2-or-later) or another GPL'd binary, the same compliance pattern applies: pin the version and checksum, bundle the license text, link to the source, and name the version in the release notes.

## External components (subprocess invocation)

These are tools we shell out to with `std::process::Command`. We never bundle them; they must be installed by the user separately (the app probes for them and reports what is missing — `engine::deps`, FR-058). CUPS `lp` (macOS/Linux), LibreOffice, a Chromium-class browser, Pandoc and macOS `sips` are all invoked as separate processes, each with an argument vector and never through a shell. **Per the [FSF's guidance](https://www.gnu.org/licenses/gpl-faq.html#GPLPlugins) and [Mere Aggregation FAQ](https://www.gnu.org/licenses/gpl-faq.en.html#MereAggregation), subprocess invocation does not create a combined work, so the subprocess's license does not infect ours.**

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

### CUPS `lp` (Apache 2.0, system component)

- **Used for:** spooling the final PDF on macOS and Linux (`lp -d … -n … -o …`), and cancelling by the returned job id.
- **Embedded?** No. It is part of the operating system.
- **Compatibility:** a system utility invoked as a subprocess; nothing to comply with.

### Image conversion — no third party involved

Image → PDF conversion happens **inside** MaeclyPrint using the `image` and `lopdf` crates (both permissive, both in the inventory above). **ImageMagick is not used**, and `magick` is never invoked. Likewise there is **no qpdf step**: nothing linearizes or decrypts PDFs. Earlier versions of this document listed both; they were never dependencies.

## Build-time only (not distributed)

These tools are used to build MaeclyPrint but are not part of the distributed binary.

### Vite (MIT)

- **Used for:** frontend bundling.
- **Distributed?** No. Vite produces output JS/CSS/HTML; only the output is distributed.
- **Compliance:** none required.

### NSIS (via `tauri build`)

- **Used for:** building the Windows installer. `tauri.conf.json` sets `bundle.windows.nsis.installMode = "currentUser"`. **Inno Setup is not used**; earlier drafts said otherwise.
- **License:** zlib/libpng.
- **Distributed?** The NSIS stub is part of the produced installer; NSIS itself is not.

### Tailwind CSS / PostCSS / Autoprefixer (MIT)

- **Used for:** layout utilities only. Design tokens are CSS custom properties in `src/styles/tokens.css`; Tailwind's `preflight` is disabled.
- **Distributed?** The *generated* CSS is bundled; the toolchain is not.

### WebdriverIO (`@wdio/*`, MIT)

- **Used for:** the end-to-end lane (`pnpm test:e2e`, `e2e/wdio.conf.ts`). **Playwright is not used.**
- **Distributed?** No.

### MakeAppx, signtool, codesign, notarytool, pkgbuild, productbuild

- **Used for:** packaging and signing.
- **Distributed?** No. These are platform-vendor SDKs (Microsoft, Apple).
- **Compliance:** governed by Microsoft / Apple SDK terms, which permit normal developer use.

## Compliance artifacts we ship

**Shipped today.** The Windows bundle carries, via `bundle.resources`:

```
<resource-dir>/
└── sumatrapdf/
    ├── SumatraPDF.exe
    └── LICENSE.txt            # SumatraPDF's GPLv3 notice, copied by the fetch script
```

**Not yet shipped (open work).** The aggregated notices file below is the intended target, not the current state: `LICENSE.txt` (our GPLv3 text) and `third-party/LICENSES.txt` are **not** added to the bundle by `tauri.conf.json` today, and neither generator runs in CI. Wiring both is a prerequisite for a public release, since the GPLv3 §4/§6 notice obligations do not depend on our tooling being convenient.

```
<install-dir>/
├── LICENSE.txt                # GPLv3 (full text)          [TODO: not bundled]
├── third-party/
│   └── LICENSES.txt           # All third-party licenses    [TODO: not generated]
└── sumatrapdf/
    ├── SumatraPDF.exe
    └── LICENSE.txt            # shipped
```

The `LICENSES.txt` file is intended to be generated at build time by:

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

**`cargo deny` is not wired up yet.** There is no `deny.toml` in the repository and `.github/workflows/ci.yml` does not run it, so today step 4 above is enforced by review only. The configuration below is the intended one and should be committed as `deny.toml` when the check lands:

```toml
# deny.toml — PROPOSED, not present in the repo
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

Once committed, a PR that introduces a denied license will fail CI. Note that `GPL-3.0` must stay on the allow list: the workspace's own crates are `GPL-3.0-only`.

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
- "SumatraPDF" is the work of Krzysztof Kowalczyk and contributors.
- "JetBrains Mono" is a typeface by JetBrains s.r.o., used under the SIL Open Font License 1.1.

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
