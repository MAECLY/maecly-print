# Bundled SumatraPDF

`SumatraPDF.exe` is the final PDF printer on Windows (ADR-0003) and ships inside
the installer, because a clean Windows machine has no copy and every print job
would otherwise fail with a `DependencyMissing`.

**The executable is not committed to this repository.** It is a ~20 MB
third-party artifact; git would keep every version of it forever, every
contributor on macOS/Linux would pay for a file they can never run, and
re-hosting someone else's compiled binary in our tree makes us its distributor
with the GPLv3 obligations that follow (see
`docs/09-legal/third-party-licenses.md`).

Instead it is fetched from the vendor's own release at packaging time, with a
pinned URL and a pinned SHA-256:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\fetch-sumatrapdf.ps1 -Arch arm64   # or x64
```

```bash
scripts/fetch-sumatrapdf.sh --arch arm64    # POSIX twin, for cross-builds
```

Both scripts drop `SumatraPDF.exe` (the build for the installer's
architecture), its licence (`LICENSE.txt`, `COPYING.BSD.txt`, `AUTHORS.txt`,
fetched from the pinned upstream source commit, since the portable archives
carry none), a `SOURCE.txt` naming the version, architecture and source
commit, and a `SOURCE-OFFER.txt` (GPL-3.0 section 6: where the corresponding
source is, and the written offer) in this directory. Everything here except this README is git-ignored.
`bundle.resources` in `src-tauri/tauri.conf.json` maps
`"resources/sumatrapdf/*"` to `"resources/sumatrapdf/"`, so whatever is here
is copied verbatim into the
bundle's resource directory — which is the first place
`engine::deps::bundled_dirs` looks.

This file exists so that glob always matches something: `tauri-build` fails the
build when a resource pattern matches no files, and on a macOS or Linux build
there is (correctly) no Windows executable here.

`resources`, not `externalBin`: `externalBin` is the sidecar mechanism. It wants
a `-$TARGET_TRIPLE` suffix on the file name, is invoked through the shell
plugin's `sidecar()` API — which Pluryprint does not use, since the engine
spawns the process itself to pass `CREATE_NO_WINDOW` — and expects a binary per
build target, which is wrong for a Windows-only helper that must not end up
inside the macOS `.app`.
