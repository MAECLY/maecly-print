# Shell Extensions

> **TL;DR** — On Windows we ship a tiny C++ COM DLL implementing `IExplorerCommand`, registered through a **Sparse MSIX** package so the entry appears in the modern Win 11 context menu (not relegated to "Show more options"). On macOS the integration ships **inside the app bundle** as an `NSServices` declaration answered by a Rust/`objc2` provider — no `.appex`, no sandbox, no separate signature and nothing for the user to enable (see §3). In both cases the shell extension does *no real work*: it serializes the user's selection and spawns the main MaeclyPrint binary.

## 1. The problem we are solving

Right-clicking files in Windows Explorer and picking "Print" calls `IContextMenu::InvokeCommand` with the print verb. Windows then enumerates the selected items, opens each one in its registered application's print handler, and dispatches one print at a time. Two things go wrong with this default path:

1. **The ~15-file limit.** When you select more than about 15 files and right-click, Explorer hides or disables the Print verb. (The exact threshold depends on the file types and registry policy, but it is real and well-known.)
2. **No batching, no profile, no queue.** Each file is printed independently, with whatever default printer + settings are active, and there is no progress UI.

MaeclyPrint replaces this with a single context-menu entry — **"Print all with MaeclyPrint"** — that:

- Receives the *full* selection (no 15-file ceiling).
- Opens a unified dialog with a profile selector and per-job overrides.
- Enqueues all files, runs converters in parallel where safe, and prints sequentially with progress reporting.

To do this properly on Windows 11 we need a real shell extension, not a registry-only command.

## 2. Windows 11: the modern context menu

Windows 11 introduced a redesigned context menu in 2021. Two surfaces exist:

- **Modern menu** — the new, compact menu shown by default.
- **"Show more options"** — falls through to the legacy Win 10 menu.

Registry-only entries (under `HKCR\*\shell\<verb>`) appear *only* in the legacy menu. To reach the modern menu, an entry must be:

1. Implemented as a COM server exposing `IExplorerCommand` (and optionally `IExplorerCommandState`, `IInitializeCommand`).
2. Registered via a **packaged identity** — either a full MSIX install or a *sparse* MSIX package (which lets a non-MSIX app obtain identity without changing how it is installed).
3. Declared in the package manifest with the `desktop4:Extension` element category `windows.fileExplorerContextMenuHandler`.

This is documented but underexposed; most third-party apps still rely on the registry-only path and accept the legacy-menu demotion. MaeclyPrint takes the proper path because the right-click experience is the entire UX.

### 2.1 The COM DLL

The DLL is C++17, ~500–1000 LOC, and links against `windowsapp.lib`, `shell32.lib`, `ole32.lib`. It exposes one COM coclass implementing `IExplorerCommand`.

#### IExplorerCommand interface

```cpp
class __declspec(uuid("{E63C5A1F-2E8B-4D7C-9F31-7B2C0F8E4A2D}"))
PrintAllCommand : public winrt::implements<
    PrintAllCommand,
    IExplorerCommand,
    IObjectWithSite>
{
public:
    // IExplorerCommand
    IFACEMETHODIMP GetTitle(IShellItemArray*, LPWSTR* name) override;
    IFACEMETHODIMP GetIcon(IShellItemArray*, LPWSTR* icon) override;
    IFACEMETHODIMP GetToolTip(IShellItemArray*, LPWSTR* infoTip) override;
    IFACEMETHODIMP GetCanonicalName(GUID* guid) override;
    IFACEMETHODIMP GetState(IShellItemArray*, BOOL okToBeSlow, EXPCMDSTATE* state) override;
    IFACEMETHODIMP Invoke(IShellItemArray* selection, IBindCtx*) override;
    IFACEMETHODIMP GetFlags(EXPCMDFLAGS* flags) override;
    IFACEMETHODIMP EnumSubCommands(IEnumExplorerCommand**) override;

    // IObjectWithSite
    IFACEMETHODIMP SetSite(IUnknown* site) override { m_site = site; return S_OK; }
    IFACEMETHODIMP GetSite(REFIID riid, void** site) override { return m_site.copy_to(riid, site); }
private:
    winrt::com_ptr<IUnknown> m_site;
};
```

