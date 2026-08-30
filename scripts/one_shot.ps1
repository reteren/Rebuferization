# One-shot: open popup (retry loop, keybd_event Alt+V), verify foreground,
# Unicode-type each query, capture, verify dark, report. Self-contained.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class One {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
    [StructLayout(LayoutKind.Explicit)] public struct InputUnion { [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    public const uint INPUT_KEYBOARD = 1;
    public const uint KEYEVENTF_UNICODE = 0x0004;
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint LD = 0x0002, LU = 0x0004;
    public static void AltV() {
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
        keybd_event(0x12, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
    public static bool IsFg(IntPtr h) { return GetForegroundWindow() == h; }
    public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(LD, 0, 0, 0, UIntPtr.Zero); mouse_event(LU, 0, 0, 0, UIntPtr.Zero); }
    static void UniKey(char ch, bool up) {
        var inp = new INPUT { type = INPUT_KEYBOARD };
        inp.U.ki = new KEYBDINPUT { wScan = ch, dwFlags = up ? KEYEVENTF_UNICODE | KEYEVENTF_KEYUP : KEYEVENTF_UNICODE };
        SendInput(1, new[] { inp }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void Type(string s) { foreach (char ch in s) { UniKey(ch, false); UniKey(ch, true); } }
    public static void CtrlA() {
        keybd_event(0x11, 0, 0, UIntPtr.Zero); keybd_event(0x41, 0, 0, UIntPtr.Zero);
        keybd_event(0x41, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); keybd_event(0x11, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
    public static void Backspace() {
        keybd_event(0x08, 0, 0, UIntPtr.Zero); keybd_event(0x08, 0, KEYEVENTF_KEYUP, UIntPtr.Zero);
    }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
Write-Output "pid=$($proc.Id) hwnd=0x$($hwnd.ToString('X'))"

$ok = $false
for ($try = 0; $try -lt 8 -and -not $ok; $try++) {
    if (-not [Win32]::IsWindowVisible($hwnd)) { [One]::AltV() }
    for ($i = 0; $i -lt 80 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    for ($i = 0; $i -lt 40 -and -not [One]::IsFg($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    $ok = [Win32]::IsWindowVisible($hwnd) -and [One]::IsFg($hwnd)
    if (-not $ok) {
        Write-Output ("try {0}: vis={1} fg={2}; if visible, closing and retrying" -f $try, [Win32]::IsWindowVisible($hwnd), [One]::IsFg($hwnd))
        if ([Win32]::IsWindowVisible($hwnd)) { [One]::AltV() }
        Start-Sleep -Milliseconds 800
    }
}
if (-not $ok) { throw 'could not open popup with focus' }
Write-Output 'popup open + focused'
Start-Sleep -Milliseconds 2000

$r = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$r)
[One]::Click($r.Left + 150, $r.Top + 24)
Start-Sleep -Milliseconds 600
if (-not [One]::IsFg($hwnd)) { Write-Output 'WARN: lost fg after search click' }

foreach ($q in @('3D8BFD', 'lorem', 'schema', 'github')) {
    if (-not [One]::IsFg($hwnd)) { Write-Output "SKIP $q (no fg)"; continue }
    [One]::CtrlA(); [One]::Backspace()
    Start-Sleep -Milliseconds 250
    [One]::Type($q)
    Start-Sleep -Milliseconds 2500
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot ("final_{0}.png" -f $q)
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "captured final_$q.png"
}
if ([One]::IsFg($hwnd)) { [One]::CtrlA(); [One]::Backspace() }
Write-Output COMPLETE