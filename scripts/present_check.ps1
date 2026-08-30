# Determines whether the popup's WebView2 content actually appears on the
# screen. Opens the popup and takes timed screen captures (CopyFromScreen) at
# 0.2/0.6/1.5/3s after visibility flips, plus PrintWindow and the DWM
# last-present-time at each step.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VP {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out long value, int size);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const int DWMWA_LAST_PRESENT_TIME = 12;
    public static void AltV() {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
        keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
    public static long Present(IntPtr h) { long v; return DwmGetWindowAttribute(h, DWMWA_LAST_PRESENT_TIME, out v, 8) == 0 ? v : -1; }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)

$r = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$r)
$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
$pre = [VP]::Present($hwnd)
Write-Output ("before open: visible={0} present={1}" -f [Win32]::IsWindowVisible($hwnd), $pre)

[VP]::AltV()
$t0 = [DateTime]::UtcNow
for ($i = 0; $i -lt 60 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
$tVis = [DateTime]::UtcNow
Write-Output ("visible at +{0:N0} ms" -f ($tVis - $t0).TotalMilliseconds)

foreach ($delayMs in 100, 300, 800, 2000) {
    Start-Sleep -Milliseconds $delayMs
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $lumSum = 0.0; $n = 0
    for ($y = 0; $y -lt $h; $y += 8) {
        for ($x = 0; $x -lt $w; $x += 8) {
            $c = $bmp.GetPixel($x, $y)
            $lumSum += 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            $n++
        }
    }
    $bmp.Dispose()
    $elapsed = (([DateTime]::UtcNow) - $t0).TotalMilliseconds
    Write-Output ("t=+{0,5:N0} ms  screen avg lum={1,5:N0}  present={2}" -f $elapsed, ($lumSum / $n), [VP]::Present($hwnd))
}

# PrintWindow as the content check
$bmp2 = New-Object System.Drawing.Bitmap($w, $h)
$g2 = [System.Drawing.Graphics]::FromImage($bmp2)
$hdc = $g2.GetHdc()
$ok = [Win32]::PrintWindow($hwnd, $hdc, [Win32]::PW_RENDERFULLCONTENT)
$g2.ReleaseHdc($hdc); $g2.Dispose()
$out = Join-Path $PSScriptRoot 'popup_content_check.png'
$bmp2.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp2.Dispose()
Write-Output "PrintWindow ok=$ok -> $out"
[VP]::AltV()