# Shell Extensions

> **TL;DR** — On Windows we ship a tiny C++ COM DLL implementing `IExplorerCommand`, registered through a **Sparse MSIX** package so the entry appears in the modern Win 11 context menu (not relegated to "Show more options"). On macOS we start with a **Quick Action** (Automator + AppleScript) for M5, then upgrade to a **Finder Sync Extension** (Swift + XPC) when polish demands it. In both cases the shell extension does *no real work*: it serializes the user's selection and spawns the main MaeclyPrint binary.

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
- Errors are returned as HRESULTs; Explorer surfaces them as a balloon, but we also write a single-line crash report into `%LOCALAPPDATA%\MaeclyPrint\logs\shellext.log`.

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

## 3. macOS: starting simple

macOS does not have a multi-select right-click "Print" entry to compete with. The two paths to the same UX are:

### 3.1 Quick Action (M5)

A Quick Action is an Automator workflow installed into `~/Library/Services/`. It appears in:

- `Finder → Services → Print all with MaeclyPrint`
- The right-click context menu, under `Quick Actions` (or `Services` on older macOS).
- The **share sheet** in some apps.

Implementation: a single `.workflow` bundle that runs an embedded AppleScript:

```applescript
on run {input, parameters}
    set pathList to {}
    repeat with f in input
        set end of pathList to POSIX path of f
    end repeat

    set pathArg to ""
    repeat with i from 1 to count of pathList
        if i > 1 then set pathArg to pathArg & ","
        set pathArg to pathArg & quoted form of (item i of pathList)
    end repeat

    do shell script "/Applications/MaeclyPrint.app/Contents/MacOS/maeclyprint " & ¬
        "--invoke=context-menu --files=" & pathArg
end run
```

Pros: no Xcode required at install; no entitlements; works on every macOS 13+ machine; shippable with the `.app` bundle.
Cons: the entry is buried under `Quick Actions`, not a top-level menu item.

### 3.2 Finder Sync Extension (post-M5 polish)

A real Finder Sync Extension is a Swift + XPC App Extension embedded in `MaeclyPrint.app/Contents/PlugIns/`. It implements `FIFinderSync` and contributes a top-level context-menu entry.

```swift
import FinderSync

class FinderSync: FIFinderSync {

    override init() {
        super.init()
        FIFinderSyncController.default().directoryURLs = [URL(fileURLWithPath: "/")]
    }

    override func menu(for menuKind: FIMenuKind) -> NSMenu {
        let menu = NSMenu(title: "")
        let item = NSMenuItem(
            title: "Print all with MaeclyPrint",
            action: #selector(printAllAction(_:)),
            keyEquivalent: "")
        item.image = NSImage(named: "MaeclyPrintIcon")
        menu.addItem(item)
        return menu
    }

    @objc func printAllAction(_ sender: AnyObject?) {
        let items = FIFinderSyncController.default().selectedItemURLs() ?? []
        let paths = items.map { $0.path }.joined(separator: ",")
        let url = URL(fileURLWithPath: "/Applications/MaeclyPrint.app/Contents/MacOS/maeclyprint")
        let proc = Process()
        proc.executableURL = url
        proc.arguments = ["--invoke=context-menu", "--files=\(paths)"]
        try? proc.run()
    }
}
```

Distribution: the `.appex` is signed with the same Apple Developer ID, included in the `.app`'s `PlugIns` folder, and the user enables it once in `System Settings → Privacy & Security → Extensions → Finder Extensions`.

Pros: top-level context menu entry, Finder badge support if we ever want it, fully native.
Cons: requires entitlements (`com.apple.security.application-groups` + `com.apple.security.files.user-selected.read-only`), is more code, and users must explicitly enable the extension.

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

The DLL logs only to `%LOCALAPPDATA%\MaeclyPrint\logs\shellext.log`, one line per invocation:

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

On macOS the Quick Action is testable via `xcrun automator -i` and `osascript`; the Finder Sync Extension is tested in the Xcode project.

## 7. Related documents

- [`system-architecture.md`](./system-architecture.md) — where this fits in the overall layout.
- [`ipc-and-cli.md`](./ipc-and-cli.md) — exact `--invoke` flag semantics.
- ADR [`0005-shell-extension-sparse-msix`](../adr/0005-shell-extension-sparse-msix.md) — why Sparse MSIX over the alternatives.
- ADR [`0006-installer-inno-setup`](../adr/0006-installer-inno-setup.md) — how the package is registered at install time.
