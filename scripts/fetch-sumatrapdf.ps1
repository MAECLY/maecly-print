<#
.SYNOPSIS
    Download the SumatraPDF binary MaeclyPrint bundles on Windows.

.DESCRIPTION
    SumatraPDF is the final PDF printer on Windows (ADR-0003). The installer has
    to ship it, because a clean Windows machine has no copy and every print job
    would otherwise fail with a DependencyMissing.

    THE BINARY IS DELIBERATELY NOT COMMITTED TO THIS REPOSITORY.

      * It is a ~20 MB platform-specific artifact. Git stores every version of
        it forever; a handful of SumatraPDF upgrades would outweigh the entire
        source history, and every contributor on macOS/Linux would pay for a
        file they can never run.
      * It is not ours. Re-hosting a third party's compiled binary in our tree
        makes us its distributor, with the GPLv3 obligations that follow
        (docs/09-legal/third-party-licenses.md). Fetching the vendor's own
        release at packaging time keeps provenance where it belongs, and this
        script copies the upstream LICENSE next to the executable so the bundle
        still carries it.
      * A pinned URL plus a pinned checksum gives a stronger guarantee than a
        committed blob: what ships is byte-identical to what the SumatraPDF
        project published, and the pin is reviewable in a diff.

    So: CI runs this script before `tauri build`, it drops the executable in
    src-tauri/resources/sumatrapdf/, and `bundle.resources` in
    src-tauri/tauri.conf.json ("resources/sumatrapdf/*") picks it up from there.

    Why `resources` and not `externalBin`:
      * `externalBin` is for sidecars. It requires the file on disk to carry a
        `-$TARGET_TRIPLE` suffix (SumatraPDF-x86_64-pc-windows-msvc.exe) and is
        resolved at runtime through the shell plugin's `sidecar()` API, which
        MaeclyPrint does not use — the engine spawns the process itself with
        std::process::Command and a CREATE_NO_WINDOW flag it needs control of.
        It also expects a binary per build target, which is nonsense for a
        Windows-only helper we must not ship inside the macOS .app.
      * `resources` copies files verbatim into the bundle's resource directory,
        resolvable with BaseDirectory::Resource (and, in our case, found by the
        plain directory walk in engine::deps::bundled_dirs). The glob matches
        nothing on a macOS/Linux build, which is exactly the behaviour we want.

.PARAMETER Sha256
    Expected SHA-256 of the downloaded archive. Defaults to $EXPECTED_SHA256
    below; the script refuses to run while that is empty (see the comment on
    it). Also readable from the SUMATRAPDF_SHA256 environment variable.

.PARAMETER Force
    Re-download even when the destination already holds an executable.

.EXAMPLE
    pwsh -File scripts/fetch-sumatrapdf.ps1
#>

[CmdletBinding()]
param(
    [string] $Sha256 = $env:SUMATRAPDF_SHA256,
    [switch] $Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# --- Pinned upstream release -------------------------------------------------
# Bump these three together, never one alone.
$VERSION = '3.5.2'
$ARCHIVE_URL = "https://www.sumatrapdfreader.org/dl/rel/$VERSION/SumatraPDF-$VERSION-64.zip"

# PIN ME. Intentionally empty: no checksum has been verified against the
# upstream release from this repository yet, and inventing one would make the
# verification below a lie that always passes. Fill it with the SHA-256 the
# SumatraPDF project publishes for exactly the file above, commit it, and the
# build becomes reproducible and tamper-evident. Until then CI must pass the
# value explicitly (-Sha256 / SUMATRAPDF_SHA256) and this script fails closed.
$EXPECTED_SHA256 = ''

$repoRoot = Split-Path -Parent $PSScriptRoot
$destDir = Join-Path $repoRoot 'src-tauri/resources/sumatrapdf'
$destExe = Join-Path $destDir 'SumatraPDF.exe'

if (-not $Sha256) { $Sha256 = $EXPECTED_SHA256 }

if ((Test-Path $destExe) -and (-not $Force)) {
    Write-Host "SumatraPDF already present at $destExe (use -Force to refresh)."
    exit 0
}

if (-not $Sha256) {
    Write-Error @'
No SHA-256 to verify against.

Set the pin in this script ($EXPECTED_SHA256), or pass one explicitly:
    pwsh -File scripts/fetch-sumatrapdf.ps1 -Sha256 <hex>
    $env:SUMATRAPDF_SHA256 = '<hex>'; pwsh -File scripts/fetch-sumatrapdf.ps1

Downloading an unverified binary and bundling it into a signed installer is not
something this script will do silently.
'@
    exit 1
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("maeclyprint-sumatra-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work -Force | Out-Null

try {
    $archive = Join-Path $work 'sumatrapdf.zip'
    Write-Host "Downloading $ARCHIVE_URL"
    Invoke-WebRequest -Uri $ARCHIVE_URL -OutFile $archive -UseBasicParsing

    $actual = (Get-FileHash -Path $archive -Algorithm SHA256).Hash
    if ($actual -ne $Sha256.ToUpperInvariant()) {
        Write-Error "Checksum mismatch for $ARCHIVE_URL`n  expected $($Sha256.ToUpperInvariant())`n  actual   $actual"
        exit 1
    }
    Write-Host "Checksum OK ($actual)."

    Expand-Archive -Path $archive -DestinationPath (Join-Path $work 'unpacked') -Force

    # The portable archive holds a single, version-stamped executable
    # (SumatraPDF-3.5.2-64.exe). We install it under the stable name the
    # resolver looks for (engine::deps::spec -> "SumatraPDF.exe"), so bumping
    # the version never changes the lookup.
    $exe = Get-ChildItem -Path (Join-Path $work 'unpacked') -Filter '*.exe' -Recurse |
        Select-Object -First 1
    if (-not $exe) {
        Write-Error "No .exe inside $ARCHIVE_URL — has the archive layout changed?"
        exit 1
    }

    New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    Copy-Item -Path $exe.FullName -Destination $destExe -Force

    # GPLv3 obligation: the licence travels with the binary we redistribute.
    $license = Get-ChildItem -Path (Join-Path $work 'unpacked') -Filter 'COPYING*' -Recurse |
        Select-Object -First 1
    if ($license) {
        Copy-Item -Path $license.FullName -Destination (Join-Path $destDir 'LICENSE.txt') -Force
    } else {
        Write-Warning 'No licence file in the archive; add SumatraPDF LICENSE.txt to the bundle by hand.'
    }

    Write-Host "SumatraPDF $VERSION installed at $destExe"
} finally {
    Remove-Item -Path $work -Recurse -Force -ErrorAction SilentlyContinue
}
