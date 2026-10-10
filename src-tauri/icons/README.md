# Icons

The app icon is the "Abanico" mark: a printer releasing three sheets in a fan.
It is drawn in code, not exported by hand, so every size agrees with every other:

```sh
pnpm icons   # node scripts/icons/generate.mjs
```

The generator writes three SVG sources to `source/` and runs `pnpm tauri icon` on each:

- `source/macos.svg`: the master, with Apple's rounded tile inset inside the canvas (macOS does not mask app icons). Produces `icon.icns`, the PNGs, and the iOS and Android sets.
- `source/windows.svg`: the same drawing edge to edge. Produces `icon.ico` and the `Square*Logo.png` / `StoreLogo.png` tiles.
- `source/small.svg`: a heavier version without motion strokes or text lines. Used for `32x32.png`, `Square30x30Logo.png` and `Square44x44Logo.png`.

Change the geometry in `scripts/icons/generate.mjs`, never the PNGs. `src/components/Brand.tsx` draws the same mark inside the app, so change both together.
