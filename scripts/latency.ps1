# Hotkey -> window-visible latency for Rebuffer.
#
# Method (reproducible): with the app running and the popup hidden, a C# probe
# thread records a high-resolution timestamp (Stopwatch.GetTimestamp), injects
# Alt+V via SendInput (the same chord the physical hotkey uses; RegisterHotKey
# fires on injected input), then busy-polls IsWindowVisible(hwnd) on a separate
# thread until the flag flips. Delta = hotkey-sent -> WS_VISIBLE set. The popup
# is toggled closed between samples with a second Alt+V so every sample starts
# from the same hidden state.
#
# Output: per-sample ms, median, mean, worst. Exit code 0.
param(
    [int]$Samples = 15,
    [int]$Warmup = 1
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Diagnostics;

public static class HotkeyProbe {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr hWnd);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint nInputs, INPUT[] inputs, int cbSize);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out long value, int size);

    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public InputUnion U; }
    [StructLayout(LayoutKind.Explicit)]
    public struct InputUnion {
        [FieldOffset(0)] public KEYBDINPUT ki;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT {
        public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo;
    }
    public const uint INPUT_KEYBOARD = 1;
    public const uint KEYEVENTF_KEYUP = 0x0002;
    public const ushort VK_MENU = 0x12;
    public const ushort VK_V = 0x56;
    const int DWMWA_LAST_PRESENT_TIME = 12;

    static void SendKey(ushort vk, bool up) {
        INPUT[] inp = new INPUT[1];
        inp[0].type = INPUT_KEYBOARD;
        inp[0].U.ki.wVk = vk;
        if (up) inp[0].U.ki.dwFlags = KEYEVENTF_KEYUP;
        SendInput(1, inp, Marshal.SizeOf(typeof(INPUT)));
    }

    public static void SendAltV() {
        SendKey(VK_MENU, false);
        SendKey(VK_V, false);
        SendKey(VK_V, true);
        SendKey(VK_MENU, true);
    }

    public static long LastPresentTicks(IntPtr hwnd) {
        long v; if (DwmGetWindowAttribute(hwnd, DWMWA_LAST_PRESENT_TIME, out v, 8) == 0) return v;
        return 0;
    }

    // returns ms latency, or -1 on timeout, -2 if already visible
    public static double MeasureOnce(IntPtr hwnd) {
        if (IsWindowVisible(hwnd)) return -2;
        long t0 = Stopwatch.GetTimestamp();
        SendAltV();
        long deadline = t0 + Stopwatch.Frequency * 2;
        while (Stopwatch.GetTimestamp() < deadline) {
            if (IsWindowVisible(hwnd)) {
                long t1 = Stopwatch.GetTimestamp();
                return (t1 - t0) * 1000.0 / Stopwatch.Frequency;
            }
        }
        return -1;
    }

    public static bool WaitHidden(IntPtr hwnd, int timeoutMs) {
        long deadline = Stopwatch.GetTimestamp() + Stopwatch.Frequency * timeoutMs / 1000;
        while (Stopwatch.GetTimestamp() < deadline) {
            if (!IsWindowVisible(hwnd)) return true;
            System.Threading.Thread.Sleep(5);
        }
        return !IsWindowVisible(hwnd);
    }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running; start it first' }
$pidOf = $proc.Id
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $pidOf)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window (title "Rebuffer") not found' }
Write-Host "popup hwnd: 0x$($hwnd.ToString('X'))"

if (-not [HotkeyProbe]::WaitHidden($hwnd, 3000)) {
    [HotkeyProbe]::SendAltV()
    if (-not [HotkeyProbe]::WaitHidden($hwnd, 3000)) { throw 'cannot get popup into hidden state' }
}

$results = New-Object System.Collections.Generic.List[double]
for ($n = 0; $n -lt $Warmup; $n++) {
    $ms = [HotkeyProbe]::MeasureOnce($hwnd)
    if ($ms -lt 0) { throw "warmup sample failed: $ms" }
    [HotkeyProbe]::SendAltV()
    $null = [HotkeyProbe]::WaitHidden($hwnd, 3000)
}

for ($n = 1; $n -le $Samples; $n++) {
    $ms = [HotkeyProbe]::MeasureOnce($hwnd)
    if ($ms -lt 0) { Write-Host "sample $n failed (code $ms)"; continue }
    $results.Add($ms)
    [HotkeyProbe]::SendAltV()
    $null = [HotkeyProbe]::WaitHidden($hwnd, 3000)
    Write-Host ("sample {0,2}: {1,8:N3} ms" -f $n, $ms)
}

if ($results.Count -eq 0) { throw 'no samples collected' }
$sorted = $results | Sort-Object
$median = if ($sorted.Count % 2 -eq 1) { $sorted[[Math]::Floor($sorted.Count / 2)] } else { ($sorted[$sorted.Count/2 - 1] + $sorted[$sorted.Count/2]) / 2 }
$worst = ($sorted | Select-Object -Last 1)
$best = ($sorted | Select-Object -First 1)
$mean = ($results | Measure-Object -Average).Average
Write-Host "----"
Write-Host ("samples : {0}" -f $results.Count)
Write-Host ("best    : {0:N3} ms" -f $best)
Write-Host ("median  : {0:N3} ms" -f $median)
Write-Host ("mean    : {0:N3} ms" -f $mean)
Write-Host ("worst   : {0:N3} ms" -f $worst)
Write-Host ("target  : < 80 ms -> {0}" -f $(if ($worst -lt 80) { 'PASS' } else { 'MISS' }))