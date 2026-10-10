<#
.SYNOPSIS
    Drive the Explorer context menu like a user, for the Windows VM e2e (FR-001).

.DESCRIPTION
    Runs inside the logged-on user's desktop session (from the Mac:
    `vm gui-ps scripts/vm/r3-contextmenu-e2e.ps1 -Path C:\x\a.pdf ...`).

    1. Opens an Explorer window with -Path selected and right-clicks it with
       real mouse input.
    2. On Windows 11, clicks "Show more options" with real mouse input
       (-Route ShowMore, the default), or opens the classic menu directly
       with Shift+right-click (-Route Shift).
    3. Hovers the "Pluryprint" entry and prints the submenu it opens.
    4. Optionally clicks a submenu entry (-Click 'Add to print queue').
    5. Optionally saves a screenshot while the menus are open (-Shot).

    Real input matters. Activating "Show more options" through UI Automation
    (InvokePattern) builds the classic menu without its owner-draw plumbing:
    every cascade in it, Windows' own "Send to" included, opens as an empty
    20x70 px box and the shell never asks a handler for the entries. That
    artefact is what the round-3 upgrade test reported as "the submenu is
    empty" (row 7b); with a real click, or the keyboard, the submenu fills.

    Classic menus (#32768) expose no children through UI Automation, so the
    entries are read with MN_GETHMENU / GetMenuString / GetMenuItemRect. The
    screenshot uses BitBlt with CAPTUREBLT, which captures layered menus that
    Graphics.CopyFromScreen misses.

    Output lines start with MENU, SUBMENU, CLICKED or WARN; the exit code is 0
    when the Pluryprint submenu had at least one entry (and -Click found its
    entry), 1 otherwise. ASCII only, for Windows PowerShell 5.1.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Path,
    [ValidateSet('ShowMore', 'Shift')][string] $Route = 'ShowMore',
    [string] $Entry = 'Pluryprint',
    [string] $Click = '',
    [string] $Shot = '',
    [switch] $KeepOpen
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text; using System.Drawing;
public static class E2E {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte v, byte s, uint f, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint id, StringBuilder s, int n, uint f);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetMenuItemRect(IntPtr h, IntPtr m, uint i, out RECT r);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr d, int x, int y, int w, int h, IntPtr s, int sx, int sy, int rop);
  public static void Click(int x, int y, bool right) {
    SetCursorPos(x, y);
    if (right) { mouse_event(8, 0, 0, 0, UIntPtr.Zero); mouse_event(16, 0, 0, 0, UIntPtr.Zero); }
    else { mouse_event(2, 0, 0, 0, UIntPtr.Zero); mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
  }
  public static Bitmap Grab(int x, int y, int w, int h) {
    var bmp = new Bitmap(w, h);
    using (var g = Graphics.FromImage(bmp)) {
      IntPtr dst = g.GetHdc(); IntPtr src = GetDC(IntPtr.Zero);
      BitBlt(dst, 0, 0, w, h, src, x, y, 0x00CC0020 | 0x40000000);
      ReleaseDC(IntPtr.Zero, src); g.ReleaseHdc(dst);
    }
    return bmp;
  }
}
"@
[void][E2E]::SetProcessDPIAware()
$AE = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
$All = [System.Windows.Automation.Condition]::TrueCondition