Only `Invoke` does meaningful work; the rest return localized strings, an icon resource ID, the canonical name (`MaeclyPrint.PrintAll`), and a state of `ECS_ENABLED` whenever the selection contains at least one file.

#### Inside `Invoke`

```cpp
IFACEMETHODIMP PrintAllCommand::Invoke(IShellItemArray* items, IBindCtx*) noexcept
try {
    DWORD count = 0;
    winrt::check_hresult(items->GetCount(&count));

    // Build a JSON array of absolute paths.
    std::wstring json = L"[";
    for (DWORD i = 0; i < count; ++i) {
        winrt::com_ptr<IShellItem> item;
        winrt::check_hresult(items->GetItemAt(i, item.put()));

        wil::unique_cotaskmem_string path;
        winrt::check_hresult(item->GetDisplayName(SIGDN_FILESYSPATH, path.put()));

        if (i > 0) json += L",";
        json += L"\"";
        json += json_escape(path.get());
        json += L"\"";
    }
    json += L"]";

    // For small selections (<= 1024 chars total) pass on the command line.
    // For larger selections, write to a temp file and pass --files-from=<path>.
    std::wstring exe = locate_main_exe();         // installed alongside the DLL
    std::wstring args;
    if (json.size() <= 8000) {
        args = L"--invoke=context-menu --files=" + quote(json);
    } else {
        std::wstring temp = write_temp_file(json);
        args = L"--invoke=context-menu --files-from=" + quote(temp);
    }

    SHELLEXECUTEINFOW sei{};
    sei.cbSize = sizeof(sei);
    sei.fMask  = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    sei.lpVerb = L"open";
    sei.lpFile = exe.c_str();
    sei.lpParameters = args.c_str();
    sei.nShow  = SW_SHOWNORMAL;
    if (!ShellExecuteExW(&sei)) {
        return HRESULT_FROM_WIN32(GetLastError());
    }
    return S_OK;
}
catch (...) { return winrt::to_hresult(); }
```

Notes:

- We use `IShellItem::GetDisplayName(SIGDN_FILESYSPATH)` which gives a real absolute path, not a virtual or library-virtualized name.
- The DLL **never reads file content**. It only handles paths.
- The DLL does not allocate large buffers in the Explorer process beyond the path JSON.
- Errors are returned as HRESULTs; Explorer surfaces them as a balloon, but we also write a single-line crash report into `%LOCALAPPDATA%\Maecly\MaeclyPrint\data\logs\shellext.log`.

### 2.2 Sparse MSIX package

A *sparse* package is an MSIX package that declares package identity for an externally installed app. The user installs MaeclyPrint via Inno Setup (small `.exe` installer) into `%LOCALAPPDATA%\Programs\MaeclyPrint`, and as part of that install we register the sparse package. The OS then treats the externally located `.exe` and `.dll` as if they were inside a packaged app, granting the COM-server-as-shell-extension capability.

#### AppManifest.xml fragments

