# Icons

This directory holds the bundled application icons:

- `32x32.png`
- `128x128.png`
- `128x128@2x.png`
- `icon.icns` (macOS)
- `icon.ico` (Windows)

These are required by `pnpm tauri build` (release bundling) but **not** by `pnpm tauri dev`. Until the design renders the canonical app icon, generate placeholders from a single source PNG:

```powershell
# From the repo root, with a 1024x1024 source.png in src-tauri/icons/:
pnpm tauri icon src-tauri/icons/source.png
```

This produces every required size + format. The canonical source design lives in `claude design/maeclyprint/project/brand.jsx` (see `MPAppIcon`).
