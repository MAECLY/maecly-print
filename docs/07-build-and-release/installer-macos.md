# Installer (macOS)

> **TL;DR.** macOS distribution is a Universal binary `.app` bundle, packaged into both a `.pkg` (via `pkgbuild` + `productbuild`) and a `.dmg` (via `create-dmg`). Both artifacts are codesigned with an Apple Developer ID Application certificate and notarized through `xcrun notarytool`. Targets macOS 13+ on Apple Silicon and Intel.

This document covers the M5 macOS port. Code signing and notarization details that overlap with Windows are in [code-signing.md](./code-signing.md); this file focuses on the bundle structure, the installer scripts, and the macOS-specific quirks.

## Targets

| OS | Architecture | Tested in CI? | Status |
|---|---|---|---|
| macOS 13 Ventura | x86_64 (Intel) | Yes (`macos-13` runner) | Primary support |
| macOS 13 Ventura | aarch64 (Apple Silicon) | Yes (`macos-14` runner with deployment target 13.0) | Primary support |
| macOS 14 Sonoma | x86_64 + aarch64 | Yes | Primary support |
| macOS 15 Sequoia | x86_64 + aarch64 | Yes | Primary support |
| macOS 12 Monterey or older | any | No | Not supported |

We require macOS 13 because:

- It's the oldest version currently receiving Apple security updates.
- Finder Sync Extensions and `notarytool` workflows are stable from 13 onward.
- Tauri 2's macOS minimum is 10.13, but our backend uses APIs (e.g. `os_log` modern variants) that benefit from 13+.

## App bundle structure

The `MaeclyPrint.app` bundle, post-build, looks like:

```
MaeclyPrint.app/
└── Contents/
    ├── Info.plist
    ├── PkgInfo
    ├── MacOS/
    │   └── maeclyprint                # Universal binary (lipo-ed)
    ├── Resources/
    │   ├── icon.icns
    │   ├── en.lproj/
    │   │   └── InfoPlist.strings
    │   ├── es.lproj/
    │   │   └── InfoPlist.strings
    │   └── third-party/
    │       └── LICENSES.txt
    ├── Frameworks/
    │   └── (any embedded frameworks; usually empty for Tauri)
    ├── PlugIns/
    │   └── MaeclyPrintFinderExt.appex/   # Finder Sync Extension
    │       └── Contents/
    │           ├── Info.plist
    │           ├── MacOS/
    │           │   └── MaeclyPrintFinderExt
    │           └── Resources/
    └── _CodeSignature/
        └── CodeResources
```

Highlights:

- `MacOS/maeclyprint` is a Universal Mach-O binary. Built with `lipo -create` from `target/aarch64-apple-darwin/release/maeclyprint` and `target/x86_64-apple-darwin/release/maeclyprint`.
- `PlugIns/MaeclyPrintFinderExt.appex` is the Finder Sync Extension, which is itself a small `.appex` bundle with its own `Info.plist` and Mach-O.
- `Frameworks/` is empty in the M5 baseline. We avoid bundling third-party `.framework` directories; SumatraPDF is Windows-only, and the Mac path uses `lpr` (CUPS) which is system-provided.

## `Info.plist` essentials

Key entries in `Contents/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>MaeclyPrint</string>
  <key>CFBundleExecutable</key>
  <string>maeclyprint</string>
  <key>CFBundleIdentifier</key>
  <string>com.maecly.maeclyprint</string>
  <key>CFBundleName</key>
  <string>MaeclyPrint</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.1.0</string>
  <key>CFBundleVersion</key>
  <string>1.1.0.42</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.utilities</string>
  <key>NSHumanReadableCopyright</key>
  <string>Copyright (c) 2026 MaeclyPrint Project. Licensed under GPLv3.</string>
  <key>NSAppTransportSecurity</key>
  <dict>
    <key>NSAllowsArbitraryLoads</key>
    <false/>
    <key>NSExceptionDomains</key>
    <dict>
      <key>updates.maecly.com</key>
      <dict>
        <key>NSIncludesSubdomains</key>
        <true/>
        <key>NSRequiresCertificateTransparency</key>
        <true/>
      </dict>
    </dict>
  </dict>
  <key>NSPrintingUsageDescription</key>
  <string>MaeclyPrint sends documents to printers you choose.</string>
</dict>
</plist>
```

