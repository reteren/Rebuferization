# Captures the open popup at 1:1 (physical pixels) and analyzes:
#  - text overflow: bright (text-like) pixels in the gaps to the right of
#    each text card, and just inside each text card's right edge
#  - thumbnail rendering: color variance per image card (real thumbnail vs
#    flat broken-image placeholder)
#  - DPI scale of the window (to rule scaling in/out of the overflow story)
# Also dumps a compact UIA inventory (tabs, groups, badges, status bar).
param([string]$Name = 'verify11')
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class DpiProbe {
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowDpiAwarenessContext(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }
if (-not [Win32]::IsWindowVisible($hwnd)) { throw 'popup not visible; open it first' }

$dpi = [DpiProbe]::GetDpiForWindow($hwnd)
$sysDpi = [DpiProbe]::GetDpiForSystem()
Write-Output "window DPI=$dpi system DPI=$sysDpi scale=$([Math]::Round($dpi / 96.0, 2))x"

$rect = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$rect)
$w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
Write-Output "window rect: $($rect.Left),$($rect.Top) ${w}x${h}"

# --- UIA inventory ---------------------------------------------------------
$root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
$names = @()
$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($el in $all) {
    $n = $el.Current.Name
    if ($n) {
        $r = $el.Current.BoundingRectangle
        $names += [pscustomobject]@{ Name = $n; X = [int]$r.X; Y = [int]$r.Y; W = [int]$r.Width; Hgt = [int]$r.Height }
    }
}
Write-Output "--- UIA named elements ($($names.Count)) ---"
$names | Where-Object { $_.W -gt 0 -and $_.Hgt -gt 0 } | ForEach-Object {
    Write-Output ("{0,5},{1,5} {2,4}x{3,4}  {4}" -f $_.X, $_.Y, $_.W, $_.Hgt, $_.Name)
} | Select-Object -First 60

# --- 1:1 capture -----------------------------------------------------------
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
$g.Dispose()
$out = Join-Path $PSScriptRoot "$Name.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "captured 1:1 -> $out"

# --- text card regions from UIA -------------------------------------------
# WebView2 exposes grid cells; find cards whose name is long text (>= 40 chars
# = text previews) and cards that are image cells (name empty). Fall back to a
# geometric scan of the whole grid area if UIA cells are unavailable.
$textRects = @()
$imageRects = @()
foreach ($el in $all) {
    $ct = $el.Current.ControlType.ProgrammaticName
    $n = $el.Current.Name
    $r = $el.Current.BoundingRectangle
    if ($r.Width -lt 40 -or $r.Height -lt 40) { continue }
    $rx = [int]$r.X - $rect.Left; $ry = [int]$r.Y - $rect.Top
    $rw = [int]$r.Width; $rh = [int]$r.Height
    if ($rx -lt 0 -or $ry -lt 0 -or $rx + $rw -gt $w -or $ry + $rh -gt $h) { continue }
    if ($ct -match 'ListItem|DataItem|Group' -and $n.Length -ge 40) {
        $textRects += [pscustomobject]@{ X = $rx; Y = $ry; W = $rw; H = $rh; Name = $n.Substring(0, 40) }
    }
}
Write-Output "--- text card rects from UIA: $($textRects.Count) ---"
$textRects | Select-Object -First 8 | ForEach-Object { Write-Output ("x={0} y={1} w={2} h={3} name='{4}'" -f $_.X, $_.Y, $_.W, $_.H, $_.Name) }

function Test-Bright([int]$x0, [int]$y0, [int]$x1, [int]$y1) {
    # count pixels clearly brighter than the dark card background
    $hits = 0; $tot = 0; $maxLum = 0
    for ($y = $y0; $y -le $y1; $y += 1) {
        for ($x = $x0; $x -le $x1; $x += 1) {
            if ($x -lt 0 -or $x -ge $w -or $y -lt 0 -or $y -ge $h) { continue }
            $c = $bmp.GetPixel($x, $y)
            $lum = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            if ($lum -gt $maxLum) { $maxLum = $lum }
            if ($lum -gt 120) { $hits++ }
            $tot++
        }
    }
    return [pscustomobject]@{ Hits = $hits; Total = $tot; MaxLum = [int]$maxLum }
}

