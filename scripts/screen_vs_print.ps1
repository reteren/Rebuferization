# Decisive: does the popup's WebView2 content ever appear on the SCREEN?
# Alternates CopyFromScreen (what the user sees) and PrintWindow (what the
# webview can produce) at the same moments.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VP7 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public static void AltV() {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
        keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
$r = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$r)
$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
Write-Output ("popup rect: {0},{1} {2}x{3}  cursor: {4}" -f $r.Left, $r.Top, $w, $h, [System.Windows.Forms.Cursor]::Position)

[VP7]::AltV()
$t0 = [DateTime]::UtcNow
for ($i = 0; $i -lt 60 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }

function Lum($bmp) {
    $sum = 0.0; $n = 0
    for ($y = 0; $y -lt $bmp.Height; $y += 6) {
        for ($x = 0; $x -lt $bmp.Width; $x += 6) {
            $c = $bmp.GetPixel($x, $y)
            $sum += 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            $n++
        }
    }
    return ($sum / $n)
}

$idx = 0
foreach ($at in 150, 400, 800, 1500, 2500) {
    Start-Sleep -Milliseconds ($at - ($idx -eq 0 ? 0 : @(150, 400, 800, 1500, 2500)[$idx - 1]))
    $idx++
    if (-not [Win32]::IsWindowVisible($hwnd)) { Write-Output "popup closed before t=$at ms"; break }
    $el = (([DateTime]::UtcNow) - $t0).TotalMilliseconds

    $scr = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($scr)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $lumS = Lum $scr

    $pw = New-Object System.Drawing.Bitmap($w, $h)
    $g2 = [System.Drawing.Graphics]::FromImage($pw)
    $hdc = $g2.GetHdc()
    $ok = [Win32]::PrintWindow($hwnd, $hdc, [Win32]::PW_RENDERFULLCONTENT)
    $g2.ReleaseHdc($hdc); $g2.Dispose()
    $lumP = Lum $pw

    Write-Output ("t=+{0,5:N0} ms  screen lum={1,5:N0}  printWindow lum={2,5:N0} (ok={3})" -f $el, $lumS, $lumP, $ok)
    $scr.Dispose(); $pw.Dispose()
    if ($el -ge 1500) { break }
}
if (-not [Win32]::IsWindowVisible($hwnd)) { [VP7]::AltV() }