```xml
<?xml version="1.0" encoding="utf-8"?>
<Package
    xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
    xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
    xmlns:uap3="http://schemas.microsoft.com/appx/manifest/uap/windows10/3"
    xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"
    xmlns:desktop4="http://schemas.microsoft.com/appx/manifest/desktop/windows10/4"
    xmlns:desktop5="http://schemas.microsoft.com/appx/manifest/desktop/windows10/5"
    IgnorableNamespaces="uap uap3 com desktop4 desktop5">

  <Identity Name="MaeclyPrint" Publisher="CN=MaeclyPrint" Version="1.0.0.0"/>

  <Properties>
    <DisplayName>MaeclyPrint</DisplayName>
    <PublisherDisplayName>MaeclyPrint</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>

  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop"
                        MinVersion="10.0.19041.0"
                        MaxVersionTested="10.0.26100.0"/>
  </Dependencies>

  <Resources>
    <Resource Language="en-us"/>
  </Resources>

  <Applications>
    <Application Id="MaeclyPrint" Executable="MaeclyPrint.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
          DisplayName="MaeclyPrint"
          Description="Print N files in batch from Explorer."
          BackgroundColor="transparent"
          Square150x150Logo="Assets\Square150x150Logo.png"
          Square44x44Logo="Assets\Square44x44Logo.png"/>

      <Extensions>

        <!-- 1. Register the COM in-process server -->
        <com:Extension Category="windows.comServer">
          <com:ComServer>
            <com:SurrogateServer DisplayName="MaeclyPrint Shell Extension">
              <com:Class Id="E63C5A1F-2E8B-4D7C-9F31-7B2C0F8E4A2D"
                         Path="MaeclyPrintShellExt.dll"
                         ThreadingModel="STA"/>
            </com:SurrogateServer>
          </com:ComServer>
        </com:Extension>

        <!-- 2. Declare it as a context-menu handler for ALL files -->
        <desktop4:Extension Category="windows.fileExplorerContextMenuHandler">
          <desktop4:FileExplorerContextMenus>
            <desktop5:ItemType Type="*">
              <desktop5:Verb Id="MaeclyPrintAll" Clsid="E63C5A1F-2E8B-4D7C-9F31-7B2C0F8E4A2D"/>
            </desktop5:ItemType>
            <!-- Optionally, scope to specific extensions to avoid noise: -->
            <!--
            <desktop5:ItemType Type=".pdf">
              <desktop5:Verb Id="MaeclyPrintAll" Clsid="..."/>
            </desktop5:ItemType>
            -->
          </desktop4:FileExplorerContextMenus>
        </desktop4:Extension>

      </Extensions>
    </Application>
  </Applications>

  <Capabilities>
    <rescap:Capability Name="runFullTrust"
        xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"/>
  </Capabilities>
</Package>
```

#### Registering the sparse package

Inno Setup runs this PowerShell snippet at install time:

```powershell
$manifest = "$env:LOCALAPPDATA\Programs\MaeclyPrint\AppxManifest.xml"
$externalLocation = "$env:LOCALAPPDATA\Programs\MaeclyPrint"

Add-AppxPackage `
    -Path $manifest `
    -ExternalLocation $externalLocation `
    -Register
```

`-ExternalLocation` is the key: it tells Windows that the app's binaries live outside the AppX deployment root. The `<uap10:AllowExternalContent>true</uap10:AllowExternalContent>` declaration in the `Properties` block is required for this to be honored.

At uninstall time:

```powershell
Get-AppxPackage MaeclyPrint | Remove-AppxPackage
```

### 2.3 Choice of `Type="*"` vs explicit list

We register against `Type="*"` so the command appears for any file, then filter at runtime in the Tauri exe. This is preferred because:

- Users expect to be able to print any file we can convert.
- Adding a new converter (e.g. `.epub`) does not require re-registering or updating the AppManifest.
- The DLL is too thin to do meaningful filtering.

The cost is a small amount of menu clutter for users who select files we can't handle. We mitigate this by filtering out unsupported types in the dialog (showing them grouped at the bottom with a "skip" badge).

### 2.4 Fallback: registry-only handler

For users on Windows 10 (or who refuse MSIX), we ship a registry-only fallback that puts the same verb into `HKCU\Software\Classes\*\shell\MaeclyPrint`. This appears only in the legacy "Show more options" menu on Win 11 but works fully on Win 10. The installer asks during setup which mode is preferred; default is **Sparse MSIX on Win 11, registry on Win 10**.

```reg
[HKEY_CURRENT_USER\Software\Classes\*\shell\MaeclyPrint]
@="Print all with MaeclyPrint"
"Icon"="C:\\Users\\<user>\\AppData\\Local\\Programs\\MaeclyPrint\\MaeclyPrint.exe,0"

