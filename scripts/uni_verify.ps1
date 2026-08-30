# Types search queries via SendInput KEYEVENTF_UNICODE (layout-independent)
# and captures the filtered grid.
param([string[]]$Queries = @('3D8BFD', 'lorem', 'schema', 'github'))
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Uni {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public InputUnion U; }
    [StructLayout(LayoutKind.Explicit)] public struct InputUnion {
        [FieldOffset(0)] public KEYBDINPUT ki;
    }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT {
        public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo;
    }
    public const uint INPUT_KEYBOARD = 1;
    public const uint KEYEVENTF_UNICODE = 0x0004;
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const uint LD = 0x0002, LU = 0x0004;
    static void SendKey(ushort scan, bool up) {
        var inp = new INPUT { type = INPUT_KEYBOARD };
        inp.U.ki = new KEYBDINPUT { wVk = 0, wScan = scan, dwFlags = (up ? KEYEVENTF_UNICODE | KEYEVENTF_KEYUP : KEYEVENTF_UNICODE), dwExtraInfo = IntPtr.Zero };
        SendInput(1, new[] { inp }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void AltV() {
        var alt = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x12 } } };
        SendInput(1, new[] { alt }, Marshal.SizeOf(typeof(INPUT)));
        var v = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x56 } } };
        SendInput(1, new[] { v }, Marshal.SizeOf(typeof(INPUT)));
        var vu = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x56, dwFlags = KEYEVENTF_KEYUP } } };
        SendInput(1, new[] { vu }, Marshal.SizeOf(typeof(INPUT)));
        var alu = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x12, dwFlags = KEYEVENTF_KEYUP } } };
        SendInput(1, new[] { alu }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static bool IsFg(IntPtr h) { return GetForegroundWindow() == h; }
    public static void Click(int x, int y) { SetCursorPos(x, y); mouse_event(LD, 0, 0, 0, UIntPtr.Zero); mouse_event(LU, 0, 0, 0, UIntPtr.Zero); }
    public static void Type(string s) {
        foreach (char ch in s) {
            SendKey(ch, false);
            SendKey(ch, true);
        }
    }
    public static void CtrlA() {
        var c = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x11 } } };
        SendInput(1, new[] { c }, Marshal.SizeOf(typeof(INPUT)));
        var a = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x41 } } };
        SendInput(1, new[] { a }, Marshal.SizeOf(typeof(INPUT)));
        var au = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x41, dwFlags = KEYEVENTF_KEYUP } } };
        SendInput(1, new[] { au }, Marshal.SizeOf(typeof(INPUT)));
        var cu = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x11, dwFlags = KEYEVENTF_KEYUP } } };
        SendInput(1, new[] { cu }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void Backspace() {
        var b = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x08 } } };
        SendInput(1, new[] { b }, Marshal.SizeOf(typeof(INPUT)));
        var bu = new INPUT { type = INPUT_KEYBOARD, U = new InputUnion { ki = new KEYBDINPUT { wVk = 0x08, dwFlags = KEYEVENTF_KEYUP } } };
        SendInput(1, new[] { bu }, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if (-not [Win32]::IsWindowVisible($hwnd)) { [Uni]::AltV() }
for ($i = 0; $i -lt 80 -and -not [Win32]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
for ($i = 0; $i -lt 40 -and -not [Uni]::IsFg($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
Start-Sleep -Milliseconds 2200
Write-Output "popup open=$([Win32]::IsWindowVisible($hwnd)) fg=$([Uni]::IsFg($hwnd))"

$r = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$r)
[Uni]::Click($r.Left + 150, $r.Top + 24)
Start-Sleep -Milliseconds 600
Write-Output "after click fg=$([Uni]::IsFg($hwnd))"

foreach ($q in $Queries) {
    [Uni]::CtrlA(); [Uni]::Backspace()
    Start-Sleep -Milliseconds 250
    [Uni]::Type($q)
    Start-Sleep -Milliseconds 2500
    $null = [Win32]::GetWindowRect($hwnd, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g.Dispose()
    $out = Join-Path $PSScriptRoot ("u_{0}.png" -f $q)
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "captured $out"
}
if ([Uni]::IsFg($hwnd)) { [Uni]::CtrlA(); [Uni]::Backspace() }