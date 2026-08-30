# Search-filters the grid to each variety item, captures, and OCRs it.
param([string[]]$Queries = @('3D8BFD', 'lorem', 'schema', 'github', 'DECISIONS'))
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Srch {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint LD = 0x0002, LU = 0x0004;
    public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(LD, 0, 0, 0, UIntPtr.Zero); mouse_event(LU, 0, 0, 0, UIntPtr.Zero); }
    public static void Type(string s) {
        foreach (char ch in s) {
            byte vk = (byte)char.ToUpperInvariant(ch);
            keybd_event(vk, 0, 0, UIntPtr.Zero);
            keybd_event(vk, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
        }
    }
    public static void CtrlA() { keybd_event(0x11, 0, 0, UIntPtr.Zero); keybd_event(0x41, 0, 0, UIntPtr.Zero); keybd_event(0x41, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); keybd_event(0x11, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static void Backspace() { keybd_event(0x08, 0, 0, UIntPtr.Zero); keybd_event(0x08, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static void Open() { keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if (-not [Win32]::IsWindowVisible($hwnd)) { [Srch]::Open() }
for ($i = 0; $i -lt 80 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
if (-not [Win32]::IsWindowVisible($hwnd)) { throw 'popup not visible' }
Start-Sleep -Milliseconds 2000

$r = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$r)

function Capture-Now([string]$name) {
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot "$name.png"
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return $out
}

# click the search input (left side of toolbar)
[Srch]::Click($r.Left + 150, $r.Top + 24)
Start-Sleep -Milliseconds 400

foreach ($q in $Queries) {
    [Srch]::CtrlA(); [Srch]::Backspace()
    Start-Sleep -Milliseconds 300
    [Srch]::Type($q)
    Start-Sleep -Milliseconds 1800
    $out = Capture-Now ("search_{0}" -f $q)
    Write-Output "== query '$q' captured: $out =="
    $ocr = powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'ocr.ps1') -Path $out -Scale 2 2>&1 | Select-Object -First 8
    $ocr | ForEach-Object { Write-Output $_ }
}

# clear the search box at the end
[Srch]::CtrlA(); [Srch]::Backspace()