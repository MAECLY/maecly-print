# shell-ext-win

`shell_ext_win.dll` — the **Print with Pluryprint** entry in the Windows
Explorer context menu (FR-001, FR-002, FR-003, FR-005; ADR-0005).

It is an in-process COM server implementing `IExplorerCommand`,
`IObjectWithSelection` and `IEnumExplorerCommand`. It never prints: it
serializes the selection to a JSON handoff file and launches

```
pluryprint.exe --invoke=context-menu --files-from <path> --action <action> [--profile <id>]
```

The app (`src-tauri/src/cli/`) validates the file and takes over.

## Layout

| Path                    | What it is                                                      |
| ----------------------- | --------------------------------------------------------------- |
| `src/handoff.rs`        | Producer of the handoff file. **Portable**, tested on any host. |
| `src/strings.rs`        | Menu model + the label cache the app writes. **Portable**.      |
| `src/supported.rs`      | Show/hide decision for the entry (FR-005). **Portable**.        |
| `src/lib.rs`            | `DllMain`, `DllGetClassObject`, `DllCanUnloadNow`.              |
| `src/factory.rs`        | `IClassFactory`.                                                |
| `src/command.rs`        | Root `IExplorerCommand` + `IObjectWithSelection`.               |
| `src/subcommands.rs`    | Leaf commands + `IEnumExplorerCommand`.                         |
| `src/selection.rs`      | `IShellItemArray` → paths, and the selection cache.             |
| `src/launch.rs`         | Handoff write + `ShellExecuteExW`.                              |
| `src/guard.rs`          | `catch_unwind` barrier on every COM entry point.                |
| `src/comstr.rs`         | `CoTaskMemAlloc`'d strings returned to Explorer.                |
| `src/module.rs`         | Where the DLL (and therefore the exe) lives.                    |
| `src/logging.rs`        | One JSONL line per invocation/panic.                            |
| `res/shellext.manifest` | The side-by-side `<msix>` manifest. **Load-bearing.**           |
| `res/shellext.rc`       | Resource script; embedded by `build.rs`.                        |

## Building

```powershell
# From this directory, on Windows with the MSVC toolchain:
$env:PLURYPRINT_REQUIRE_RESOURCES = "1"   # fail the build if the manifest is not embedded
cargo build --release --target x86_64-pc-windows-msvc
```

The artifact is `target/<triple>/release/shell_ext_win.dll`. The installer must
place it **next to `pluryprint.exe`**: the extension derives the executable's
path from its own module path.

On a non-Windows host, `cargo check --target x86_64-pc-windows-msvc` type-checks
everything and `cargo test` runs the portable tests. Neither produces a usable
DLL — there is no `rc.exe`, so the `<msix>` manifest is not embedded.

## Things that will silently break the extension

1. **A missing or mismatched `<msix>` manifest.** `res/shellext.manifest` must
   agree character for character with `AppxManifest.xml`:
   `publisher` ↔ `Identity/@Publisher`, `packageName` ↔ `Identity/@Name`,
   `applicationId` ↔ `Application/@Id`. The identical manifest must also be
   embedded in `pluryprint.exe`. Symptom: `0x80073D54`, or an extension that
   registers cleanly and never appears.
2. **`panic = "abort"`.** Both profiles in `Cargo.toml` pin `"unwind"`. With
   `abort`, `catch_unwind` catches nothing and a panic takes `explorer.exe` down.
   Do not fold this crate into a workspace whose root profile says otherwise.
3. **A changed CLSID.** `src/clsid.rs` is the only place it should ever be
   edited, and it should never be edited: already-installed machines keep the
   old one registered.
4. **Explorer caching the old DLL.** Explorer must be restarted after
   (re)registering; several reports (PowerToys, NanaZip) describe machines where
   a newly registered packaged handler does not appear until `explorer.exe` is
   killed.

## Registration (owned by the installer, not by this crate)

The DLL exports **no** `DllRegisterServer`. On Windows 11 the sparse package
supplies the COM registration; on Windows 10 the installer writes the registry
itself. Both are outside this crate.

CLSID: `{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}`

### Windows 11 — sparse (external-location) package

