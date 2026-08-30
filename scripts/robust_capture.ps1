# Robust popup capture: opens the popup and takes PrintWindow shots in a loop
# until non-black content is captured (the webview sometimes needs a beat after
# show). Saves every distinct capture. Exits as soon as one capture has
# substantial content, or after the timeout.
param(
    [int]$TimeoutMs = 6000,
    [string]$Name = 'popup_capture'
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PC {
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

if (-not [Win32]::IsWindowVisible($hwnd)) { [PC]::AltV() }

$t0 = [DateTime]::UtcNow
$captured = 0
while (([DateTime]::UtcNow - $t0).TotalMilliseconds -lt $TimeoutMs) {
    if ([Win32]::IsWindowVisible($hwnd)) {
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        $ok = [Win32]::PrintWindow($hwnd, $hdc, [Win32]::PW_RENDERFULLCONTENT)
        $g.ReleaseHdc($hdc); $g.Dispose()

        $lumSum = 0.0; $n = 0
        for ($y = 0; $y -lt $h; $y += 5) {
            for ($x = 0; $x -lt $w; $x += 5) {
                $c = $bmp.GetPixel($x, $y)
                $lumSum += 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
                $n++
            }
        }
        $lum = $lumSum / $n
        $el = (([DateTime]::UtcNow) - $t0).TotalMilliseconds
        if ($ok -and $lum -gt 12) {
            $out = Join-Path $PSScriptRoot "$Name.png"
            $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
            $bmp.Dispose()
            Write-Output ("captured at +{0:N0} ms, lum={1:N0}: {2}" -f $el, $lum, $out)
            $captured++
            break
        }
        $bmp.Dispose()
        Write-Output ("t+{0:N0} ms visible ok={1} lum={2:N0}" -f $el, $ok, $lum)
    } else {
        Write-Output "popup not visible, re-showing"
        [PC]::AltV()
        Start-Sleep -Milliseconds 300
    }
    Start-Sleep -Milliseconds 120
}
if (-not [Win32]::IsWindowVisible($hwnd)) { [PC]::AltV() }
Write-Output "RESULT captured=$captured"