function Find-Top([string]$rx, [int]$seconds) {
    $end = (Get-Date).AddSeconds($seconds)
    do {
        foreach ($w in $AE::RootElement.FindAll($TS::Children, $All)) {
            if ($w.Current.Name -match $rx -or $w.Current.ClassName -match $rx) { return $w }
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $end)
    throw "no top-level window matching /$rx/"
}
function Find-Named($scope, [string]$name) {
    $scope.FindFirst($TS::Descendants, (New-Object System.Windows.Automation.PropertyCondition($AE::NameProperty, $name)))
}
function Center($e) { $r = $e.Current.BoundingRectangle; @([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) }
function Classic-Menus {
    $h = [IntPtr]::Zero
    while ($true) {
        $h = [E2E]::FindWindowEx([IntPtr]::Zero, $h, '#32768', $null)
        if ($h -eq [IntPtr]::Zero) { break }
        if ([E2E]::IsWindowVisible($h)) { $h }
    }
}
function Classic-Items([IntPtr]$only = [IntPtr]::Zero) {
    foreach ($h in @(Classic-Menus)) {
        if ($only -ne [IntPtr]::Zero -and $h -ne $only) { continue }
        $m = [E2E]::SendMessage($h, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)   # MN_GETHMENU
        if ($m -eq [IntPtr]::Zero) { continue }
        for ($i = 0; $i -lt [E2E]::GetMenuItemCount($m); $i++) {
            $sb = New-Object System.Text.StringBuilder 256
            [void][E2E]::GetMenuString($m, [uint32]$i, $sb, 256, 0x400)       # MF_BYPOSITION
            $r = New-Object E2E+RECT; [void][E2E]::GetMenuItemRect($h, $m, [uint32]$i, [ref]$r)
            [pscustomobject]@{ Hwnd = $h; Text = $sb.ToString().Replace('&', ''); X = [int](($r.L + $r.R) / 2); Y = [int](($r.T + $r.B) / 2); W = $r.R - $r.L }
        }
    }
}
function Wait-Until([scriptblock]$cond, [int]$seconds) {
    $end = (Get-Date).AddSeconds($seconds)
    while (-not (& $cond) -and (Get-Date) -lt $end) { Start-Sleep -Milliseconds 250 }
    & $cond
}

$full = (Resolve-Path $Path).Path
$leaf = Split-Path $full -Leaf
Start-Process explorer.exe -ArgumentList ('/select,"' + $full + '"')
$win = Find-Top ([regex]::Escape((Split-Path (Split-Path $full -Parent) -Leaf))) 15
# Explorer in front even when another app holds the foreground: a test
# harness may tap Alt to be allowed to call SetForegroundWindow.
[E2E]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [E2E]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
[void][E2E]::SetForegroundWindow([IntPtr]$win.Current.NativeWindowHandle)
Start-Sleep -Milliseconds 800
$item = $null
[void](Wait-Until { $script:item = Find-Named $win $leaf; if (-not $script:item) { $script:item = Find-Named $win ([IO.Path]::GetFileNameWithoutExtension($leaf)) }; $script:item } 10)
if (-not $item) { throw "the file $leaf is not visible in Explorer" }
$xy = Center $item
[E2E]::Click($xy[0], $xy[1], $false); Start-Sleep -Milliseconds 300

if ($Route -eq 'Shift') {
    [E2E]::keybd_event(0x10, 0, 0, [UIntPtr]::Zero)
    [E2E]::Click($xy[0], $xy[1], $true)
    [E2E]::keybd_event(0x10, 0, 2, [UIntPtr]::Zero)
} else {
    [E2E]::Click($xy[0], $xy[1], $true)
    $more = $null
    [void](Wait-Until {
        foreach ($w in $AE::RootElement.FindAll($TS::Children, $All)) {
            foreach ($n in 'Show more options', 'Mostrar m' + [char]0xE1 + 's opciones') {
                $script:more = Find-Named $w $n; if ($script:more) { return $true }
            }
        }
        $false
    } 8)
    if ($more) {
        $mxy = Center $more
        [E2E]::Click($mxy[0], $mxy[1], $false)     # real input, never InvokePattern (see above)
    } else {
        'WARN no "Show more options" (Windows 10, or the classic menu is already the default)'
    }
}
if (-not (Wait-Until { @(Classic-Items).Count -gt 0 } 8)) { 'WARN no classic menu opened'; exit 1 }
foreach ($ci in @(Classic-Items)) { 'MENU "{0}"' -f $ci.Text }

$root = @(Classic-Items | Where-Object { $_.Text -eq $Entry }) | Select-Object -First 1
if (-not $root) { "WARN no '$Entry' entry in the classic menu"; exit 1 }
$before = @(Classic-Menus)
[void][E2E]::SetCursorPos($root.X, $root.Y)
$sub = $null
[void](Wait-Until { $script:sub = @(Classic-Menus | Where-Object { $before -notcontains $_ }) | Select-Object -Last 1; $script:sub } 6)
Start-Sleep -Milliseconds 600
$entries = if ($sub) { @(Classic-Items $sub) } else { @() }
foreach ($e in $entries) { 'SUBMENU "{0}" width={1}' -f $e.Text, $e.W }
$filled = @($entries | Where-Object { $_.Text -and $_.W -gt 40 }).Count -gt 0
if (-not $filled) { 'WARN the submenu is empty' }

if ($Shot) {
    $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = [E2E]::Grab($vs.Left, $vs.Top, $vs.Width, $vs.Height)
    New-Item -ItemType Directory -Force -Path (Split-Path $Shot) | Out-Null
    $bmp.Save($Shot, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    "SHOT $Shot"
}

$ok = $filled
if ($Click) {
    $target = @($entries | Where-Object { $_.Text -like "$Click*" }) | Select-Object -First 1
    if ($target) {
        [void][E2E]::SetCursorPos($target.X, $target.Y); Start-Sleep -Milliseconds 300
        [E2E]::Click($target.X, $target.Y, $false)
        'CLICKED "{0}" at {1:o}' -f $target.Text, (Get-Date)
    } else { "WARN no submenu entry like '$Click'"; $ok = $false }
}
if (-not $KeepOpen) {
    if (-not $Click) { [System.Windows.Forms.SendKeys]::SendWait('{ESC}{ESC}{ESC}') }
    try { ($win.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)).Close() } catch { }
}
if ($ok) { exit 0 } else { exit 1 }
