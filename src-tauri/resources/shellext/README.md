# Bundled Explorer extension

The Windows installer ships the Explorer context-menu handler
(`shell_ext_win.dll`, built from `shell-ext-win/`) for **both** Windows
architectures, one folder each:

```
resources/shellext/x64/shell_ext_win.dll
resources/shellext/arm64/shell_ext_win.dll
```

The installer registers the DLL that matches the **operating system's** native
architecture, not the installer's: Explorer on Windows on ARM is a native ARM64
process and cannot load an x64 DLL, even when the x64 installer runs there
under emulation.

**The DLLs are not committed.** CI builds them with `scripts/build-shell-ext.ps1`
and copies them here before `tauri build`. This README keeps the folder in a
fresh checkout, so `tauri.conf.json`'s `"resources/shellext/"` entry resolves on
every build, including macOS ones that never ship a DLL.
