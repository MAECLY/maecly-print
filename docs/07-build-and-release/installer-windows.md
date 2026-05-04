# Installer (Windows)

> **TL;DR.** MaeclyPrint uses Inno Setup 6 to produce a self-extracting `setup.exe` (~10 MB) that installs to `%LOCALAPPDATA%\Programs\MaeclyPrint` (no admin rights required), registers HKCU shell entries, and deploys a Sparse MSIX package to grant identity to the Win 11 modern shell extension.

This document specifies the structure of the Inno Setup script, the Sparse MSIX manifest, the install layout, all registry writes, dependency handling, and the uninstall flow. The goal: any contributor can reproduce a byte-identical installer from a clean checkout, and any user can trust that uninstall leaves no residue.

## Why Inno Setup

Inno Setup 6 was chosen over alternatives for these reasons:

| Tool | Verdict | Why / why not |
|---|---|---|
| **Inno Setup** | Chosen | Free for commercial use, Pascal-like scripting (full control), single self-extracting `.exe`, mature, no admin install supported, large ecosystem of examples. |
| WiX Toolset / MSI | Rejected | MSI requires admin for system install; per-user MSI is convoluted; XML scripting is verbose; Windows Installer service interactions unpredictable for shell extensions. |
| MSIX (full) | Rejected as primary | Requires Microsoft Store association or sideloading flags; restricts what installers can do; AppContainer constraints break some Win32 print API calls. We use **Sparse** MSIX as a complement, not a replacement. |
| NSIS | Rejected | Capable but with smaller mindshare for our specific shell-extension + sparse-MSIX combo. |
| Custom PowerShell installer | Rejected | Antivirus heuristics flag PS-based installers more often; UX is worse. |

## Inno Setup script structure

The script lives at `installers/windows/inno-setup.iss`. Below is the section-by-section structure with the key directives. The full `.iss` is generated via the build script and pinned in the repo for reproducibility.

### `[Setup]` section

```iss
[Setup]
AppId={{B72E6F3A-1C4F-4C8E-9B62-3A4E5D6F7A8B}
AppName=MaeclyPrint
AppVersion={#MyAppVersion}
AppPublisher=MaeclyPrint Project
AppPublisherURL=https://maecly.com
AppSupportURL=https://maecly.com/support
AppUpdatesURL=https://maecly.com/releases
DefaultDirName={localappdata}\Programs\MaeclyPrint
DefaultGroupName=MaeclyPrint
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir=output
OutputBaseFilename=MaeclyPrint-Setup-{#MyAppVersion}
SetupIconFile=..\..\src-tauri\icons\icon.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64 arm64
ArchitecturesAllowed=x64 arm64
MinVersion=10.0.19041
UninstallDisplayIcon={app}\maeclyprint.exe
UninstallDisplayName=MaeclyPrint {#MyAppVersion}
SignTool=signtool
```

Notes:

- **`PrivilegesRequired=lowest`** plus the override allowance lets the user install per-user without admin (default) or system-wide if they elevate.
- **`MinVersion=10.0.19041`** is Windows 10 20H1. Lower versions get a clear "unsupported" dialog rather than a partial install.
- **`AppId` GUID** never changes across versions; it's how Windows recognizes upgrades.
- **`SignTool=signtool`** is configured globally in Inno Setup options to invoke our `signtool` wrapper that signs every produced `.exe`.

### `[Files]` section

```iss
[Files]
Source: "..\..\src-tauri\target\release\maeclyprint.exe"; DestDir: "{app}"; Flags: ignoreversion signonce
Source: "..\..\src-tauri\target\release\WebView2Loader.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\src-tauri\resources\sumatrapdf\SumatraPDF.exe"; DestDir: "{app}\sumatrapdf"; Flags: ignoreversion
Source: "..\..\src-tauri\resources\sumatrapdf\LICENSE.txt"; DestDir: "{app}\sumatrapdf"; Flags: ignoreversion
Source: "..\..\target\release\shell-ext-win.dll"; DestDir: "{app}"; Flags: ignoreversion regserver 64bit
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "sparse-msix\MaeclyPrint.msix"; DestDir: "{app}\sparse-msix"; Flags: ignoreversion; Check: IsWindows11
```