[HKEY_CURRENT_USER\Software\Classes\*\shell\MaeclyPrint\command]
@="\"C:\\Users\\<user>\\AppData\\Local\\Programs\\MaeclyPrint\\MaeclyPrint.exe\" --invoke=context-menu --files=\"%1\""
```

The registry path only receives one file at a time per `%1` substitution, which is exactly the limitation Windows imposes. To work around this, the second-and-later launches are detected by the Tauri exe via a named mutex and the additional paths are pushed into the running instance over a named pipe.

## 3. macOS: an NSService inside the app

**Status: implemented.** The Finder integration ships in the app bundle itself, as an
[NSService](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/SysServices/Articles/properties.html).
There is no `.appex`, no Automator workflow and no second bundle to sign. The
alternatives — a Finder Sync extension, an Action extension, an Automator Quick
Action — and the reasoning that rejected them are tabulated in
[`shell-ext-mac/README.md`](../../shell-ext-mac/README.md); this section
describes what exists.

### 3.1 What the user sees

Selecting any number of files or folders in the Finder, right-clicking and
opening the **Services** submenu offers:

- **Print with MaeclyPrint** — opens the print dialog pre-loaded with the
  selection (`action = configure`);
- **Print with MaeclyPrint (default profile)** — prints straight away with the
  default profile (`action = print-default`).

The entry is one level deeper than the Windows 11 top-level menu. That is the
price of not shipping a sandboxed extension the user has to enable, and it is
the only mechanism of the four that keeps working under an MDM policy that
blocks ExtensionKit. A user who wants it faster can bind a shortcut in
**System Settings → Keyboard → Keyboard Shortcuts → Services**.

### 3.2 The declaration

[`src-tauri/Info.plist`](../../src-tauri/Info.plist) carries an `NSServices`
array with one dictionary per menu entry. The Tauri CLI (v2) looks for
`Info.plist` next to `tauri.conf.json` and merges it over the plist it
generates, so no bundler script and no `bundle.macOS.infoPlist` setting is
needed — the file's presence is the whole mechanism.

```xml
<key>NSServices</key>
<array>
  <dict>
    <key>NSMenuItem</key>     <dict><key>default</key><string>Print with MaeclyPrint</string></dict>
    <key>NSMessage</key>      <string>printWithMaeclyPrint</string>
    <key>NSPortName</key>     <string>MaeclyPrint</string>
    <key>NSUserData</key>     <string>configure</string>
    <key>NSSendTypes</key>    <array><string>public.file-url</string></array>
    <key>NSSendFileTypes</key><array><string>public.item</string></array>
    <key>NSRequiredContext</key><dict/>
    <key>NSTimeout</key>      <string>10000</string>
  </dict>
  <!-- second entry: same NSMessage, NSUserData = print-default -->