Notes:

- `CFBundleIdentifier` must match the Developer ID provisioning profile.
- `LSMinimumSystemVersion` enforces the macOS 13 floor; older versions get a clear "won't open" error from Launch Services.
- `NSAppTransportSecurity` whitelists only `updates.maecly.com`; everything else is denied.

## Universal binary with `lipo`

Build steps for the Universal Mach-O:

```bash
#!/usr/bin/env bash
set -euo pipefail

# Build both slices
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

# Combine
lipo -create \
  -output target/universal-apple-darwin/release/maeclyprint \
  target/aarch64-apple-darwin/release/maeclyprint \
  target/x86_64-apple-darwin/release/maeclyprint

# Verify
lipo -info target/universal-apple-darwin/release/maeclyprint
# Expected: Architectures in the fat file: maeclyprint are: x86_64 arm64
```

The same flow runs for the Finder Sync Extension's executable.

## Building the `.pkg` installer

`installers/macos/pkgbuild.sh`:

```bash
#!/usr/bin/env bash
# Builds MaeclyPrint.pkg from a fully-built MaeclyPrint.app
set -euo pipefail

VERSION="${1:?Usage: pkgbuild.sh <version>}"
APP_PATH="build/MaeclyPrint.app"
PKG_PATH="dist/MaeclyPrint-${VERSION}.pkg"
COMPONENT_PKG="dist/MaeclyPrint-component-${VERSION}.pkg"

mkdir -p dist

# 1. Build the component package (the app payload)
pkgbuild \
  --component "${APP_PATH}" \
  --install-location "/Applications" \
  --identifier "com.maecly.maeclyprint" \
  --version "${VERSION}" \
  --sign "${APPLE_DEVELOPER_ID_INSTALLER}" \
  "${COMPONENT_PKG}"

# 2. Build the distribution package (wraps the component, adds welcome/license/conclusion)
productbuild \
  --distribution "installers/macos/distribution.xml" \
  --package-path "dist" \
  --resources "installers/macos/resources" \
  --sign "${APPLE_DEVELOPER_ID_INSTALLER}" \
  "${PKG_PATH}"

# 3. Cleanup component package; only distribution package ships
rm "${COMPONENT_PKG}"

echo "Built: ${PKG_PATH}"
```

`installers/macos/distribution.xml`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
  <title>MaeclyPrint</title>
  <organization>com.maecly</organization>
  <welcome file="welcome.html" mime-type="text/html"/>
  <license file="LICENSE.html" mime-type="text/html"/>
  <conclusion file="conclusion.html" mime-type="text/html"/>
  <options customize="never" require-scripts="false" hostArchitectures="x86_64,arm64"/>
  <volume-check>
    <allowed-os-versions>
      <os-version min="13.0"/>
    </allowed-os-versions>
  </volume-check>
  <choices-outline>
    <line choice="default">
      <line choice="com.maecly.maeclyprint"/>
    </line>
  </choices-outline>
  <choice id="default"/>
  <choice id="com.maecly.maeclyprint" visible="false">
    <pkg-ref id="com.maecly.maeclyprint"/>
  </choice>
  <pkg-ref id="com.maecly.maeclyprint" version="1.1.0" auth="root">MaeclyPrint-component-1.1.0.pkg</pkg-ref>
</installer-gui-script>
```

The `auth="root"` is required because the install destination (`/Applications`) needs admin write. Users see the standard macOS authentication dialog.

## Building the `.dmg`

`installers/macos/create-dmg.sh`:

```bash
#!/usr/bin/env bash
# Builds MaeclyPrint.dmg using create-dmg (Homebrew: brew install create-dmg)
set -euo pipefail

VERSION="${1:?Usage: create-dmg.sh <version>}"
APP_PATH="build/MaeclyPrint.app"
DMG_PATH="dist/MaeclyPrint-${VERSION}.dmg"