Notes:

- `signonce` ensures we sign each binary exactly once during install (we already signed during build, this is belt-and-suspenders for cases where Inno Setup repacks).
- The shell extension DLL gets `regserver` only as a safety net; the canonical registration is via the Sparse MSIX (Win 11) or HKCU registry entries (Win 10), set in `[Registry]`.
- `Check: IsWindows11` skips the MSIX file on Win 10. `IsWindows11` is a custom Pascal function (see [Code] section).

### `[Registry]` section

For Win 10 fallback (when Sparse MSIX is not deployed), we register a classic `IContextMenu` extension via HKCU:

```iss
[Registry]
; CLSID for the in-proc COM server
Root: HKCU; Subkey: "Software\Classes\CLSID\{{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}}"; ValueType: string; ValueData: "MaeclyPrint Shell Extension"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\CLSID\{{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}}\InprocServer32"; ValueType: string; ValueData: "{app}\shell-ext-win.dll"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\CLSID\{{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}}\InprocServer32"; ValueType: string; ValueName: "ThreadingModel"; ValueData: "Apartment"

; Attach to "AllFileSystemObjects" so the verb appears for any file/folder selection
Root: HKCU; Subkey: "Software\Classes\AllFileSystemObjects\shellex\ContextMenuHandlers\MaeclyPrint"; ValueType: string; ValueData: "{{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}}"; Flags: uninsdeletekey

; Register the application URL handler for the maeclyprint:// scheme (used by IPC handoff)
Root: HKCU; Subkey: "Software\Classes\maeclyprint"; ValueType: string; ValueData: "URL:MaeclyPrint Protocol"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\maeclyprint"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\maeclyprint\shell\open\command"; ValueType: string; ValueData: """{app}\maeclyprint.exe"" --uri ""%1"""
```

Notes:

- All entries are `HKCU\Software\Classes\` (per-user). No `HKLM` writes; no admin needed.
- `uninsdeletekey` ensures the key is removed on uninstall.
- The CLSID GUID `{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}` is fixed across versions; it's the contract with Windows.

### `[Icons]` section

```iss
[Icons]
Name: "{group}\MaeclyPrint"; Filename: "{app}\maeclyprint.exe"
Name: "{group}\Uninstall MaeclyPrint"; Filename: "{uninstallexe}"
Name: "{userdesktop}\MaeclyPrint"; Filename: "{app}\maeclyprint.exe"; Tasks: desktopicon
```

```iss
[Tasks]
Name: "desktopicon"; Description: "Create a &desktop icon"; GroupDescription: "Additional shortcuts:"; Flags: unchecked
```

### `[Run]` section — Sparse MSIX deployment

```iss
[Run]
Filename: "powershell.exe"; \
  Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""Add-AppxPackage -Path '{app}\sparse-msix\MaeclyPrint.msix' -ExternalLocation '{app}'"""; \
  Flags: runhidden waituntilterminated; \
  Check: IsWindows11; \
  StatusMsg: "Registering modern shell integration..."

Filename: "{app}\maeclyprint.exe"; Description: "{cm:LaunchProgram,MaeclyPrint}"; Flags: postinstall skipifsilent nowait
```

The `-ExternalLocation` flag is what makes this a **Sparse** package — the MSIX manifest declares files that live outside the package, in the main install directory. This avoids duplicating the binary while still giving us MSIX identity.

### `[UninstallRun]` section

```iss
[UninstallRun]
Filename: "powershell.exe"; \
  Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""Get-AppxPackage -Name 'MaeclyPrint' | Remove-AppxPackage"""; \
  Flags: runhidden waituntilterminated; \
  RunOnceId: "RemoveSparseMsix"
```

### `[Code]` section — custom Pascal logic

```pascal
[Code]
function IsWindows11(): Boolean;
var
  Major, Minor, Build: Cardinal;
begin
  GetWindowsVersionEx(Major, Minor, Build);
  Result := (Major = 10) and (Build >= 22000);
end;

