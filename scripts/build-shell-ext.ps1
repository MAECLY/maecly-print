<#
.SYNOPSIS
    Build the Explorer context-menu DLL for the installer.

.DESCRIPTION
    shell_ext_win.dll (shell-ext-win/) is a separate crate with its own target
    directory and release profile, so `tauri build` does not build it. The
    per-user NSIS installer ships it as a bundle resource, one copy per
    Windows architecture:

        src-tauri/resources/shellext/x64/shell_ext_win.dll
        src-tauri/resources/shellext/arm64/shell_ext_win.dll

    and src-tauri/windows/installer-hooks.nsh registers the one matching the
    operating system's native architecture (Explorer on Windows on ARM cannot
    load an x64 DLL, even when the x64 installer runs there emulated). The
    hooks refuse to build an installer with neither DLL, and a CI build
    (CI=true) without both.

    This script builds the DLL in release mode (with
    PLURYPRINT_REQUIRE_RESOURCES=1, so a DLL without its embedded manifest
    fails the build) and copies it there. Run it before `pnpm tauri build`,
    next to scripts/fetch-sumatrapdf.ps1.

    ASCII only, so Windows PowerShell 5.1 parses it.

.PARAMETER Arch
    x64, arm64 or all. Defaults to the host architecture; release builds use
    all (the cross target needs `rustup target add` and the MSVC libraries for
    it).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\build-shell-ext.ps1 -Arch all
#>

[CmdletBinding()]
param(
    [ValidateSet('', 'x64', 'arm64', 'all')]
    [string] $Arch = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $Arch) {
    $hostArch = $env:PROCESSOR_ARCHITEW6432
    if (-not $hostArch) { $hostArch = $env:PROCESSOR_ARCHITECTURE }
    if ($hostArch -eq 'ARM64') { $Arch = 'arm64' } else { $Arch = 'x64' }
}
$arches = if ($Arch -eq 'all') { @('x64', 'arm64') } else { @($Arch) }
$triples = @{ 'x64' = 'x86_64-pc-windows-msvc'; 'arm64' = 'aarch64-pc-windows-msvc' }

$repoRoot = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $repoRoot 'shell-ext-win/Cargo.toml'
$env:PLURYPRINT_REQUIRE_RESOURCES = '1'

foreach ($a in $arches) {
    $triple = $triples[$a]
    $built = Join-Path $repoRoot "shell-ext-win/target/$triple/release/shell_ext_win.dll"
    $destDir = Join-Path $repoRoot "src-tauri/resources/shellext/$a"

    Write-Host "Building shell_ext_win.dll for $triple"
    & cargo build --release --manifest-path $manifest --target $triple
    if ($LASTEXITCODE -ne 0) { throw "cargo build of shell-ext-win for $triple failed ($LASTEXITCODE)" }
    if (-not (Test-Path $built)) { throw "Build succeeded but $built is missing" }

    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
    Copy-Item -Path $built -Destination (Join-Path $destDir 'shell_ext_win.dll') -Force
    Write-Host "shell_ext_win.dll ($a) copied to $destDir"
}
