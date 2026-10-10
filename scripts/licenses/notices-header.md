# Third-party notices

Pluryprint is licensed under the GNU Affero General Public License v3.0 (see
`LICENSE`; it was GPL-3.0-only until 2026-10-09). It links third-party code
into its binaries and its webview, and its Windows installers carry one
third-party program (SumatraPDF). Their licences require the notices below to
travel with the program, so this file ships inside every application bundle
(`bundle.resources` in `src-tauri/tauri.conf.json`), together with `LICENSE`
and the licence texts in `LICENSES/`, not only in the repository.

`docs/09-legal/third-party-licenses.md` is the human-readable inventory of
direct dependencies, how each is used, and why its licence is compatible with
the AGPL-3.0. This file is the notice that ships.

## How this file is made

The first part, up to [Rust crates](#rust-crates), is written by hand
(`scripts/licenses/notices-header.md`): the bundled program, the native code,
the installer, the font and the licence compatibility table. Everything from
[Rust crates](#rust-crates) on is **generated** by
`scripts/licenses/generate-notices.sh` and must not be edited by hand:

- **Rust:** `cargo about` (configuration in `scripts/licenses/about.toml`) on
  `src-tauri` (the `pluryprint` and `pluryprint-cli` binaries) and on
  `shell-ext-win` (the Explorer handler DLL), for the four release targets
  (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`,
  `aarch64-pc-windows-msvc`), from `Cargo.lock` and the crates' own published
  sources, offline. For every crate it reproduces the licence text the crate
  ships, which carries its copyright lines; crates offered under a choice
  ("MIT OR Apache-2.0") are listed under the licence Pluryprint uses. Build
  scripts and test-only crates are left out; a procedural macro that is a
  normal dependency is listed even though its code runs only at build time,
  so the list errs on the side of more notices, not fewer.
- **Frontend:** the production dependencies of `package.json` (from
  `pnpm licenses list --prod`), plus Tailwind CSS, whose generated stylesheet
  is in the bundle although the package is a development dependency. Type-only
  packages (`@types/*`) contain no shipped code and are left out. Each entry
  reproduces the package's own licence file.

The release workflow (`release.yml`, job `guard`) regenerates the file and
refuses to build when the committed copy differs, so a dependency change
cannot reach a release without its notice.

---

## Program shipped beside the app (Windows only)

### SumatraPDF 3.6.1

Pluryprint's Windows build prints PDFs through `SumatraPDF.exe`, which it runs
as a separate process. It is not linked into Pluryprint and is not modified.
`scripts/fetch-sumatrapdf.ps1` (or its POSIX twin,
`scripts/fetch-sumatrapdf.sh`) downloads the pinned upstream release for the
installer's architecture (`SumatraPDF-3.6.1-64.zip` or
`SumatraPDF-3.6.1-arm64.zip` from sumatrapdfreader.org), checks its SHA-256,
and puts `SumatraPDF.exe` in `src-tauri/resources/sumatrapdf/`, which
`bundle.resources` ships. The archives carry no licence, so the scripts also
fetch `COPYING`, `COPYING.BSD` and `AUTHORS` from the pinned source commit and
put them beside the executable as `LICENSE.txt`, `COPYING.BSD.txt` and
`AUTHORS.txt`, with a `SOURCE.txt` naming the version and source commit and a
`SOURCE-OFFER.txt` carrying the written offer of GPL-3.0 section 6. The macOS
bundle does not contain it.

| Component                              | Licence  | Copyright                                                                                   |
| -------------------------------------- | -------- | ------------------------------------------------------------------------------------------- |
| SumatraPDF                             | GPL-3.0  | Copyright Krzysztof Kowalczyk and the SumatraPDF contributors (see `AUTHORS` in its source) |
| MuPDF (compiled into `SumatraPDF.exe`) | AGPL-3.0 | Copyright Artifex Software, Inc.                                                            |

The GPL-3.0 text is in `LICENSES/GPL-3.0.txt` and the AGPL-3.0 text is
`LICENSE`; both ship in every bundle. The corresponding source is commit
`16c59fde8b824ab54c56f23aef910a6fdd874ad0` (tag `3.6.1rel`) of
https://github.com/sumatrapdfreader/sumatrapdf, which includes MuPDF under
`mupdf/`. Every Pluryprint release also attaches that tree as
`SumatraPDF-3.6.1-source.tar.gz`, next to the installers.

---

## Native code compiled into the binaries

### ring

`ring`, the cryptography under `rustls` (reached through `ureq` and through
`tauri`'s `reqwest`), compiles C and assembly derived from BoringSSL on every
target. Apache-2.0 AND ISC; the licence is given at the top of each source
file:

| Part                                                | Licence    | Copyright                                                                   |
| --------------------------------------------------- | ---------- | --------------------------------------------------------------------------- |
| ring's own code                                     | ISC        | Copyright 2015-2025 Brian Smith                                             |
| C and assembly taken from OpenSSL through BoringSSL | Apache-2.0 | Copyright 1995-2020 The OpenSSL Project Authors (year ranges vary per file) |
| C taken from BoringSSL                              | Apache-2.0 | Copyright 2014-2024 The BoringSSL Authors (year ranges vary per file)       |
| Other BoringSSL-derived files                       | ISC        | Copyright (c) 2014, Google Inc.; Copyright (c) 2014, Intel Corporation      |
| fiat-crypto field arithmetic (`third_party/fiat`)   | Apache-2.0 | Copyright 2015-2020 the fiat-crypto authors                                 |

The texts are reproduced under [Rust crates](#rust-crates); the Apache-2.0
text is also in `LICENSES/Apache-2.0.txt`.

### Objective-C helpers (macOS only)

`mac-notification-sys` compiles a small Objective-C file for native
notifications, and `objc2-exception-helper` a small C file. Both are covered
by their crate entries under [Rust crates](#rust-crates).

### Microsoft WebView2 loader (Windows only)

`webview2-com-sys` links Microsoft's `WebView2LoaderStatic.lib` statically
when the target is MSVC, and both Windows release targets are MSVC builds.
`wry` calls it to start the webview, so the loader is part of
`pluryprint.exe`; no `WebView2Loader.dll` is shipped. The text below is the
`LICENSE.txt` of the `Microsoft.Web.WebView2` NuGet package:

```text
Copyright (C) Microsoft Corporation. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * The name of Microsoft Corporation, or the names of its contributors
may not be used to endorse or promote products derived from this
software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

The WebView2 _runtime_ (the Edge engine) is part of Windows and is not shipped;
the installer downloads Microsoft's bootstrapper only when it is missing.

---

## The Windows installer

The `.exe` setup programs are built with NSIS (Nullsoft Scriptable Install
System) by the Tauri bundler, which also compiles in its `nsis_tauri_utils`
plug-in (MIT OR Apache-2.0, Copyright the Tauri Programme within The
Commons Conservancy). The installer stub and its LZMA decompressor are part
of the setup program, not of Pluryprint, which the setup program unpacks and
installs. NSIS's licence notice:

```text
{{NSIS_COPYING}}
```

---

## Font

Three weights of JetBrains Mono (`JetBrainsMono-Regular.woff2`, `-Medium` and
`-SemiBold`, in `src/assets/fonts/`) are copied into the bundle and used as the
monospace face. SIL Open Font License 1.1. Copyright 2020 The JetBrains Mono
Project Authors (https://github.com/JetBrains/JetBrainsMono). The copyright
line and the full licence text are in `LICENSES/OFL-1.1.txt`. The fonts are
not modified.

---

## System components the app uses but does not ship

- **macOS:** WebKit (`WKWebView`) and the other system frameworks, the CUPS
  `lp` command, which prints, and `textutil`.
- **Windows:** the WebView2 runtime. The WebView2 _loader_ is linked in; see
  [Microsoft WebView2 loader](#microsoft-webview2-loader-windows-only).
- **Programs the user installs, if they want those formats:** LibreOffice and
  the operating system's image codecs, run or loaded as separate components.
  None of them is shipped.

---

## Licence compatibility

Every licence in the shipped set lets its code be combined with, or shipped
beside, code under the AGPL-3.0:

| Licence                                                                                       | Where                                          | Why it is compatible                                                                                                        |
| --------------------------------------------------------------------------------------------- | ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| MIT, MIT-0, ISC, BSD-2-Clause, BSD-3-Clause, Zlib, 0BSD, Unlicense, CC0-1.0, BSL-1.0, OpenSSL | most crates, the frontend packages             | Permissive; ask only for a notice                                                                                           |
| Apache-2.0 (alone, or with LLVM-exception)                                                    | many crates, `ring`, choices used under Apache | Compatible with GPLv3 and so with AGPLv3 (its patent terms are what GPLv2 could not accept)                                 |
| Unicode-3.0, Unicode-DFS-2016, CDLA-Permissive-2.0                                            | the ICU4X crates, `webpki-roots` data          | Permissive data licences; ask only for a notice                                                                             |
| MPL-2.0                                                                                       | `option-ext`, `cssparser`, `selectors`         | Section 3.3 allows combination with the GNU licences; none carries an Exhibit B notice                                      |
| OFL-1.1                                                                                       | JetBrains Mono                                 | A font licence; the fonts are shipped unmodified with their licence                                                         |
| BSD-style Microsoft licence                                                                   | the WebView2 loader                            | Permissive; asks for the notice reproduced above                                                                            |
| GPL-3.0                                                                                       | SumatraPDF                                     | A separate program, shipped beside Pluryprint; AGPL-3.0 section 13 and GPL-3.0 section 13 also allow the two to be combined |
| AGPL-3.0                                                                                      | MuPDF, inside SumatraPDF                       | The same licence as Pluryprint                                                                                              |
| zlib/libpng, bzip2, CPL-1.0                                                                   | the NSIS installer stub                        | The setup program that carries Pluryprint, not part of it: an aggregate, which the AGPL-3.0 (section 5) permits             |

The MPL-2.0 crates are used unmodified; their source is available from
crates.io at the versions listed below. No shipped component is
GPL-2.0-only, LGPL-only, EPL, CDDL, SSPL or proprietary.

---
