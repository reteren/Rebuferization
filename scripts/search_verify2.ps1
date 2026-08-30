# Safe search-filtered variety verification: opens the popup, verifies it is
# the foreground window before EVERY key injection, captures, and verifies the
# capture is actually the popup (dark background) before accepting it.
param([string[]]$Queries = @('3D8BFD', 'lorem', 'schema', 'github', 'DECISIONS'))
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class S2 {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint LD = 0x0002, LU = 0x0004;
    public static void AltV() { keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, 0, UIntPtr.Zero); keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static bool IsFg(IntPtr h) { return GetForegroundWindow() == h; }
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
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup hwnd not found' }

function Ensure-PopupOpen {
    for ($try = 0; $try -lt 6; $try++) {
        if (-not [Win32]::IsWindowVisible($hwnd)) { [S2]::AltV() }
        for ($i = 0; $i -lt 80 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
        if ([Win32]::IsWindowVisible($hwnd)) {
            for ($i = 0; $i -lt 40 -and -not [S2]::IsFg($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
            if ([S2]::IsFg($hwnd)) { return $true }
            Write-Output "popup visible but NOT foreground (try $try); closing it"
            [S2]::AltV(); Start-Sleep -Milliseconds 500
        } else {
            Write-Output "popup not visible (try $try)"
        }
        Start-Sleep -Milliseconds 800
    }
    return $false
}

$r = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$r)

function Capture-Check([string]$name) {
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot "$name.png"
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    # verify dark popup background: sample a grid of points
    $dark = 0; $n = 0
    for ($y = 0; $y -lt $h; $y += 40) {
        for ($x = 0; $x -lt $w; $x += 40) {
            $c = $bmp.GetPixel($x, $y)
            if ((0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B) -lt 70) { $dark++ }
            $n++
        }
    }
    $bmp.Dispose()
    $pct = 100.0 * $dark / $n
    Write-Output "capture $name dark-pct=$([int]$pct)"
    return ,@($out, $pct)
}

if (-not (Ensure-PopupOpen)) { throw 'could not get popup open and focused' }
Start-Sleep -Milliseconds 2000

# click the search input
function Click-Search {
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    [S2]::Click($r.Left + 150, $r.Top + 24)
    Start-Sleep -Milliseconds 500
    return [S2]::IsFg($hwnd)
}

if (-not (Click-Search)) { throw 'popup lost foreground after search click' }

foreach ($q in $Queries) {
    if (-not [S2]::IsFg($hwnd)) {
        Write-Output "query '$q': popup not foreground, retrying open..."
        if (-not (Ensure-PopupOpen)) { Write-Output "query '$q' SKIPPED: cannot regain focus"; continue }
        Start-Sleep -Milliseconds 1200
        $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
        if (-not (Click-Search)) { Write-Output "query '$q' SKIPPED: lost fg after re-click"; continue }
    }
    [S2]::CtrlA(); [S2]::Backspace()
    Start-Sleep -Milliseconds 300
    if (-not [S2]::IsFg($hwnd)) { Write-Output "query '$q' SKIPPED: lost fg"; continue }
    [S2]::Type($q)
    Start-Sleep -Milliseconds 2000
    $cap = Capture-Check ("search_{0}" -f $q)
    if ($cap[1] -gt 60) {
        Write-Output "== query '$q': popup capture OK =="
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'ocr.ps1') -Path $cap[0] -Scale 2 2>&1 | Select-Object -First 8 | ForEach-Object { Write-Output $_ }
    } else {
        Write-Output "== query '$q': capture NOT the popup (dark=$($cap[1])) - skipped =="
    }
}

if ([S2]::IsFg($hwnd)) { [S2]::CtrlA(); [S2]::Backspace() }