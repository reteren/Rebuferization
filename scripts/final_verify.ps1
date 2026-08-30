# Final popup verification pass: Images tab thumbnails + search-filtered
# variety cards (color, code, long text, link, file). Runs against ONE app
# instance it starts itself.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Fin {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint LD = 0x0002, LU = 0x0004;
    public static void AltV() { keybd_event(0x12,0,0,UIntPtr.Zero); keybd_event(0x56,0,0,UIntPtr.Zero); keybd_event(0x56,0,KEYEVENTF_KEYUP,UIntPtr.Zero); keybd_event(0x12,0,KEYEVENTF_KEYUP,UIntPtr.Zero); }
    public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(LD,0,0,0,UIntPtr.Zero); mouse_event(LU,0,0,0,UIntPtr.Zero); }
    public static void KeyChar(char ch) {
        keybd_event((byte)ch, 0, 0, UIntPtr.Zero); keybd_event((byte)ch, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
    public static void CtrlA() { keybd_event(0x11,0,0,UIntPtr.Zero); keybd_event(0x41,0,0,UIntPtr.Zero); keybd_event(0x41,0,KEYEVENTF_KEYUP,UIntPtr.Zero); keybd_event(0x11,0,KEYEVENTF_KEYUP,UIntPtr.Zero); }
}
'@

# --- start ONE instance ----------------------------------------------------
Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
Start-Process -FilePath (Join-Path $PSScriptRoot '..\src-tauri\target\debug\rebuffer.exe')
Start-Sleep -Seconds 7
$proc = Get-Process rebuffer -ErrorAction SilentlyContinue | Sort-Object StartTime | Select-Object -Last 1
if (-not $proc) { throw 'app did not start' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }
Write-Output "app pid=$($proc.Id) hwnd=0x$($hwnd.ToString('X'))"

function Open-Popup {
    if (-not [Win32]::IsWindowVisible($hwnd)) { [Fin]::AltV() }
    for ($i = 0; $i -lt 80 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    return [Win32]::IsWindowVisible($hwnd)
}
function Capture-Now([string]$name) {
    $r = New-Object Win32+RECT
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot "$name.png"
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return ,@($r, $w, $h, $out)
}
function Find-TextCols($bmp, [int]$y0, [int]$y1, [int]$xFrom, [int]$xTo) {
    $cols = @()
    for ($x = $xFrom; $x -lt $xTo; $x += 1) {
        $n = 0
        for ($y = $y0; $y -lt $y1; $y++) {
            $c = $bmp.GetPixel($x, $y)
            $lum = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            if ($lum -gt 100) { $n++ }
        }
        if ($n -ge 2) { $cols += $x }
    }
    # cluster
    $clusters = @()
    if ($cols.Count) {
        $s = $cols[0]; $p = $cols[0]
        foreach ($x in $cols[1..($cols.Count - 1)]) {
            if ($x - $p -gt 3) { $clusters += ,@($s, $p); $s = $x }
            $p = $x
        }
        $clusters += ,@($s, $p)
    }
    return ,$clusters
}

if (-not (Open-Popup)) { throw 'popup did not open' }
Start-Sleep -Milliseconds 2200
$cap = Capture-Now 'final_main'
$r = $cap[0]; $w = $cap[1]; $h = $cap[2]
Write-Output "captured ${w}x${h} -> $($cap[3])"

$bmp = [System.Drawing.Bitmap]::new($cap[3])
# tabs band: scan rows 40..95 for text clusters
$tabClusters = Find-TextCols $bmp 45 90 0 ($w - 1)
Write-Output ("tab-row text clusters (x0-x1): {0}" -f (($tabClusters | ForEach-Object { "$($_[0])-$($_[1])" }) -join ', '))
$bmp.Dispose()