</array>
```

Four keys carry the load:

| Key | Why it is what it is |
| --- | --- |
| `NSMessage` | Names the provider method minus the `:userData:error:` tail AppKit appends: `printWithMaeclyPrint` ⇒ `-printWithMaeclyPrint:userData:error:`. Changing one without the other silently produces a menu entry that does nothing. |
| `NSPortName` | Must be the app's `CFBundleName`, i.e. `MaeclyPrint`. |
| `NSUserData` | The official way to let two menu entries share one `NSMessage`. Its values are the CLI's `--action` vocabulary (`configure`, `print-default`), so the Finder and the command line cannot drift apart. |
| `NSSendFileTypes` | `public.item` covers both files and folders, which is what FR-004 needs. |

`NSRequiredContext` is declared empty ("no restriction"). Apple's guide treats
it as required; several macOS releases are reported to filter out services that
omit it entirely, and an empty dictionary costs nothing.

**Deliberately not declared: `CFBundleDocumentTypes`.** It would add a free
"Open With → MaeclyPrint" entry, but Launch Services delivers those files
through `application:openURLs:`, an AppKit delegate callback the app does not
implement today. Declaring it would advertise a capability that silently drops
the user's files. It is a follow-up, not a freebie.

### 3.3 The provider

[`src-tauri/src/platform/macos/services.rs`](../../src-tauri/src/platform/macos/services.rs)
defines `MaeclyPrintServicesProvider`, an Objective-C class built with `objc2`
(the same `objc2 0.6` / `objc2-*` `0.3` versions `tao` already pulls in, so the
integration adds **zero** new crates to the dependency graph). Tauri's `setup`
hook registers it:

```rust
#[cfg(target_os = "macos")]
if let Err(e) = crate::platform::macos::services::register(app.handle()) {
    tracing::warn!(event = "macos_services_registration_failed", error = %e);
}
```

`register` calls `-[NSApplication setServicesProvider:]` and then
`NSUpdateDynamicServices()`, which forces the system to re-read this bundle's
declarations — without it a freshly built app inherits the previous build's (or
no) entries from the Services cache.

When the user picks the menu entry, AppKit calls
`-printWithMaeclyPrint:userData:error:` on the main thread. The handler:

1. maps `userData` onto a `ContextAction` (unknown ⇒ `configure`: never print
   something the user did not ask for);
2. reads the selection with
   `-[NSPasteboard readObjectsForClasses:options:]` for `NSURL`, which returns
   the **whole** selection — there is no documented ceiling, which is what
   FR-003 needs — and drops anything that is not a file URL, the same way the
   Windows side drops shell items with no `SIGDN_FILESYSPATH`;
3. writes the selection as a handoff file and hands it to the app (§3.4);
4. runs inside `catch_unwind`: a panic unwinding into an Objective-C frame is
   undefined behaviour, and in a release build (`panic = "abort"`) it would take
   the app down mid-print.

One behaviour worth knowing: `NSURL` reports paths in the filesystem's
**decomposed** form (NFD), so `ñ` arrives as `n` + U+0303, not the precomposed
character a user types. The path still opens the file; string comparisons
against a path from any other source do not match. Anything that deduplicates
across sources must compare canonicalized paths, never raw strings. There is a
test pinning this.

### 3.4 Handoff, and why there is no relaunch

The Windows extension is a DLL inside Explorer, so it *must* start or signal the
app: it writes a handoff file and launches
`maeclyprint --invoke=context-menu --files-from <PATH>`. On macOS the service is
answered by the app itself — Launch Services starts it if it is not running and
reuses it if it is, which is the same single-instance guarantee
`tauri-plugin-single-instance` gives on Windows. Relaunching our own binary from
inside our own process would add a spawn, a window race and an untested path,
so the selection is delivered in-process.

It still travels through the handoff file
([`src-tauri/src/platform/macos/handoff.rs`](../../src-tauri/src/platform/macos/handoff.rs)
writes it; [`src-tauri/src/cli/handoff.rs`](../../src-tauri/src/cli/handoff.rs)
reads it), for three reasons: the selection then passes the *same* validation
gate a Windows invocation passes (schema version, freshness window, directory
containment, path and byte caps); the file is an artefact tests can assert on
without AppKit; and the bytes are identical to what a `--files-from` relaunch
would carry, so the schema cannot fork per platform. The write is a temp file
plus `rename`, and the handoff directory is `chmod 0700`.

After validation the selection is parked in `PendingSelections` and announced
with the `SelectionForwarded` event — exactly what the single-instance callback
does on Windows — and the main window is unminimized and focused.

```
Finder selection
      │  (Services menu)
      ▼
NSApplication ──► MaeclyPrintServicesProvider.printWithMaeclyPrint:userData:error:
                        │  NSPasteboard → Vec<PathBuf>
                        ▼
              platform::macos::handoff::write   ──►  <data_dir>/handoff/sel-<uuid>.json
                        │
                        ▼
              cli::handoff::{is_within_dir, consume, is_fresh}   ← the shared gate
                        │
                        ▼
              PendingSelections + SelectionForwarded  ──►  print dialog
