# Scrolls the popup grid with the mouse wheel and captures + OCRs the result.
param(
    [int]$Notches = 6,
    [string]$Name = 'popup_scrolled',
    [int]$OcrScale = 4,
    [switch]$NoOcr
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class InputS {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    public const uint MOUSEEVENTF_WHEEL = 0x0800;
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public static void Wheel(int delta) {
        mouse_event(MOUSEEVENTF_WHEEL, 0, 0, (uint)delta, UIntPtr.Zero);
    }
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
if (-not [Win32]::IsWindowVisible($hwnd)) {
    [InputS]::AltV()
    for ($i = 0; $i -lt 40 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    Start-Sleep -Seconds 2
}

$rect = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$rect)
$cx = [int](($rect.Left + $rect.Right) / 2)
$cy = [int]($rect.Top + ($rect.Bottom - $rect.Top) * 0.45)
[InputS]::SetCursorPos($cx, $cy)
Start-Sleep -Milliseconds 200
for ($n = 0; $n -lt $Notches; $n++) {
    [InputS]::Wheel(-120)
    Start-Sleep -Milliseconds 60
}
Start-Sleep -Milliseconds 600

if (-not [Win32]::IsWindowVisible($hwnd)) {
    [InputS]::AltV()
    for ($i = 0; $i -lt 40 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    Start-Sleep -Seconds 1
}

$null = [Win32]::GetWindowRect($hwnd, [ref]$rect)

$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
$out = Join-Path $PSScriptRoot "$Name.png"
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$null = [Win32]::PrintWindow($hwnd, $hdc, [Win32]::PW_RENDERFULLCONTENT)
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
Write-Output "saved $out"
$bmp.Dispose()

if (-not $NoOcr) {
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'ocr.ps1') -Path $out | Out-String | Write-Output
}