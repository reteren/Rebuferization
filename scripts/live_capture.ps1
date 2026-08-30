# Captures the popup's on-screen content DURING the brief window it is open,
# and tests how long it stays open with a non-browser previous foreground.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class VP4 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr FindWindowW(string cls, string title);
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public static void AltV() {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
        keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
}
'@

# Bring an Explorer (non-browser) window to the foreground first.
$shell = [VP4]::FindWindowW('Progman', $null)
if ($shell -ne [IntPtr]::Zero) { $null = [VP4]::SetForegroundWindow($shell) }
Start-Sleep -Milliseconds 800

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
$r = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$r)
$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top

[VP4]::AltV()
$t0 = [DateTime]::UtcNow
for ($i = 0; $i -lt 60 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }

$shots = @()
for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep -Milliseconds 200
    $el = (([DateTime]::UtcNow) - $t0).TotalMilliseconds
    $vis = [Win32]::IsWindowVisible($hwnd)
    if ($vis -and $shots.Count -lt 2) {
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
        $g.Dispose()
        $out = Join-Path $PSScriptRoot ("popup_live_{0}.png" -f $shots.Count)
        $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        $shots += $out
    }
    if (-not $vis) {
        Write-Output ("popup closed at +{0:N0} ms (previous foreground: explorer)" -f $el)
        break
    }
    if ($i -eq 29) { Write-Output "popup still open after 6s" }
}
foreach ($s in $shots) { Write-Output "captured $s" }
if (-not [Win32]::IsWindowVisible($hwnd)) { [VP4]::AltV() }