```

### 3.5 Verifying it

`cargo test` covers everything that does not need the Finder: payload writing
and its caps, the pasteboard→paths decoding (built from real `NSURL`s), hostile
file names, the NFD behaviour above, and the round trip through the reader.

Making the menu entry actually *appear* needs a real bundle registered with
Launch Services; the commands (`lsregister -f`, `pbs -flush`,
`pbs -dump_pboard | grep`, `killall Finder`) are in
[`shell-ext-mac/README.md`](../../shell-ext-mac/README.md). CI can assert two of
those steps — that `NSServices` survived the bundler, and that `pbs` discovered
the service — but that the entry renders in the Finder's menu is not
automatable.

## 4. Flow: User right-click → DLL → spawn → Tauri renders

```
+-----------+      +-------------------+      +----------------------+      +---------------+
| Explorer  |      | MaeclyPrint       |      | MaeclyPrint.exe      |      | WebView2      |
| (or       | ---> | shell ext DLL     | ---> | (Rust core)          | ---> | print dialog  |
| Finder)   |      | (C++/Swift)       |      |                      |      | (React UI)    |
+-----------+      +-------------------+      +----------------------+      +---------------+
                          |                            |                            |
                          | 1. enumerate items         |                            |
                          | 2. resolve full paths      |                            |
                          | 3. dedupe                  |                            |
                          | 4. serialize JSON          |                            |
                          | 5. spawn Tauri exe         |                            |
                          | 6. exit                    |                            |
                          X                            |                            |
                                                       | A. parse --files arg       |
                                                       | B. acquire singleton mutex |
                                                       |    (or pipe to existing)   |
                                                       | C. boot Tauri runtime      |
                                                       | D. open print dialog       |
                                                       |--------------------------> |
                                                       |                            |
                                                       |       user picks profile,  |
                                                       |       hits "Print"         |
                                                       |<-------------------------- |
                                                       | E. enqueue jobs            |
                                                       | F. progress events to UI   |
```

### Why this shape?

- **Cheap DLL.** Anything beyond serialization in the DLL risks crashing Explorer.
- **Single source of truth.** The Tauri exe is the only place that knows about profiles, supported formats, the queue, etc. The DLL doesn't need to be redeployed when a converter is added.
- **Singleton.** A second right-click while the dialog is open routes new files into the existing instance (named pipe `\\.\pipe\MaeclyPrint`).

## 5. Logging and diagnostics

The DLL logs only to `%LOCALAPPDATA%\Maecly\MaeclyPrint\data\logs\shellext.log`, one line per invocation:

```
2026-05-03T12:34:56Z INVOKE files=12 transport=args ok=true exe=...\MaeclyPrint.exe
2026-05-03T12:35:11Z INVOKE files=312 transport=files-from path=...\filelist-abc.json ok=true
2026-05-03T12:36:02Z INVOKE files=1 transport=args ok=false hr=0x80004005
```

The Tauri exe logs in JSONL to its own log file (see [`data-model.md`](./data-model.md)).

## 6. Testing the shell extension

- **Unit (DLL).** Mock `IShellItemArray` and assert the JSON output.
- **Integration (Win).** A PowerShell script enumerates known test files, builds a `Shell.Application` COM object, finds the verb, and invokes it; assert the Tauri exe was launched with the correct `--files` payload.
- **Manual.** A `tools/install-dev-shellext.ps1` script registers the development build of the sparse package without uninstalling the production install.

On macOS everything that does not need the Finder is a plain `cargo test` in `platform::macos::*` — handoff writing and its caps, `NSPasteboard`→paths decoding built from real `NSURL`s, hostile file names, the action vocabulary. Whether the system *discovered* the service is assertable in CI (`plutil -extract NSServices` on the bundled `Info.plist`, then `lsregister -f` + `pbs -flush` + `pbs -dump_pboard | grep`); whether the entry renders in the Finder's menu is not automatable (see `shell-ext-mac/README.md`).

## 7. Related documents

- [`system-architecture.md`](./system-architecture.md) — where this fits in the overall layout.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — exact `--invoke` flag semantics.
- ADR [`0005-shell-extension-sparse-msix`](../adr/0005-shell-extension-sparse-msix.md) — why Sparse MSIX over the alternatives.
- ADR [`0006-installer-inno-setup`](../adr/0006-installer-inno-setup.md) — how the package is registered at install time.
