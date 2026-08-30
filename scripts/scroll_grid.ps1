# Scrolls the popup grid and captures frames; the worker's store cycles
# plain/code/link/color/file cards among the newest items, so scrolling the
# first ~600 items surfaces color and code cards without any typing.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Scr {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint WHEEL = 0x0800;
    public static void AltV() { keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static bool IsFg(IntPtr h) { return GetForegroundWindow() == h; }
    public static void Wheel(int x, int y, int delta) { SetCursorPos(x, y); mouse_event(WHEEL, 0, 0, (uint)delta, UIntPtr.Zero); }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)

$ok = $false
for ($try = 0; $try -lt 8 -and -not $ok; $try++) {
    if (-not [Win32]::IsWindowVisible($hwnd)) { [Scr]::AltV() }
    for ($i = 0; $i -lt 80 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    for ($i = 0; $i -lt 40 -and -not [Scr]::IsFg($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    $ok = [Win32]::IsWindowVisible($hwnd) -and [Scr]::IsFg($hwnd)
    if (-not $ok) {
        if ([Win32]::IsWindowVisible($hwnd)) { [Scr]::AltV() }
        Start-Sleep -Milliseconds 700
    }
}
if (-not $ok) { throw 'could not open popup' }
Write-Output 'popup open + focused'
Start-Sleep -Milliseconds 2000

$r = New-Object Win32+RECT
for ($frame = 0; $frame -lt 10; $frame++) {
    if (-not [Scr]::IsFg($hwnd)) {
        Write-Output ("frame {0}: fg lost, re-opening" -f $frame)
        [Scr]::AltV()
        for ($i = 0; $i -lt 60 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
        for ($i = 0; $i -lt 40 -and -not [Scr]::IsFg($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
        Start-Sleep -Milliseconds 1500
    }
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    if ($frame -gt 0) {
        $delta = -1800
        $wm = [Scr].GetMethod('Wheel')
        $wm.Invoke($null, [object[]]@([int]($r.Left + $w / 2), [int]($r.Top + $h * 0.5), [int]$delta)) | Out-Null
        Start-Sleep -Milliseconds 700
    }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot ("sc_{0:00}.png" -f $frame)
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "frame $frame -> $out"
}
if ([Win32]::IsWindowVisible($hwnd)) { [Scr]::AltV() }