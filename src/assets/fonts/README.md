# Bundled fonts

## JetBrains Mono

- **Source:** https://github.com/JetBrains/JetBrainsMono
- **License:** SIL Open Font License 1.1 (compatible with the AGPL-3.0)
- **Files:**
  - `JetBrainsMono-Regular.woff2`
  - `JetBrainsMono-Medium.woff2`
  - `JetBrainsMono-SemiBold.woff2`

The copyright notice and the full license text are in `LICENSES/OFL-1.1.txt` at the repository root, which ships in the app bundle; `THIRD-PARTY-NOTICES.md` lists the fonts.

To refresh the bundled files (e.g. when JetBrains ships a new version):

```powershell
$dir = "src/assets/fonts"
$base = "https://github.com/JetBrains/JetBrainsMono/raw/master/fonts/webfonts"
foreach ($f in "JetBrainsMono-Regular.woff2","JetBrainsMono-Medium.woff2","JetBrainsMono-SemiBold.woff2") {
  Invoke-WebRequest -Uri "$base/$f" -OutFile "$dir/$f" -UseBasicParsing
}
```

## Why we bundle (not Google Fonts CDN)

Tauri is offline-first and ships air-gapped via the installer. Loading fonts from a CDN at runtime would (a) leak a request to `fonts.gstatic.com` on every cold start, leaking usage signal, (b) fail in offline environments, and (c) create a runtime dependency on an external service. Bundling adds ~280 KB to the final binary, which is acceptable.