mkdir -p dist

create-dmg \
  --volname "MaeclyPrint ${VERSION}" \
  --volicon "installers/macos/resources/volume.icns" \
  --background "installers/macos/resources/dmg-bg.png" \
  --window-pos 200 120 \
  --window-size 600 400 \
  --icon-size 100 \
  --icon "MaeclyPrint.app" 175 200 \
  --hide-extension "MaeclyPrint.app" \
  --app-drop-link 425 200 \
  --no-internet-enable \
  --codesign "${APPLE_DEVELOPER_ID_APPLICATION}" \
  "${DMG_PATH}" \
  "${APP_PATH}"

echo "Built: ${DMG_PATH}"
```

The `--codesign` flag invokes `codesign` on the produced `.dmg` itself (not just the contained `.app`). DMGs are signed because Gatekeeper checks the container, not just the contents, when downloaded.

## Codesigning the `.app`

Before `pkgbuild` or `create-dmg`, the `.app` and its embedded `.appex` must be signed. Order matters: sign inside-out.

```bash
#!/usr/bin/env bash
set -euo pipefail

APP="build/MaeclyPrint.app"
SIGN_ID="${APPLE_DEVELOPER_ID_APPLICATION}"
ENTITLEMENTS="installers/macos/resources/entitlements.plist"

# 1. Sign the Finder Sync Extension first (deepest first)
codesign --force --options runtime --timestamp \
  --entitlements "installers/macos/resources/finder-ext-entitlements.plist" \
  --sign "${SIGN_ID}" \
  "${APP}/Contents/PlugIns/MaeclyPrintFinderExt.appex"

# 2. Sign any frameworks (none in M5; placeholder for future)
# for fw in "${APP}/Contents/Frameworks/"*.framework; do
#   codesign --force --options runtime --timestamp --sign "${SIGN_ID}" "$fw"
# done

# 3. Sign the main app
codesign --force --options runtime --timestamp \
  --entitlements "${ENTITLEMENTS}" \
  --sign "${SIGN_ID}" \
  "${APP}"

# 4. Verify
codesign --verify --deep --strict --verbose=2 "${APP}"
spctl --assess --type execute --verbose "${APP}"
```

`entitlements.plist` (main app):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.app-sandbox</key>
  <false/>
  <key>com.apple.security.cs.allow-jit</key>
  <false/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
  <false/>
  <key>com.apple.security.cs.disable-library-validation</key>
  <false/>
  <key>com.apple.security.network.client</key>
  <true/>
  <key>com.apple.security.print</key>
  <true/>
  <key>com.apple.security.files.user-selected.read-only</key>
  <true/>
</dict>
</plist>
```

Minimal entitlements: network for the updater, print for printer access, user-selected read-only files (the user picks files to print in Finder; we read them).

We **do not** enable App Sandbox in M5. Sandboxing complicates Finder Sync Extensions and CUPS interactions. We may revisit for a Mac App Store edition later, but the Developer ID distribution does not require sandbox.

## Notarization

Notarization is mandatory for macOS 10.15+ (and we target 13+). After signing:

```bash
#!/usr/bin/env bash
# installers/macos/notarize.sh
set -euo pipefail

ARTIFACT="${1:?Usage: notarize.sh <path-to-pkg-or-dmg>}"
APPLE_ID="${APPLE_ID:?APPLE_ID env var required}"
APPLE_TEAM_ID="${APPLE_TEAM_ID:?APPLE_TEAM_ID env var required}"
APPLE_APP_PASSWORD="${APPLE_APP_PASSWORD:?APPLE_APP_PASSWORD env var required}"

# 1. Submit and wait for notarization
xcrun notarytool submit "${ARTIFACT}" \
  --apple-id "${APPLE_ID}" \
  --team-id "${APPLE_TEAM_ID}" \
  --password "${APPLE_APP_PASSWORD}" \
  --wait

# 2. Staple the notarization ticket to the artifact
xcrun stapler staple "${ARTIFACT}"

# 3. Verify
xcrun stapler validate "${ARTIFACT}"
spctl --assess --type install --verbose "${ARTIFACT}"
```

