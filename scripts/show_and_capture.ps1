# Show the popup via the hotkey and capture it in the same run, so focus loss
# between invocations cannot close it mid-capture.
param(
    [string]$Name = 'popup',
    [switch]$KeepOpen,
    [switch]$FromScreen
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class KbProbe2 {
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
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }

if (-not [Win32]::IsWindowVisible($hwnd)) {
    [KbProbe2]::AltV()
    for ($i = 0; $i -lt 40 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
}
if (-not [Win32]::IsWindowVisible($hwnd)) { throw 'popup did not become visible' }

Start-Sleep -Milliseconds 600

$rect = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$rect)
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
Write-Host "window rect: $($rect.Left),$($rect.Top) ${w}x${h}"

$out = Join-Path $PSScriptRoot "$Name.png"
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [Win32]::PrintWindow($hwnd, $hdc, [Win32]::PW_RENDERFULLCONTENT)
$g.ReleaseHdc($hdc)
$g.Dispose()

$nonBlack = 0; $total = 0
for ($y = 0; $y -lt $h; $y += 4) {
    for ($x = 0; $x -lt $w; $x += 4) {
        $c = $bmp.GetPixel($x, $y)
        $total++
        if ($c.R -gt 8 -or $c.G -gt 8 -or $c.B -gt 8) { $nonBlack++ }
    }
}
$pct = if ($total -gt 0) { [Math]::Round(100.0 * $nonBlack / $total, 1) } else { 0 }
Write-Host "PrintWindow ok=$ok non-black=$pct%"

if ($ok -and $pct -gt 5) {
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "saved $out"
} elseif ($FromScreen -or -not $ok -or $pct -le 5) {
    $bmp.Dispose()
    $screen = New-Object System.Drawing.Bitmap($w, $h)
    $g2 = [System.Drawing.Graphics]::FromImage($screen)
    $g2.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g2.Dispose()
    $screen.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "screen-copy fallback saved $out"
    $screen.Dispose()
}

if (-not $KeepOpen) {
    [KbProbe2]::AltV()
}