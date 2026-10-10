<#
.SYNOPSIS
    Download the SumatraPDF binary Pluryprint bundles on Windows, with its
    licence.

.DESCRIPTION
    SumatraPDF is the final PDF printer on Windows (ADR-0003). The installer has
    to ship it, because a clean Windows machine has no copy and every print job
    would otherwise fail with a DependencyMissing.

    THE BINARY IS DELIBERATELY NOT COMMITTED TO THIS REPOSITORY.

      * It is a ~16 MB platform-specific artifact. Git stores every version of
        it forever; a handful of SumatraPDF upgrades would outweigh the entire
        source history, and every contributor on macOS/Linux would pay for a
        file they can never run.
      * It is not ours. Re-hosting a third party's compiled binary in our tree
        makes us its distributor, with the GPLv3 obligations that follow
        (docs/09-legal/third-party-licenses.md). Fetching the vendor's own
        release at packaging time keeps provenance where it belongs.
      * A pinned URL plus a pinned checksum gives a stronger guarantee than a
        committed blob: what ships is byte-identical to what the SumatraPDF
        project published, and the pin is reviewable in a diff.

    The portable archives contain the executable only, no licence. The GPLv3
    text (COPYING), the BSD notice for the parts under it (COPYING.BSD) and the
    AUTHORS list are therefore fetched from the SumatraPDF source repository at
    the commit the release tag points to, each with its own pinned SHA-256, and
    copied next to the executable. A missing or altered licence file fails the
    script: shipping the binary without its licence is not an option. A
    SOURCE-OFFER.txt (GPL-3.0 section 6) is written next to them, naming the
    source commit and the source archive attached to every release.

    The architecture matters: an ARM64 installer must carry the ARM64 build
    (an x64 SumatraPDF only runs there under emulation). -Arch picks it; by
    default it follows the host (PROCESSOR_ARCHITECTURE).

    So: the release build runs this script before `tauri build`, it drops the
    files in src-tauri/resources/sumatrapdf/, and `bundle.resources` in
    src-tauri/tauri.conf.json ("resources/sumatrapdf/*") picks them up.

    Why `resources` and not `externalBin`:
      * `externalBin` is for sidecars. It requires the file on disk to carry a
        `-$TARGET_TRIPLE` suffix and is resolved at runtime through the shell
        plugin's `sidecar()` API, which Pluryprint does not use: the engine
        spawns the process itself with std::process::Command and a
        CREATE_NO_WINDOW flag it needs control of.
      * `resources` copies files verbatim into the bundle's resource directory,
        found by the plain directory walk in engine::deps::bundled_dirs. The
        glob matches only the README on a macOS/Linux build.

    This file is ASCII on purpose: Windows PowerShell 5.1 reads a BOM-less
    script in the ANSI code page, and a single typographic dash or quote then
    breaks parsing (src-tauri/tests/fetch_scripts.rs guards it).

.PARAMETER Arch
    x64 or arm64. Defaults to the host architecture.

.PARAMETER Sha256
    Override the pinned SHA-256 of the archive (for testing a new release
    before bumping the pin). Also readable from SUMATRAPDF_SHA256.

.PARAMETER Force
    Re-download even when the destination already holds this exact build.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\fetch-sumatrapdf.ps1 -Arch arm64
#>