Notes:

- `notarytool` typically returns within 5–15 minutes; in busy periods, up to a few hours.
- `--wait` blocks until Apple finishes; alternative is `submit --output-format json`, capture submission ID, poll separately.
- Stapling embeds the notarization ticket in the artifact so Gatekeeper can verify offline.
- Both `.pkg` and `.dmg` must be notarized and stapled separately; notarization is per-artifact, not per-app.

The `App Specific Password` (`APPLE_APP_PASSWORD`) is generated in [appleid.apple.com](https://appleid.apple.com) under "App-Specific Passwords". Stored in GitHub Actions secrets as `APPLE_APP_PASSWORD`.

## Finder Sync Extension

`shell-ext-mac/MaeclyPrintFinderExt/` is a Swift target. Built with `xcodebuild`:

```bash
xcodebuild -project shell-ext-mac/MaeclyPrintFinderExt/MaeclyPrintFinderExt.xcodeproj \
  -scheme MaeclyPrintFinderExt \
  -configuration Release \
  -destination "generic/platform=macOS" \
  ARCHS="x86_64 arm64" \
  ONLY_ACTIVE_ARCH=NO \
  CODE_SIGN_IDENTITY="${APPLE_DEVELOPER_ID_APPLICATION}" \
  build
```

Output is copied into `MaeclyPrint.app/Contents/PlugIns/MaeclyPrintFinderExt.appex/`.

The extension's `Info.plist` declares it's a Finder Sync Extension:

```xml
<key>NSExtension</key>
<dict>
  <key>NSExtensionPointIdentifier</key>
  <string>com.apple.FinderSync</string>
  <key>NSExtensionPrincipalClass</key>
  <string>$(PRODUCT_MODULE_NAME).FinderSync</string>
</dict>
```

Users must enable the extension in **System Settings → Extensions → Added Extensions → MaeclyPrint** after first launch. The app prompts the user to do this on first run with a deep link.

If Finder Sync Extension proves too restrictive (it has known limitations around right-click menus on multi-selections in some Finder versions), the fallback is a **Quick Action** (Automator workflow) installed at `~/Library/Services/`. Decision is made by week 3 of M5.

## Gatekeeper user experience

After notarization, on first launch:

- Downloaded `.dmg` → user double-clicks → Finder shows: "MaeclyPrint is an app downloaded from the Internet. Are you sure you want to open it?" → user clicks Open → app runs.
- Downloaded `.pkg` → user double-clicks → Installer.app launches → standard install flow → app runs.

No right-click bypass needed. No "developer cannot be verified" dialog.

If the user disables Gatekeeper or the notarization ticket is missing, they get the legacy "unidentified developer" dialog. We do not support this path.

## Universal binary on M-series CI

`macos-14` is Apple Silicon. Building the x86_64 slice from there requires Rosetta 2 only at runtime (not at build time, since Rust cross-compiles natively). The `x86_64-apple-darwin` target is added via `rustup target add x86_64-apple-darwin`. No special tooling required.

We could alternatively build x86_64 on `macos-13` (Intel) and aarch64 on `macos-14`, then combine in a third job. The single-runner approach is simpler and ~5 minutes faster end-to-end.

## Reproducibility notes

macOS reproducibility is harder than Windows due to:

- `codesign` embeds timestamps (mitigated by `--timestamp` argument which uses Apple's TSA).
- Notarization staples are timestamps embedded in the artifact.
- `pkgbuild` and `productbuild` embed timestamps in their archive metadata.

We accept that **signed + notarized** artifacts are not bit-reproducible. The **unsigned** intermediate `.app` bundle is reproducible given fixed `SOURCE_DATE_EPOCH`.

## See also

- [installer-windows.md](./installer-windows.md) — Windows counterpart
- [code-signing.md](./code-signing.md) — full code signing details for both platforms
- [auto-update-flow.md](./auto-update-flow.md) — how installed apps receive updates
- [ci-cd.md](./ci-cd.md) — where this installer is built in CI