function InitializeSetup(): Boolean;
begin
  Result := True;
  // Warn if optional dependencies missing
  if not CheckLibreOfficeInstalled() then
    MsgBox('LibreOffice is not installed. Office documents will be skipped. ' +
           'Install LibreOffice from https://www.libreoffice.org/ to enable Office support.',
           mbInformation, MB_OK);
end;

function CheckLibreOfficeInstalled(): Boolean;
var
  Found: Boolean;
begin
  Found := RegKeyExists(HKLM, 'SOFTWARE\LibreOffice\UNO\InstallPath') or
           RegKeyExists(HKCU, 'SOFTWARE\LibreOffice\UNO\InstallPath');
  Result := Found;
end;

procedure DeinitializeUninstall();
var
  AppDataPath: String;
begin
  // Optional: prompt user to remove their data
  AppDataPath := ExpandConstant('{userappdata}\MaeclyPrint');
  if DirExists(AppDataPath) then
  begin
    if MsgBox('Remove your MaeclyPrint profiles and logs at ' + AppDataPath + '?', mbConfirmation, MB_YESNO) = IDYES then
      DelTree(AppDataPath, True, True, True);
  end;
end;
```

## Sparse MSIX package

### Why "sparse"

A sparse MSIX package contains a manifest only — no payload. Files are declared with paths that point outside the package, into the main install directory. The package's only purpose is to give the shell extension DLL a **package identity**, which Windows 11's modern shell (the new File Explorer) requires for `IExplorerCommand` registration.

Without identity:

- Win 11's modern File Explorer ignores the extension.
- Only the legacy `IContextMenu` path works (Win 10 style), which doesn't surface in the new modern command bar.

### Package layout

```
sparse-msix/
├── AppxManifest.xml
├── Assets/
│   ├── StoreLogo.png         (50×50)
│   ├── Square150x150Logo.png
│   ├── Square44x44Logo.png
│   └── Wide310x150Logo.png
└── (no other files — payload lives in main install dir)
```

### `AppxManifest.xml`

```xml
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap3="http://schemas.microsoft.com/appx/manifest/uap/windows10/3"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"
  xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap uap3 uap10 com desktop rescap">

  <Identity
    Name="MaeclyPrint"
    Publisher="CN=MaeclyPrint Project, O=MaeclyPrint Project, C=US"
    Version="1.0.0.0" />

  <Properties>
    <DisplayName>MaeclyPrint</DisplayName>
    <PublisherDisplayName>MaeclyPrint Project</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>

  <Resources>
    <Resource Language="en-us" />
  </Resources>

  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.22000.0" MaxVersionTested="10.0.26200.0" />
  </Dependencies>

  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>

  <Applications>
    <Application Id="MaeclyPrint"
                 Executable="maeclyprint.exe"
                 uap10:TrustLevel="mediumIL"
                 uap10:RuntimeBehavior="win32App">
      <uap:VisualElements
        DisplayName="MaeclyPrint"
        Description="Batch print N files from File Explorer"
        BackgroundColor="transparent"
        Square150x150Logo="Assets\Square150x150Logo.png"
        Square44x44Logo="Assets\Square44x44Logo.png">
        <uap:DefaultTile Wide310x150Logo="Assets\Wide310x150Logo.png" />
      </uap:VisualElements>

      <Extensions>
        <com:Extension Category="windows.comServer">
          <com:ComServer>
            <com:SurrogateServer DisplayName="MaeclyPrint Shell Extension">
              <com:Class Id="A1B2C3D4-E5F6-7890-ABCD-EF1234567890" Path="shell-ext-win.dll" ThreadingModel="STA" />
            </com:SurrogateServer>
          </com:ComServer>
        </com:Extension>

        <desktop:Extension Category="windows.fileExplorerContextMenus">
          <desktop:FileExplorerContextMenus>
            <desktop:ItemType Type="*">
              <desktop:Verb Id="MaeclyPrintVerb" Clsid="A1B2C3D4-E5F6-7890-ABCD-EF1234567890" />
            </desktop:ItemType>
            <desktop:ItemType Type="Directory">
              <desktop:Verb Id="MaeclyPrintVerb" Clsid="A1B2C3D4-E5F6-7890-ABCD-EF1234567890" />
            </desktop:ItemType>
          </desktop:FileExplorerContextMenus>
        </desktop:Extension>
      </Extensions>
    </Application>
  </Applications>