[CmdletBinding()]
param(
    [ValidateSet('', 'x64', 'arm64')]
    [string] $Arch = '',
    [string] $Sha256 = $env:SUMATRAPDF_SHA256,
    [switch] $Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

# --- Pinned upstream release -------------------------------------------------
# Bump all of these together, never one alone, and keep
# scripts/fetch-sumatrapdf.sh in step (src-tauri/tests/fetch_scripts.rs
# compares the two).
$VERSION = '3.6.1'
# Archive SHA-256s. Computed on 2026-10-10 on macOS and, separately, on the
# Windows 11 ARM64 test VM, and identical to the hashes the Scoop "extras"
# bucket publishes for the same archives (bucket/sumatrapdf.json), which were
# computed by a third party on its own network.
$ARCHIVES = @{
    'x64'   = @{ Suffix = '64';    Sha256 = '98b33a518d42986856d225064b0cd2d3643ecf78cbf84ab873d26cc51877a544' }
    'arm64' = @{ Suffix = 'arm64'; Sha256 = 'f21091ca6741e4049b0ca375121708a7959803d39336f0468084808943667ffe' }
}
# The commit the upstream tag 3.6.1rel points to: the corresponding source.
$SOURCE_COMMIT = '16c59fde8b824ab54c56f23aef910a6fdd874ad0'
$SOURCE_REPO = 'https://github.com/sumatrapdfreader/sumatrapdf'
# Where Pluryprint publishes its releases (and the source archive above).
$OFFER_RELEASES = 'https://github.com/MAECLY/maecly-print/releases'
$OFFER_ISSUES = 'https://github.com/MAECLY/maecly-print/issues'
# Licence files from that commit: name in the repository -> name in the bundle.
$LICENSES = @(
    @{ From = 'COPYING';     To = 'LICENSE.txt';     Sha256 = '3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986' },
    @{ From = 'COPYING.BSD'; To = 'COPYING.BSD.txt'; Sha256 = 'ff33648659aa06892ed13a731588a57006fafee2f848d35f70bf273a13cf9d27' },
    @{ From = 'AUTHORS';     To = 'AUTHORS.txt';     Sha256 = 'e16c411b4a9e65058e198dbb3a5e5941a65d9c44bd1a321b977644ec0b18aa53' }
)

if (-not $Arch) {
    $hostArch = $env:PROCESSOR_ARCHITEW6432
    if (-not $hostArch) { $hostArch = $env:PROCESSOR_ARCHITECTURE }
    if ($hostArch -eq 'ARM64') { $Arch = 'arm64' } else { $Arch = 'x64' }
}
$pin = $ARCHIVES[$Arch]
$ARCHIVE_URL = "https://www.sumatrapdfreader.org/dl/rel/$VERSION/SumatraPDF-$VERSION-$($pin.Suffix).zip"
if (-not $Sha256) { $Sha256 = $pin.Sha256 }
$Sha256 = $Sha256.ToUpperInvariant()

$repoRoot = Split-Path -Parent $PSScriptRoot
$destDir = Join-Path $repoRoot 'src-tauri/resources/sumatrapdf'
$destExe = Join-Path $destDir 'SumatraPDF.exe'
$stamp = Join-Path $destDir 'SOURCE.txt'
$offer = Join-Path $destDir 'SOURCE-OFFER.txt'
$stampLine = "SumatraPDF $VERSION $Arch $Sha256"

if ((Test-Path $destExe) -and (Test-Path $stamp) -and (Test-Path $offer) -and (-not $Force)) {
    if ((Get-Content -Path $stamp -TotalCount 1) -eq $stampLine) {
        Write-Host "SumatraPDF $VERSION ($Arch) already present at $destExe (use -Force to refresh)."
        exit 0
    }
}

function Get-Verified([string] $Url, [string] $OutFile, [string] $Expected) {
    Write-Host "Downloading $Url"
    Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
    $actual = (Get-FileHash -Path $OutFile -Algorithm SHA256).Hash
    if ($actual -ne $Expected.ToUpperInvariant()) {
        throw "Checksum mismatch for $Url`n  expected $($Expected.ToUpperInvariant())`n  actual   $actual"
    }
    Write-Host "Checksum OK ($actual)."
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("pluryprint-sumatra-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work -Force | Out-Null

try {
    # TLS 1.2 for Windows PowerShell 5.1 on older .NET defaults.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $archive = Join-Path $work 'sumatrapdf.zip'
    Get-Verified $ARCHIVE_URL $archive $Sha256
    Expand-Archive -Path $archive -DestinationPath (Join-Path $work 'unpacked') -Force

    # The portable archive holds a single, version-stamped executable
    # (SumatraPDF-3.6.1-64.exe). It is installed under the stable name the
    # resolver looks for (engine::deps::spec -> "SumatraPDF.exe"), so a
    # version bump never changes the lookup.
    $exe = Get-ChildItem -Path (Join-Path $work 'unpacked') -Filter '*.exe' -Recurse |
        Select-Object -First 1
    if (-not $exe) {
        throw "No .exe inside $ARCHIVE_URL - has the archive layout changed?"
    }

    # GPLv3 obligation: the licence travels with the binary we redistribute.
    $fetched = @()
    foreach ($l in $LICENSES) {
        $url = "https://raw.githubusercontent.com/sumatrapdfreader/sumatrapdf/$SOURCE_COMMIT/$($l.From)"
        $out = Join-Path $work $l.To
        Get-Verified $url $out $l.Sha256
        $fetched += $out
    }

    New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    Copy-Item -Path $exe.FullName -Destination $destExe -Force
    foreach ($f in $fetched) { Copy-Item -Path $f -Destination $destDir -Force }
    $source = @(
        $stampLine,
        '',
        "SumatraPDF.exe is the unmodified $Arch build of SumatraPDF $VERSION,",
        "downloaded from $ARCHIVE_URL",
        '',
        'SumatraPDF is free software under the GNU General Public License v3',
        '(LICENSE.txt); parts of it are under the BSD licence in COPYING.BSD.txt.',
        'Its complete corresponding source code is the upstream repository at',
        "commit $SOURCE_COMMIT (tag ${VERSION}rel):",
        "$SOURCE_REPO/tree/$SOURCE_COMMIT"
    )
    Set-Content -Path $stamp -Value $source -Encoding ASCII

    # GPL-3.0 section 6: the Corresponding Source of the object code we
    # convey. release.yml attaches the same tree to every release as
    # SumatraPDF-<version>-source.tar.gz.
    $offerText = @(
        'Written offer for the Corresponding Source of SumatraPDF',
        '(GNU General Public License version 3, section 6)',
        '',
        "This copy of Pluryprint includes SumatraPDF.exe, the unmodified $Arch build",
        "of SumatraPDF $VERSION. SumatraPDF is free software under the GNU General",
        'Public License version 3 (LICENSE.txt); parts of it are under the BSD',
        'licence in COPYING.BSD.txt, and MuPDF, compiled into it, is under the GNU',
        'Affero General Public License version 3. Pluryprint runs it as a separate',
        'program and does not modify it.',
        '',
        'Its Corresponding Source is the SumatraPDF source tree at commit',
        "$SOURCE_COMMIT (tag ${VERSION}rel):",
        "  $SOURCE_REPO/tree/$SOURCE_COMMIT",
        'An archive of that exact tree is attached to every Pluryprint release that',
        "ships this build, next to the installers, as SumatraPDF-$VERSION-source.tar.gz:",
        "  $OFFER_RELEASES",
        '',
        'For at least three years after the last Pluryprint release that ships this',
        'build, the Pluryprint maintainers will give anyone who asks a copy of that',
        'Corresponding Source, at no charge beyond the cost of physically performing',
        'the transfer. Ask through the issue tracker:',
        "  $OFFER_ISSUES"
    )
    Set-Content -Path $offer -Value $offerText -Encoding ASCII

    Write-Host "SumatraPDF $VERSION ($Arch) installed at $destExe, with its licence."
} finally {
    Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
}
