<#
.SYNOPSIS
    Windows print-quality end-to-end check (decision D16) against PAUSED
    queues only: nothing is ever printed.

.DESCRIPTION
    Runs src-tauri/tests/print_quality_windows.rs, which spools probe pages
    with SumatraPDF and reads each job's own DEVMODE (JOB_INFO_2.pDevMode)
    while it waits on a paused queue:

      E1/E2  Draft and High reach the job (v4 / class drivers through the
             PrintTicket psk:PageOutputQuality), and the user's per-user
             default is restored byte for byte;
      E3     a user with no per-user default gets none back;
      E4     a process killed right after the swap is recovered on the next
             start (PLURYPRINT_TEST_ABORT_AFTER_SWAP);
      E5     a change to the default made during the swap is kept;
      E6     two concurrent processes (Draft and High) each get their own;
      E7     paperkind=/bin= ids land in the job;
      FR-059 SumatraPDF's refusal of a no-print PDF is printing_not_allowed.

    Every queue used must be paused; the test refuses any other. Jobs are
    deleted on the way out, and this script removes any left behind.

    Run it as the user whose per-user defaults are being tested (the level 9
    DEVMODE lives in that user's profile), e.g. through the VM kit:
      vm run-long quality -f scripts/vm/print-quality-e2e.ps1

    This file is ASCII on purpose: Windows PowerShell 5.1 reads a BOM-less
    script in the ANSI code page.

.PARAMETER RepoRoot
    The checkout to test (its src-tauri is built). Defaults to this script's
    repository.

.PARAMETER Queues
    Paused queues to test. Defaults to Maecly_Hold_Win plus the IPP class
    driver queue created with -IppQueue.

.PARAMETER IppQueue
    Name of a paused queue on the Microsoft IPP Class Driver (the driver
    add_printer installs) bound to a local file port; created if missing.
    Pass '' to skip it.

.PARAMETER IppUrl
    The URL of a virtual IPP printer (ippeveprinter on the host, e.g.
    http://192.168.64.1:8631/ipp/print) for a paused queue on the IPP class
    driver that knows the device's print-quality-supported, so Draft and
    High are really offered. Created as -IppUrlQueue if missing. '' skips.

.PARAMETER IppUrlQueue
    The name of that queue.

.PARAMETER SumatraDir
    Folder holding SumatraPDF.exe (put first on PATH). Defaults to
    src-tauri\resources\sumatrapdf.

.PARAMETER TargetDir
    CARGO_TARGET_DIR for the build.
#>
[CmdletBinding()]
param(
    [string] $RepoRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string[]] $Queues = @('Maecly_Hold_Win'),
    [string] $IppQueue = 'Pluryprint_Hold_IPP',
    [string] $IppUrl = '',
    [string] $IppUrlQueue = 'Pluryprint_Hold_IPPEve',
    [string] $SumatraDir = '',
    [string] $TargetDir = ''
)

$ErrorActionPreference = 'Continue'
Set-StrictMode -Version Latest

function Clear-Jobs([string] $name) {
    Get-PrintJob -PrinterName $name -ErrorAction SilentlyContinue | Remove-PrintJob -ErrorAction SilentlyContinue
}

if ($IppQueue) {
    if (-not (Get-Printer -Name $IppQueue -ErrorAction SilentlyContinue)) {
        $port = Join-Path $env:TEMP "$IppQueue.prn"
        if (-not (Get-PrinterPort -Name $port -ErrorAction SilentlyContinue)) {
            Add-PrinterPort -Name $port
        }
        Add-Printer -Name $IppQueue -DriverName 'Microsoft IPP Class Driver' -PortName $port
    }
    # Paused before anything can reach it.
    Invoke-CimMethod -MethodName Pause -InputObject (Get-CimInstance Win32_Printer -Filter "Name='$IppQueue'") | Out-Null
    $Queues = @($Queues) + $IppQueue
}

if ($IppUrl) {
    if (-not (Get-Printer -Name $IppUrlQueue -ErrorAction SilentlyContinue)) {
        Add-Printer -Name $IppUrlQueue -IppURL $IppUrl
    }
    Invoke-CimMethod -MethodName Pause -InputObject (Get-CimInstance Win32_Printer -Filter "Name='$IppUrlQueue'") | Out-Null
    $Queues = @($Queues) + $IppUrlQueue
}

foreach ($q in $Queues) {
    if ((Get-Printer -Name $q).PrinterStatus -ne 'Paused') { throw "refusing to use $q`: it is not paused" }
    if ($q -match '^EPSON') { throw "refusing to use $q`: it is a real printer" }
    Clear-Jobs $q
    Get-Printer -Name $q | Select-Object Name, DriverName, PortName, PrinterStatus | Format-List | Out-String | Write-Host
}

if (-not $SumatraDir) { $SumatraDir = Join-Path $RepoRoot 'src-tauri\resources\sumatrapdf' }
if (-not (Test-Path (Join-Path $SumatraDir 'SumatraPDF.exe'))) {
    throw "no SumatraPDF.exe in $SumatraDir (run scripts\fetch-sumatrapdf.ps1)"
}
$env:PATH = "$SumatraDir;C:\LLVM\bin;$env:USERPROFILE\.cargo\bin;" + $env:PATH
$env:CARGO_INCREMENTAL = '0'
if ($TargetDir) { $env:CARGO_TARGET_DIR = $TargetDir }
$env:PLURYPRINT_SKIP_MIGRATION = '1'
$env:MAECLY_REAL_PRINTER = '1'
$env:PLURYPRINT_QUALITY_QUEUES = ($Queues -join ',')

Push-Location (Join-Path $RepoRoot 'src-tauri')
try {
    cargo test --test print_quality_windows -- --ignored --test-threads=1 --nocapture 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
} finally {
    Pop-Location
    foreach ($q in $Queues) { Clear-Jobs $q }
}
Write-Host "print-quality-e2e exit=$code"
exit $code