</Package>
```

The CLSID `A1B2C3D4-...` matches the one in the Inno Setup `[Registry]` section. They must agree.

## Install directory layout

After install:

```
%LOCALAPPDATA%\Programs\MaeclyPrint\
├── maeclyprint.exe              # Main app (Tauri runtime + all logic)
├── shell-ext-win.dll            # COM in-proc server
├── WebView2Loader.dll           # WebView2 (system WebView2 used; this is loader stub)
├── LICENSE.txt                  # GPLv3
├── CHANGELOG.md
├── sumatrapdf\
│   ├── SumatraPDF.exe           # Bundled, GPLv3
│   └── LICENSE.txt              # SumatraPDF's GPLv3 notice
├── sparse-msix\
│   ├── AppxManifest.xml
│   └── Assets\
└── unins000.exe                 # Inno Setup-generated uninstaller
```

User data lives separately:

```
%APPDATA%\MaeclyPrint\
└── profiles\
    ├── default.toml
    └── (user-created profiles)

%LOCALAPPDATA%\MaeclyPrint\
└── logs\
    ├── maeclyprint.2026-05-03.jsonl
    └── (rotated daily, retained 30 days)
```

This separation matters: install-area is read-only after install (we never write there at runtime); user-area is everything that survives uninstall unless the user opts in to wipe.

## Registry surface

Complete list of registry writes performed by the installer (all under `HKCU\Software\Classes\`):

| Key | Purpose |
|---|---|
| `CLSID\{A1B2C3D4-...}` | Friendly name of the COM server |
| `CLSID\{A1B2C3D4-...}\InprocServer32` | DLL path + threading model |
| `AllFileSystemObjects\shellex\ContextMenuHandlers\MaeclyPrint` | Hooks the verb into Win 10 File Explorer |
| `maeclyprint` | URL protocol handler for `maeclyprint://` IPC scheme |
| `maeclyprint\shell\open\command` | Command to launch when URL is invoked |

Plus a single key under `HKCU\Software\MaeclyPrint`:

| Key | Purpose |
|---|---|
| `HKCU\Software\MaeclyPrint\Settings` | App-level settings (channel, language, etc.) — written by app at runtime, not by installer |

Inno Setup's standard `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\<AppId>_is1` key is also written automatically (for Add/Remove Programs).

## Dependency detection

The installer **detects** but does not **install** optional runtimes:

| Dependency | Detection method | If missing |
|---|---|---|
| WebView2 Runtime | `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}\pv` | Hard error: WebView2 is required (preinstalled on Win 11; bootstrap link shown for Win 10). |
| LibreOffice | `HKLM` or `HKCU` `SOFTWARE\LibreOffice\UNO\InstallPath` | Soft warning: Office documents will be skipped at runtime. |
| Pandoc | `where.exe pandoc` returns 0 | Soft warning: Markdown formatting may be reduced (we have a fallback). |
| ImageMagick | `where.exe magick` returns 0 | Soft warning: Image preprocessing skipped (raw image print still works). |
| Chromium / Edge | Edge is preinstalled; check `where.exe msedge` | Soft warning: HTML printing may have reduced fidelity. |

We **never** silently install third-party software. Every install action is the user's choice.

## Uninstall flow

When the user runs the uninstaller (`unins000.exe`):

