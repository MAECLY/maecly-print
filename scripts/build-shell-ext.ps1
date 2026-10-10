<#
.SYNOPSIS
    Build the Explorer context-menu DLL for the installer.

.DESCRIPTION
    shell_ext_win.dll (shell-ext-win/) is a separate crate with its own target
    directory and release profile, so `tauri build` does not build it. The
    per-user NSIS installer ships it through src-tauri/windows/installer-hooks.nsh,
    which takes it from src-tauri/windows/shell_ext_win.dll and refuses to build
    the installer when it is missing.

    This script builds the DLL in release mode for the installer's architecture
    (with PLURYPRINT_REQUIRE_RESOURCES=1, so a DLL without its embedded manifest
    fails the build) and copies it there. Run it before `pnpm tauri build`, next
    to scripts/fetch-sumatrapdf.ps1.

    ASCII only, so Windows PowerShell 5.1 parses it.

.PARAMETER Arch
    x64 or arm64. Defaults to the host architecture.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\build-shell-ext.ps1 -Arch arm64
#>

[CmdletBinding()]
param(
    [ValidateSet('', 'x64', 'arm64')]
    [string] $Arch = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $Arch) {
    $hostArch = $env:PROCESSOR_ARCHITEW6432
    if (-not $hostArch) { $hostArch = $env:PROCESSOR_ARCHITECTURE }
    if ($hostArch -eq 'ARM64') { $Arch = 'arm64' } else { $Arch = 'x64' }
}
$triple = @{ 'x64' = 'x86_64-pc-windows-msvc'; 'arm64' = 'aarch64-pc-windows-msvc' }[$Arch]

$repoRoot = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $repoRoot 'shell-ext-win/Cargo.toml'
$built = Join-Path $repoRoot "shell-ext-win/target/$triple/release/shell_ext_win.dll"
$dest = Join-Path $repoRoot 'src-tauri/windows/shell_ext_win.dll'

$env:PLURYPRINT_REQUIRE_RESOURCES = '1'
Write-Host "Building shell_ext_win.dll for $triple"
& cargo build --release --manifest-path $manifest --target $triple
if ($LASTEXITCODE -ne 0) { throw "cargo build of shell-ext-win failed ($LASTEXITCODE)" }
if (-not (Test-Path $built)) { throw "Build succeeded but $built is missing" }

Copy-Item -Path $built -Destination $dest -Force
Write-Host "shell_ext_win.dll ($Arch) copied to $dest"
