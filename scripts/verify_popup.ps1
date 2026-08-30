# Single-invocation popup verification: open (Alt+V), settle, screen-capture,
# Ctrl+wheel zoom + capture, Esc-close test, outside-click-close test.
# Everything happens inside ONE script run so no focus-stealing happens
# between steps (the popup closes on ANY focus loss).
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VIn {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint MOUSEEVENTF_WHEEL = 0x0800;
    public const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    public const uint MOUSEEVENTF_LEFTUP = 0x0004;
    public static void AltV() {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
        keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
    public static void Key(byte vk, bool up) { keybd_event(vk, 0, up ? KEYEVENTF_KEYUP : 0u, UIntPtr.Zero); }
    public static void CtrlWheel(int delta) {
        keybd_event(0x11, 0, 0, UIntPtr.Zero);
        mouse_event(MOUSEEVENTF_WHEEL, 0, 0, (uint)delta, UIntPtr.Zero);
        keybd_event(0x11, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
    public static void ClickAt(int x, int y) {
        SetCursorPos(x, y);
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, UIntPtr.Zero);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, UIntPtr.Zero);
    }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }

function Get-Rect {
    $r = New-Object Win32+RECT
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    return ,$r
}

function Capture-Screen([string]$name) {
    $r = Get-Rect
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot "$name.png"
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "saved $out"
}

function Wait-Visible([int]$timeoutMs) {
    for ($i = 0; $i -lt $timeoutMs / 50; $i++) {
        if ([Win32]::IsWindowVisible($hwnd)) { return $true }
        Start-Sleep -Milliseconds 50
    }
    return [Win32]::IsWindowVisible($hwnd)
}
function Wait-Hidden([int]$timeoutMs) {
    for ($i = 0; $i -lt $timeoutMs / 50; $i++) {
        if (-not [Win32]::IsWindowVisible($hwnd)) { return $true }
        Start-Sleep -Milliseconds 50
    }
    return -not [Win32]::IsWindowVisible($hwnd)
}

# --- 1. open + capture -----------------------------------------------------
if (-not [Win32]::IsWindowVisible($hwnd)) {
    [VIn]::AltV()
}
$opened = Wait-Visible 3000
Write-Output "open via Alt+V: $opened"
Start-Sleep -Milliseconds 1500
Capture-Screen 'popup_open1'

# --- 2. zoom test: ctrl+wheel over the grid --------------------------------
$r = Get-Rect
$cx = [int](($r.Left + $r.Right) / 2)
$cy = [int]($r.Top + ($r.Bottom - $r.Top) * 0.4)
[VIn]::SetCursorPos($cx, $cy)
Start-Sleep -Milliseconds 150
[VIn]::CtrlWheel(-120)
[VIn]::CtrlWheel(-120)
[VIn]::CtrlWheel(-120)
Start-Sleep -Milliseconds 800
Write-Output "zoom: ctrl+wheel x3 sent"
Capture-Screen 'popup_zoomed'

# --- 3. Esc closes ---------------------------------------------------------
[VIn]::Key(0x1B, $false); [VIn]::Key(0x1B, $true)
$escClosed = Wait-Hidden 2000
Write-Output "esc closed: $escClosed"

# --- 4. outside click closes ----------------------------------------------
if (-not [Win32]::IsWindowVisible($hwnd)) { [VIn]::AltV() }
$reopened = Wait-Visible 3000
Write-Output "reopened for outside-click test: $reopened"
Start-Sleep -Milliseconds 800
[VIn]::ClickAt(40, 40)   # top-left corner of the primary monitor, outside the popup
$clickClosed = Wait-Hidden 2000
Write-Output "outside click closed: $clickClosed"

# --- summary ----------------------------------------------------------------
Write-Output "RESULT open=$opened escClosed=$escClosed outsideClickClosed=$clickClosed"