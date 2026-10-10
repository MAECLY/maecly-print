<#
.SYNOPSIS
    Regression check: printing HTML/Markdown and starting the app must not
    record failed Windows logons.

.DESCRIPTION
    Chromium's password manager probes for a blank OS password with an empty
    LogonUser call. Each probe is a failed logon (Security event 4625), and the
    Windows 11 default lockout policy (10 failures in 10 minutes) then locks
    the account. Pluryprint seeds the converter's and the WebView2's
    "Local State" so the probe is skipped (src-tauri/src/util/chromium.rs).

    This script counts 4625 events for the current user before and after
    -Runs HTML prints through pluryprint-cli (into a paused queue, so nothing
    comes out) and, with -App, after starting and closing the GUI. It exits 1
    when any failure was recorded. Reading the Security log needs an elevated
    (or UAC-disabled administrator) session.

    ASCII only, so Windows PowerShell 5.1 parses it.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\check-failed-logons.ps1 `
        -Cli "$env:LOCALAPPDATA\Pluryprint\pluryprint-cli.exe" -Printer Maecly_Hold_Win -Runs 3 -App
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)] [string] $Cli,
    [Parameter(Mandatory = $true)] [string] $Printer,
    [int] $Runs = 3,
    [switch] $App,
    [int] $AppSeconds = 45
)

$ErrorActionPreference = 'Stop'

function Get-FailedLogons {
    $user = $env:USERNAME
    @(Get-WinEvent -FilterHashtable @{ LogName = 'Security'; Id = 4625 } -ErrorAction SilentlyContinue |
        Where-Object { $_.Properties[5].Value -eq $user }).Count
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("pluryprint-logon-check-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
$page = Join-Path $work 'logon-check.html'
Set-Content -Path $page -Value '<!doctype html><title>logon check</title><h1>Pluryprint</h1>' -Encoding ASCII

$total = 0
try {
    $before = Get-FailedLogons
    for ($i = 1; $i -le $Runs; $i++) {
        & $Cli print --files $page --printer $Printer --json | Out-Null
        Write-Host "html print $i exit $LASTEXITCODE"
    }
    Start-Sleep -Seconds 5
    $delta = (Get-FailedLogons) - $before
    Write-Host "failed logons after $Runs HTML prints: +$delta"
    $total += $delta

    if ($App) {
        $exe = Join-Path (Split-Path -Parent $Cli) 'pluryprint.exe'
        $before = Get-FailedLogons
        $p = Start-Process -FilePath $exe -PassThru
        # WebView2 runs its probe some 20 s after start.
        Start-Sleep -Seconds $AppSeconds
        if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
        Start-Sleep -Seconds 3
        $delta = (Get-FailedLogons) - $before
        Write-Host "failed logons after one app start: +$delta"
        $total += $delta
    }
} finally {
    Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
}

if ($total -gt 0) {
    Write-Error "Pluryprint caused $total failed logon(s)"
    exit 1
}
Write-Host 'OK: no failed logons'