1. **Stop running app:** if `maeclyprint.exe` is running, prompt the user to close it (or wait + retry).
2. **Remove sparse MSIX:** `Remove-AppxPackage -Name MaeclyPrint`. If this fails (rare), log and continue — orphan MSIX entries are harmless.
3. **Unregister shell extension DLL:** `regsvr32 /u shell-ext-win.dll` (no-op if Inno Setup's `regserver` flag wasn't used, but defensive).
4. **Delete registry entries:** all `HKCU\Software\Classes\` entries listed in `[Registry]` (Inno Setup handles via `uninsdeletekey`).
5. **Delete install directory:** Inno Setup default behavior.
6. **Prompt for user data:** ask whether to delete `%APPDATA%\MaeclyPrint` and `%LOCALAPPDATA%\MaeclyPrint`. Default is **no** (preserve user data).
7. **Refresh shell:** broadcast `WM_SETTINGCHANGE` so File Explorer re-reads context menu registrations.

After a clean uninstall (with user data wiped), `regedit` and `Process Monitor` should find zero MaeclyPrint references. We test this in CI as part of the installer test job.

## Build script

`installers/windows/build.ps1` is the single entrypoint. It:

1. Validates that `maeclyprint.exe`, `shell-ext-win.dll`, and `SumatraPDF.exe` exist in expected locations.
2. Reads version from `src-tauri/tauri.conf.json`.
3. Generates the sparse MSIX with `MakeAppx.exe` from the Windows SDK.
4. Signs the MSIX with the EV cert (release builds) or self-signed cert (nightly builds).
5. Invokes `iscc.exe inno-setup.iss /DMyAppVersion=<version>`.
6. Verifies the produced `setup.exe` is signed: `signtool verify /pa /v setup.exe`.
7. Outputs SHA-256 of the final installer for the release manifest.

```powershell
# installers/windows/build.ps1 (excerpt)
param(
  [Parameter(Mandatory)] [string]$Version,
  [ValidateSet('release', 'nightly')] [string]$Channel = 'release'
)

$ErrorActionPreference = 'Stop'

# 1. Validate prerequisites
$required = @('iscc.exe', 'MakeAppx.exe', 'SignTool.exe')
foreach ($tool in $required) {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
    throw "Required tool not found in PATH: $tool"
  }
}

# 2. Build sparse MSIX
$msixPath = "sparse-msix/MaeclyPrint.msix"
& MakeAppx.exe pack /d "sparse-msix/" /p $msixPath /nv | Out-Host
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed" }

# 3. Sign MSIX
$cert = if ($Channel -eq 'release') { $env:EV_CERT_THUMBPRINT } else { $env:NIGHTLY_CERT_THUMBPRINT }
& SignTool.exe sign /sha1 $cert /fd SHA256 /td SHA256 /tr "http://timestamp.digicert.com" $msixPath
if ($LASTEXITCODE -ne 0) { throw "MSIX signing failed" }

# 4. Build setup.exe
& iscc.exe "inno-setup.iss" "/DMyAppVersion=$Version" "/DChannel=$Channel"
if ($LASTEXITCODE -ne 0) { throw "Inno Setup compile failed" }

# 5. Verify signature
$setupPath = "output/MaeclyPrint-Setup-$Version.exe"
& SignTool.exe verify /pa /v $setupPath
if ($LASTEXITCODE -ne 0) { throw "Setup verification failed" }

# 6. Emit hash
$hash = (Get-FileHash $setupPath -Algorithm SHA256).Hash
"setup.exe SHA-256: $hash" | Out-Host
$hash | Out-File "$setupPath.sha256" -Encoding ASCII -NoNewline
```

## Reproducibility

Two builds from the same commit must produce byte-identical `setup.exe`. To achieve this:

- Inno Setup's `Compression=lzma2/ultra64` is deterministic given the same input file order.
- Files are listed in `[Files]` in a fixed order.
- The build script enforces UTC timestamps via `SOURCE_DATE_EPOCH` env var (Inno Setup 6.2+ supports this).
- Sparse MSIX is built with `MakeAppx /nv` (no version stamp randomness).
- Signing introduces a timestamp; we accept that signed binaries differ in the timestamp byte range only. The unsigned binary is the bit-for-bit reproducible artifact.

CI verifies reproducibility by building twice on the same commit and `diff`ing the unsigned installers.

## See also

- [installer-macos.md](./installer-macos.md) — macOS counterpart
- [code-signing.md](./code-signing.md) — signing details for both platforms
- [auto-update-flow.md](./auto-update-flow.md) — how installed apps receive updates
- [ci-cd.md](./ci-cd.md) — where this installer is built in CI
