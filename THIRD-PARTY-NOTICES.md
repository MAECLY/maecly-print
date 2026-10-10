# Third-party notices

Pluryprint is licensed under the GNU Affero General Public License v3.0 (see
`LICENSE`; it was GPL-3.0-only until 2026-10-09). It links third-party code
into its binaries and its webview, and ships one third-party program
(SumatraPDF, on Windows). Several of those licences require their notices to
travel with the program. This file ships inside the application bundle
(`bundle.resources` in `src-tauri/tauri.conf.json`), together with `LICENSE`
and the licence texts in `LICENSES/`, not only in the repository.

**Status: incomplete.** This file names every native component, bundled
program, font and webview package built into Pluryprint, every Rust crate that
is MIT only or Apache-2.0 only, and every Rust crate whose licence is outside
MIT and Apache-2.0. The remaining crates, licensed under a choice that includes
MIT or Apache-2.0, are named only in `src-tauri/Cargo.lock` and
`shell-ext-win/Cargo.lock`. It gives a copyright line for the native
components, the bundled program, the font, the webview packages and the Rust
crates whose licence is outside MIT and Apache-2.0. It does not give a
copyright line for the other Rust crates, and it reproduces full licence texts
only for SumatraPDF (GPL-3.0, in `LICENSES/GPL-3.0.txt`, and MuPDF's AGPL-3.0,
which is `LICENSE`), Apache-2.0 (`LICENSES/Apache-2.0.txt`), JetBrains Mono
(`LICENSES/OFL-1.1.txt`) and the Windows WebView2 loader (below). Four items
are open (see [Open items](#open-items)). Until a generated licence bundle
replaces this hand-written list, treat it as an inventory, not as a finished
compliance document.

`docs/09-legal/third-party-licenses.md` is the human-readable inventory of
direct dependencies, how each is used, and why its licence is compatible with
the AGPL-3.0. This file is the notice that ships.

## How this list was made

Checked on 2026-10-09 against `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`,
`shell-ext-win/Cargo.toml`, `shell-ext-win/Cargo.lock` and `package.json`:

- Rust: `cargo tree -e normal,no-proc-macro --prefix none --format '{p}|{l}' --locked`
  for each of the four release targets (`aarch64-apple-darwin`,
  `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`)
  on `src-tauri` (the `pluryprint` and `pluryprint-cli` binaries), and for the
  two Windows targets on `shell-ext-win` (the Explorer handler DLL), de-duplicated
  by name and version. That gives 369 third-party crates: 359 in the app, 28 in
  the DLL, 10 of them only in the DLL (it pins its own versions). Procedural
  macros and the crates reachable only through them run at build time and are
  not counted; neither are `[build-dependencies]` or `[dev-dependencies]`.
- C and Objective-C: the shipped crates whose build scripts compile native code
  (`cargo tree -i cc -e build`), and the `#[link]` attributes of the `-sys`
  crates in that set.
- Frontend: a production `vite build` with source maps, listing every package
  under `node_modules` whose code ends up in the emitted JavaScript or CSS, plus
  the files under `src/assets/` that Vite copies into the bundle. Build and test
  tooling (Vite, Vitest, WebdriverIO, ESLint, Prettier, TypeScript, jsdom, the
  Tauri CLI) leaves no code in the bundle.
- Every licence found was checked for compatibility with the AGPL-3.0. None is
  incompatible: there is no GPL-2.0-only, EPL, CDDL or proprietary code in the
  shipped set. The full list is in
  [Licence compatibility](#licence-compatibility).

---

## Program shipped beside the app (Windows only)

### SumatraPDF 3.5.2

Pluryprint's Windows build prints PDFs through `SumatraPDF.exe`, which it runs
as a separate process. It is not linked into Pluryprint. `scripts/fetch-sumatrapdf.ps1`
(or its POSIX twin, `scripts/fetch-sumatrapdf.sh`) downloads the pinned upstream
release for the target architecture (`SumatraPDF-3.5.2-64.zip` or
`SumatraPDF-3.5.2-arm64.zip` from sumatrapdfreader.org), checks its SHA-256, and
puts `SumatraPDF.exe` in `src-tauri/resources/sumatrapdf/`, which
`bundle.resources` ships. The archives carry no licence, so the scripts also
fetch `COPYING`, `COPYING.BSD` and `AUTHORS` from the pinned source commit and
put them beside the executable as `LICENSE.txt`, `COPYING.BSD.txt` and
`AUTHORS.txt`, with a `SOURCE.txt` naming the version and source commit. The macOS bundle does not contain it.

| Component                              | Licence  | Copyright                                                                                   |
| -------------------------------------- | -------- | ------------------------------------------------------------------------------------------- |
| SumatraPDF                             | GPL-3.0  | Copyright Krzysztof Kowalczyk and the SumatraPDF contributors (see `AUTHORS` in its source) |
| MuPDF (compiled into `SumatraPDF.exe`) | AGPL-3.0 | Copyright Artifex Software, Inc.                                                            |

The GPL-3.0 text is in `LICENSES/GPL-3.0.txt` and the AGPL-3.0 text is
`LICENSE`; both ship in every bundle. The corresponding
source is the `3.5.2rel` tag of https://github.com/sumatrapdfreader/sumatrapdf,
which includes MuPDF under `mupdf/`. Pluryprint does not modify SumatraPDF.

---

## Native code compiled into the binaries

### ring

`ring` 0.17.14, the cryptography under `rustls` (reached through `ureq` and
through `tauri`'s `reqwest`), compiles C and assembly derived from BoringSSL on
every target.

Apache-2.0 AND ISC. The licence is given at the top of each source file:

| Part                                                | Licence    | Copyright                                                                   |
| --------------------------------------------------- | ---------- | --------------------------------------------------------------------------- |
| ring's own code                                     | ISC        | Copyright 2015-2025 Brian Smith                                             |
| C and assembly taken from OpenSSL through BoringSSL | Apache-2.0 | Copyright 1995-2020 The OpenSSL Project Authors (year ranges vary per file) |
| C taken from BoringSSL                              | Apache-2.0 | Copyright 2014-2024 The BoringSSL Authors (year ranges vary per file)       |
| Other BoringSSL-derived files                       | ISC        | Copyright (c) 2014, Google Inc.; Copyright (c) 2014, Intel Corporation      |
| fiat-crypto field arithmetic (`third_party/fiat`)   | Apache-2.0 | Copyright 2015-2020 the fiat-crypto authors                                 |

`LICENSE-BoringSSL` and `LICENSE-other-bits` in the crate hold the full texts;
the Apache-2.0 text is also in `LICENSES/Apache-2.0.txt`.

### Objective-C helpers (macOS only)

`mac-notification-sys` 0.6.15 (MIT OR Apache-2.0, used under MIT) compiles a
small Objective-C file for native notifications, and `objc2-exception-helper`
0.1.1 (Zlib OR Apache-2.0 OR MIT, used under MIT) a small C file. Both are
covered by the Rust crate notes below.

### Microsoft WebView2 loader (Windows only)

`webview2-com-sys` 0.38.2 links Microsoft's `WebView2LoaderStatic.lib`
statically when the target is MSVC (`link(name = "WebView2LoaderStatic",
kind = "static")` under `target_env = "msvc"`), and both Windows release targets
are MSVC builds. `wry` calls it to start the webview, so the loader is part of
`pluryprint.exe`; no `WebView2Loader.dll` is shipped. The crate's changelog says
it took the library from WebView2 SDK 1.0.3650.58. The text below is the
`LICENSE.txt` of the `Microsoft.Web.WebView2` NuGet package:

> Copyright (C) Microsoft Corporation. All rights reserved.
>
> Redistribution and use in source and binary forms, with or without
> modification, are permitted provided that the following conditions are
> met:
>
> - Redistributions of source code must retain the above copyright
>   notice, this list of conditions and the following disclaimer.
> - Redistributions in binary form must reproduce the above
>   copyright notice, this list of conditions and the following disclaimer
>   in the documentation and/or other materials provided with the
>   distribution.
> - The name of Microsoft Corporation, or the names of its contributors
>   may not be used to endorse or promote products derived from this
>   software without specific prior written permission.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
> "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
> LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
> A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
> OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
> SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
> LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
> DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
> THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
> (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
> OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

The WebView2 _runtime_ (the Edge engine) is part of Windows and is not shipped.

---

## Rust crates

The 369 crates are recorded with exact versions in `src-tauri/Cargo.lock` and
`shell-ext-win/Cargo.lock`. Most are MIT, Apache-2.0, or a choice that includes
one of them. The crates below have a licence outside that set, or a licence that
applies in addition to it:

| Crate                                                                                                                                          | Version        | Licence                                               | Copyright                                                                                                                 |
| ---------------------------------------------------------------------------------------------------------------------------------------------- | -------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `option-ext`                                                                                                                                   | 0.2.0          | MPL-2.0                                               | Simon Ochsenreither (crate author; the licence file has no copyright line)                                                |
| `icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`         | 2.2.0          | Unicode-3.0                                           | Copyright © 2020-2024 Unicode, Inc.                                                                                       |
| `litemap` 0.8.2, `potential_utf` 0.1.5, `tinystr` 0.8.3, `writeable` 0.6.3, `yoke` 0.8.2, `zerofrom` 0.1.7, `zerotrie` 0.2.4, `zerovec` 0.11.6 | —              | Unicode-3.0                                           | Copyright © 2020-2024 Unicode, Inc.                                                                                       |
| `ring`                                                                                                                                         | 0.17.14        | Apache-2.0 AND ISC                                    | See [ring](#ring) above                                                                                                   |
| `rustls-webpki`                                                                                                                                | 0.103.13       | ISC                                                   | Copyright 2015 Brian Smith                                                                                                |
| `untrusted`                                                                                                                                    | 0.9.0          | ISC                                                   | Copyright 2015-2016 Brian Smith                                                                                           |
| `webpki-roots`                                                                                                                                 | 0.26.11, 1.0.8 | CDLA-Permissive-2.0 (Mozilla's root certificate list) | none given in the licence file                                                                                            |
| `brotli`                                                                                                                                       | 8.0.2          | BSD-3-Clause AND MIT                                  | Copyright (c) 2009, 2010, 2013-2016 by the Brotli Authors; Copyright (c) 2016 Dropbox, Inc.                               |
| `alloc-no-stdlib`, `alloc-stdlib`                                                                                                              | 2.0.4, 0.2.2   | BSD-3-Clause                                          | Copyright (c) 2016 Dropbox, Inc. (`alloc-stdlib` ships no licence file)                                                   |
| `subtle`                                                                                                                                       | 2.6.1          | BSD-3-Clause                                          | Copyright (c) 2016-2017 Isis Agora Lovecruft, Henry de Valence; Copyright (c) 2016-2024 Isis Agora Lovecruft              |
| `glyph-names`                                                                                                                                  | 0.2.0          | BSD-3-Clause                                          | Copyright 2002-2019 Adobe; Copyright 2020-2023 YesLogic Pty. Ltd.                                                         |
| `encoding_rs`                                                                                                                                  | 0.8.35         | (Apache-2.0 OR MIT) AND BSD-3-Clause (WHATWG data)    | Copyright Mozilla Foundation; Copyright © WHATWG (Apple, Google, Mozilla, Microsoft)                                      |
| `dpi`                                                                                                                                          | 0.1.2          | Apache-2.0 AND MIT                                    | MIT part is code from `rust-lang/libm`; `LICENSE-LIBM-MIT` carries its musl, FreeBSD and Sun Microsystems copyright lines |

The source of the MPL-2.0 crate above is available, unmodified, from crates.io
at the version listed. It carries no "Incompatible With Secondary Licenses"
notice, so MPL-2.0 section 3.3 lets it be combined with AGPL-3.0 code.

Crates whose licence is a choice that includes MIT or Apache-2.0 (273 of them,
for example `Unlicense OR MIT`, `Zlib OR Apache-2.0 OR MIT`,
`BSD-3-Clause OR Apache-2.0`, `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`)
are used under MIT or Apache-2.0 and are not repeated here. Both of those
licences still ask for a notice in a binary distribution: MIT for the copyright
and permission notice, Apache-2.0 for a copy of the licence, which is in
`LICENSES/Apache-2.0.txt`. This file does not yet carry the MIT notices for any
of these crates (see [Open items](#open-items)).

Six shipped crates are Apache-2.0 only: `allsorts` 0.16.1, `sync_wrapper`
1.0.2, `tao` 0.35.0, `unicode-canonical-combining-class` 1.0.0,
`unicode-general-category` 1.1.0 and `unicode-joining-type` 1.0.0. None of them
ships an Apache `NOTICE` file.

These 62 shipped crates (58 names) are MIT only, with no Apache-2.0
alternative, so their copyright and permission notice has to travel with the
binary. This file names them but does not yet reproduce those notices:
`aliasable` 0.1.3, `block2` 0.6.2, `bytes` 1.11.1, `cfb` 0.7.3, `ecb` 0.1.2,
`fax` 0.2.7, `generic-array` 0.14.7, `http-body` 1.0.1, `http-body-util` 0.1.3,
`hyper` 1.9.0, `hyper-util` 0.1.20, `infer` 0.19.0, `lopdf` 0.34.0 and 0.39.0,
`matchers` 0.2.0, `minisign-verify` 0.2.5, `mio` 1.2.0, `nom` 7.1.3 and 8.0.0,
`nom_locate` 5.0.0, `nu-ansi-term` 0.50.3, `objc2` 0.6.4, `objc2-encode` 4.1.0,
`objc2-foundation` 0.3.2, `open` 5.3.6, `phf` 0.11.3, `phf_shared` 0.11.3,
`plist` 1.9.0, `printpdf` 0.9.1, `quick-xml` 0.39.2, `rfd` 0.16.0,
`sharded-slab` 0.1.7, `simd-adler32` 0.3.9, `slab` 0.4.12, `specta`
2.0.0-rc.22, `specta-serde` 0.0.9, `specta-typescript` 0.0.9, `strsim` 0.11.1,
`tauri-specta` 2.0.0-rc.21, `tiff` 0.11.3, `tokio` 1.52.1, `tokio-util` 0.7.18,
`tower` 0.5.3, `tower-http` 0.6.8, `tower-layer` 0.3.3, `tower-service` 0.3.3,
`tracing` 0.1.44, `tracing-appender` 0.2.5, `tracing-core` 0.1.36,
`tracing-log` 0.2.0, `tracing-serde` 0.2.0, `tracing-subscriber` 0.3.23,
`try-lock` 0.2.5, `urlpattern` 0.3.0, `want` 0.3.1, `webview2-com` 0.38.2,
`webview2-com-sys` 0.38.2, `winnow` 0.5.40 and 1.0.2, `zip` 4.6.1, `zmij`
1.0.21 and 1.0.23.

The `block2` and `objc2*` crates ship only on macOS; `webview2-com`,
`webview2-com-sys` and `zip` (through `tauri-plugin-updater`) only on Windows.

Some crates appear in `Cargo.lock` but are reached only through a procedural
macro or a build script and do not ship: among them `cssparser`,
`cssparser-macros`, `selectors` and `dtoa-short` (MPL-2.0), `foldhash` (Zlib),
`Inflector` (BSD-2-Clause) and `libloading` (ISC), and `embed-resource`, which
compiles the shell extension's version resource.

---

## Frontend (webview)

The bundled JavaScript and CSS include:

| Component                                                                         | Version | Licence                            | Copyright                                                 |
| --------------------------------------------------------------------------------- | ------- | ---------------------------------- | --------------------------------------------------------- |
| `react`, `react-dom`                                                              | 18.3.1  | MIT                                | Copyright (c) Facebook, Inc. and its affiliates           |
| `scheduler` (pulled in by `react-dom`)                                            | 0.23.2  | MIT                                | Copyright (c) Facebook, Inc. and its affiliates           |
| `zustand`                                                                         | 5.0.14  | MIT                                | Copyright (c) 2019 Paul Henschel                          |
| `sonner`                                                                          | 1.7.4   | MIT                                | Copyright (c) 2023 Emil Kowalski                          |
| `@tauri-apps/api`                                                                 | 2.11.0  | Apache-2.0 OR MIT (used under MIT) | Copyright (c) 2017 - Present Tauri Apps Contributors      |
| `@tauri-apps/plugin-dialog`                                                       | 2.7.1   | MIT OR Apache-2.0 (used under MIT) | 2019-2022, The Tauri Programme in the Commons Conservancy |
| Tailwind CSS (`tailwindcss`, a `devDependency`): the generated utility stylesheet | 3.4.19  | MIT                                | Copyright (c) Tailwind Labs, Inc.                         |

### JetBrains Mono

Three weights of JetBrains Mono (`JetBrainsMono-Regular.woff2`, `-Medium` and
`-SemiBold`, in `src/assets/fonts/`) are copied into the bundle and used as the
monospace face.

SIL Open Font License 1.1. Copyright 2020 The JetBrains Mono Project Authors
(https://github.com/JetBrains/JetBrainsMono). The copyright line and the full
licence text are in `LICENSES/OFL-1.1.txt`. The fonts are not modified.

---

## System components the app uses but does not ship

- **macOS:** WebKit (`WKWebView`) and the other system frameworks, and the CUPS
  `lp` command, which prints.
- **Windows:** the WebView2 runtime. The WebView2 _loader_ is linked in; see
  [Microsoft WebView2 loader](#microsoft-webview2-loader-windows-only).
- **Programs the user installs, if they want those formats:** LibreOffice,
  Pandoc and a Chromium-class browser, run as separate processes to convert
  files to PDF. None of them is shipped.

---

## Licence compatibility

Every licence in the shipped set lets its code be combined with, or shipped
beside, code under the AGPL-3.0:

| Licence                                                                     | Where                                         | Why it is compatible                                                                                                        |
| --------------------------------------------------------------------------- | --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| MIT, MIT-0, ISC, BSD-2-Clause, BSD-3-Clause, Zlib, 0BSD, Unlicense, CC0-1.0 | most crates, the frontend packages            | Permissive; ask only for a notice                                                                                           |
| Apache-2.0 (alone, or with LLVM-exception)                                  | six crates, `ring`, choices used under Apache | Compatible with GPLv3 and so with AGPLv3 (its patent terms are what GPLv2 could not accept)                                 |
| Unicode-3.0, CDLA-Permissive-2.0                                            | the ICU4X crates, `webpki-roots` data         | Permissive data licences; ask only for a notice                                                                             |
| MPL-2.0                                                                     | `option-ext`                                  | Section 3.3 allows combination with the GNU licences; no Exhibit B notice                                                   |
| OFL-1.1                                                                     | JetBrains Mono                                | A font licence; the fonts are shipped unmodified with their licence                                                         |
| BSD-style Microsoft licence                                                 | the WebView2 loader                           | Permissive; asks for the notice reproduced above                                                                            |
| GPL-3.0                                                                     | SumatraPDF                                    | A separate program, shipped beside Pluryprint; AGPL-3.0 section 13 and GPL-3.0 section 13 also allow the two to be combined |
| AGPL-3.0                                                                    | MuPDF, inside SumatraPDF                      | The same licence as Pluryprint                                                                                              |

No shipped component is GPL-2.0-only, LGPL-only, EPL, CDDL, SSPL or
proprietary.

---

## Open items

1. **Notices for the Rust crates.** MIT, Apache-2.0, BSD, ISC, Zlib and
   Unicode-3.0 all ask for a copyright line, a licence text, or both to travel
   with a binary. This file gives copyright lines only for the components above
   and the crates in the Rust table, and none for the other Rust crates (the 62
   MIT-only crates listed above among them).
2. **SumatraPDF's checksum pin is empty.** `EXPECTED_SHA256` in both fetch
   scripts is blank, and the scripts refuse to run unless a checksum is passed
   explicitly. Filling it in, and naming the SumatraPDF version in the release
   notes so "corresponding source" is unambiguous, is release-blocking.
3. **The Linux bundles.** `tauri.conf.json` also builds `.deb`, `.rpm` and
   `.AppImage`, which are not release targets and were not inventoried here.
   The AppImage copies shared libraries (GTK, WebKitGTK and their dependencies,
   mostly LGPL) from the build machine.
4. **The Windows installer.** The NSIS installer stub (zlib/libpng licence)
   is part of the produced `-setup.exe`; its notice is not reproduced here.