```xml
<com:Extension Category="windows.comServer">
  <com:ComServer>
    <com:SurrogateServer DisplayName="Pluryprint Shell Extension">
      <com:Class Id="B269B8DC-044F-4D65-8F1E-8F20A2E653EA"
                 Path="shell_ext_win.dll" ThreadingModel="STA" />
    </com:SurrogateServer>
  </com:ComServer>
</com:Extension>

<desktop4:Extension Category="windows.fileExplorerContextMenus">
  <desktop4:FileExplorerContextMenus>
    <desktop5:ItemType Type="*">
      <desktop5:Verb Id="Pluryprint" Clsid="B269B8DC-044F-4D65-8F1E-8F20A2E653EA" />
    </desktop5:ItemType>
    <desktop5:ItemType Type="Directory">
      <desktop5:Verb Id="Pluryprint" Clsid="B269B8DC-044F-4D65-8F1E-8F20A2E653EA" />
    </desktop5:ItemType>
  </desktop4:FileExplorerContextMenus>
</desktop4:Extension>
```

The category is `windows.fileExplorerContextMenus` — **not**
`windows.fileExplorerContextMenuHandler`, which appears in some of the repo's
docs and does not exist.

**Remove the old identity first.** Before ADR-0009 the package identity was
`MaeclyPrint` (`packageName` / `applicationId` in `res/shellext.manifest`). A
machine where that package was ever registered — a developer box, or a test VM
— still has it, and it claims this same CLSID and verb. Whatever registers the
Pluryprint package (installer step, `tools/install-dev-shellext.ps1`) must
first run

```powershell
Get-AppxPackage -Name MaeclyPrint | Remove-AppxPackage
```

and the uninstaller must remove both names. No such package was ever shipped,
so this is a requirement for `installers/windows/sparse-msix/` when it is
written, not a migration that exists today.

### Windows 10 — registry verb, same DLL

Windows 10 has no modern menu but does honour `ExplorerCommandHandler`, so the
same object serves both. Registered per-user, no elevation:

```reg
[HKEY_CURRENT_USER\Software\Classes\*\shell\Pluryprint]
@="Print with Pluryprint"
"ExplorerCommandHandler"="{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}"
"MultiSelectModel"="Player"

[HKEY_CURRENT_USER\Software\Classes\Directory\shell\Pluryprint]
@="Print with Pluryprint"
"ExplorerCommandHandler"="{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}"
"MultiSelectModel"="Player"

[HKEY_CURRENT_USER\Software\Classes\CLSID\{B269B8DC-044F-4D65-8F1E-8F20A2E653EA}\InprocServer32]
@="<install dir>\\shell_ext_win.dll"
"ThreadingModel"="Apartment"
```

`MultiSelectModel="Player"` is assumed for COM-based verbs, but the platform
documentation says to declare it on every verb anyway. A COM verb has **no**
item limit; the 15-item cap belongs to `Document`-model _command-line_ verbs.
Do **not** register `shellex\ContextMenuHandlers`: that route calls
`IShellExtInit`/`IContextMenu`, which this DLL does not implement.

## The menu label cache

The app writes `<data_local_dir>\shell-menu.json` (`src-tauri/src/shell_menu.rs`)
at startup and whenever the profile list, the default profile or the UI
language changes, from the same i18n catalog the UI uses (`common.json` →
`shellMenu`). Missing or invalid is fine — the menu falls back to built-in
English with no profile entries. `tests/fixtures/shell-menu.es.json` is the
app's exact output for a fixed input; both crates' tests read it.

```json
{
  "schema_version": 1,
  "lang": "es",
  "labels": {
    "root": "Pluryprint",
    "print_one": "Imprimir 1 archivo",
    "print_many": "Imprimir {count} archivos",
    "print_with_profile": "Imprimir con «{name}»",
    "configure": "Configurar trabajo de impresión…",
    "queue": "Añadir a la cola",
    "tooltip": "Imprime la selección con Pluryprint"
  },
  "profiles": [{ "id": "quick", "name": "Rápido" }]
}
```

At most 8 profiles are shown (16-item ceiling per handler); the rest stay
reachable through "Configure print job…". The Windows 11 top-level menu does not
support a submenu inside a submenu, so the profile entries are flat.
