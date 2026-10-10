# TRADEMARKS

**Pluryprint**, the two-colour wordmark (`Plury` + `print` in the accent colour),
and the **Abanico** mark belong to MAECLY, the name under which the founder, Miguel Angel Esparza
Calero, publishes his software.

The Abanico mark is a printer releasing three sheets in a fan (left, up, right),
which is what the app does: many files out at once. It has two forms, and both
are the mark:

- **The detailed form.** Motion strokes and text lines on the sheets. The app
  icon uses it from 48px up, and the app draws it wherever the mark is 40px or
  larger, as on the About screen.
- **The simplified form.** Heavier sheets, no strokes or lines. The 16-32px icons
  use it, and so does the sidebar wordmark.

The icon draws the mark in the brand oranges (`#C2410C` and `#F97316`) and
ink (`#1C1917`). Inside the app the mark takes the current theme's accent.

- The code is licensed under the GNU Affero General Public License v3.0 (see
  `LICENSE`). As an additional term under its section 7(e), the licence grants
  no rights under trademark law to these names and marks beyond reasonable and
  customary use in describing where the work came from — for example, "a fork of
  Pluryprint".
- Forks and derivatives **must** change the product name, wordmark and mark before
  distributing. The name "Pluryprint" may not be used to market a fork as if it
  were the original product. `docs/09-legal/trademark.md` has the full policy.
- Until October 2026 the product was called **MaeclyPrint**. That name belongs to
  the founder as well. The app still reads its old folders, settings and
  environment variables so an existing install upgrades in place (ADR-0009); a
  fork should drop that code rather than inherit an install base it never had.

## What a fork has to replace

The name, identifiers and mark live in these places in this repository:

- `src-tauri/icons/` - the desktop icon set. `scripts/icons/generate.mjs` writes
  all of it (`pnpm icons`); change the generator, not the PNGs.
- `src/components/Brand.tsx` - the mark drawn inside the app (`FanMark`,
  `MPGlyph`, `MPAppIcon`), from the same geometry as the icon, and the
  `MPWordmark`.
- `src-tauri/src/platform/testpage.rs` - the printer test page, which draws the
  mark, the wordmark and the product name into a PDF.
- `index.html` - the page `<title>`.
- `package.json` - the npm package name `pluryprint`.
- `src-tauri/tauri.conf.json` - `productName` and the window `title`
  ("Pluryprint"), the bundle `identifier` (`com.maecly.pluryprint`), `publisher`,
  `copyright`, the NSIS `installerHooks`, and the updater `pubkey` and
  `endpoints`. A fork that keeps Pluryprint's public key cannot publish updates
  its own builds will accept.
- `src-tauri/src/util/paths.rs` - `ORG` and `APP`, the `ProjectDirs` triple
  (`com`, `Maecly`, `Pluryprint`) that names every config, data, cache and log
  folder. `shell-ext-win/src/strings.rs`, `shell-ext-win/src/handoff.rs` and
  `shell-ext-win/src/logging.rs` repeat it and must change with it. A fork that
  keeps it shares Pluryprint's settings, profiles and queue on the same machine.
- `src-tauri/src/util/legacy.rs` - the MaeclyPrint folders, log prefix and
  environment variables the app migrates from (see above).
- `src-tauri/src/settings/policy.rs` - the admin policy locations
  (`HKLM\SOFTWARE\Policies\Maecly\Pluryprint`, `ProgramData\Pluryprint`,
  `/Library/Application Support/Pluryprint`, `/etc/pluryprint`).
- `src-tauri/src/logging/mod.rs` - the `pluryprint-` log file prefix.
- `src-tauri/Cargo.toml` - the package `pluryprint` and lib `pluryprint_lib`,
  which name the `pluryprint` binary; `src-tauri/src/bin/pluryprint-cli.rs` names
  the console binary, and `src-tauri/src/cli/args.rs` the CLI's program name.
- `src-tauri/Info.plist`, `src-tauri/macos/*.lproj/ServicesMenu.strings` and
  `src-tauri/src/platform/macos/services.rs` - the Finder service ("Print with
  Pluryprint"), its `NSPortName` (the bundle name) and the ObjC provider class.
- `shell-ext-win/` - the package name `pluryprint-shellext`, the version
  resource (`res/shellext.rc`), the sparse package identity
  (`res/shellext.manifest`), the Explorer verb text (`src/strings.rs`), and the
  COM CLSID (`src/clsid.rs`). A fork must generate its own CLSID: two products
  registering the same one take over each other's context menu.
- `src-tauri/windows/installer-hooks.nsh` (`NSIS_HOOK_PREINSTALL`) - removes a MaeclyPrint install.
- `src/i18n/locales/en/*.json` and `src/i18n/locales/es/*.json` - the product
  name in the interface copy, and `about.json` `byline` ("by Maecly").
- `src/lib/repo.ts`, `src-tauri/src/update/install.rs` (`DEFAULT_ENDPOINT`) and
  `src-tauri/src/update/mod.rs` - the GitHub repository the app links to and
  checks for updates. Leave them and the fork goes looking for Pluryprint's
  releases.
- `src/lib/theme.ts`, `src/App.tsx` and `src/i18n/I18nProvider.tsx` - the
  `pluryprint:` prefix of the keys kept in the webview's `localStorage`.

The repository is `github.com/MAECLY/pluryprint`. Until 2026-10-10 it was
called `maecly-print`; GitHub redirects the old URL.

## Other names

The app and its docs mention products it works with, prints through or runs on.
Those names belong to their owners, and their use here only describes
compatibility: Tauri, React, SumatraPDF, CUPS, Adobe, Acrobat, Microsoft,
Windows, Explorer, Apple, macOS, Finder, GitHub, Epson, Brother, HP, MuPDF, and
similar marks. Licence notices for the third-party code compiled into the app
are in `THIRD-PARTY-NOTICES.md`.

Copyright © 2026 maecly.