# --- overflow scan ---------------------------------------------------------
Write-Output '--- overflow scan (bright pixels right of text cards) ---'
$anyBleed = $false
foreach ($r in $textRects) {
    $gapX = $r.X + $r.W + 1
    $gapW = 8
    if ($gapX + $gapW -gt $w) { $gapW = $w - $gapX - 1 }
    $gap = Test-Bright $gapX ($r.Y + 2) ($gapX + $gapW) ($r.Y + $r.H - 2)
    $inside = Test-Bright ($r.X + $r.W - 6) ($r.Y + 2) ($r.X + $r.W - 1) ($r.Y + $r.H - 2)
    $bleed = ($gap.Hits -gt 0)
    if ($bleed) { $anyBleed = $true }
    Write-Output ("card x={0} y={1} w={2} h={3}: gap-bright={4} gapMaxLum={5} insideRightMaxLum={6} -> {7}" -f $r.X, $r.Y, $r.W, $r.H, $gap.Hits, $gap.MaxLum, $inside.MaxLum, $(if ($bleed) { 'BLEED' } else { 'clipped' }))
}
Write-Output "OVERFLOW VERDICT: $(if ($anyBleed) { 'REAL overflow past card right edge' } else { 'no pixels past card right edge (clipped)' })"

# --- thumbnail variance per image card --------------------------------------
Write-Output '--- image card variance (thumbnail vs broken image) ---'
# image cards are found by scanning the grid rows for cells with photo-like
# content; use UIA: an image card shows as a cell whose name is empty/short.
$imgVariance = @()
foreach ($el in $all) {
    $ct = $el.Current.ControlType.ProgrammaticName
    $n = $el.Current.Name
    $r = $el.Current.BoundingRectangle
    if ($r.Width -lt 60 -or $r.Height -lt 60) { continue }
    if ($n) { continue }   # named cells are text/link/color/file cards
    $rx = [int]$r.X - $rect.Left; $ry = [int]$r.Y - $rect.Top
    $rw = [int]$r.Width; $rh = [int]$r.Height
    if ($rx -lt 0 -or $ry -lt 0 -or $rx + $rw -gt $w -or $ry + $rh -gt $h) { continue }
    $sum = 0.0; $sum2 = 0.0; $n2 = 0; $distinct = @{}
    for ($y = $ry + 6; $y -lt $ry + $rh - 6; $y += 4) {
        for ($x = $rx + 6; $x -lt $rx + $rw - 6; $x += 4) {
            $c = $bmp.GetPixel($x, $y)
            $lum = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            $sum += $lum; $sum2 += $lum * $lum; $n2++
            $k = "{0},{1},{2}" -f ([int]($c.R/32)*32), ([int]($c.G/32)*32), ([int]($c.B/32)*32)
            $distinct[$k] = $true
        }
    }
    if ($n2 -eq 0) { continue }
    $mean = $sum / $n2
    $sd = [Math]::Sqrt([Math]::Max(0, $sum2 / $n2 - $mean * $mean))
    $imgVariance += [pscustomobject]@{ X = $rx; Y = $ry; W = $rw; H = $rh; Mean = [int]$mean; Sd = [int]$sd; Colors = $distinct.Count }
}
$imgVariance | ForEach-Object {
    $verdict = if ($_.Sd -gt 25 -and $_.Colors -gt 25) { 'thumbnail rendered' } else { 'FLAT - broken image likely' }
    Write-Output ("img card x={0} y={1}: meanLum={2} sd={3} colors={4} -> {5}" -f $_.X, $_.Y, $_.Mean, $_.Sd, $_.Colors, $verdict)
}
$bmp.Dispose()