# Popup visibility probe: inject Alt+V and record every visible/hidden
# transition on the popup window with millisecond resolution.
# Usage: .\popup_probe.ps1 [-HideFirst] [-HoldMs 0] [-ObsMs 3000]
param(
    [switch]$HideFirst,
    [int]$HoldMs = 0,
    [int]$ObsMs = 3000
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class PP {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    static IntPtr _found;
    public static IntPtr FindByPid(string title, uint pid) {
        _found = IntPtr.Zero;
        EnumWindows((h, l) => {
            if (_found != IntPtr.Zero) return false;
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && GetTitle(h) == title) { _found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return _found;
    }
    public delegate bool EW(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EW cb, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder sb, int max);
    static string GetTitle(IntPtr h) { var sb = new System.Text.StringBuilder(256); GetWindowText(h, sb, 256); return sb.ToString(); }

    public static string Run(IntPtr hwnd, int holdMs, int obsMs) {
        var sb = new System.Text.StringBuilder();
        long t0 = Stopwatch.GetTimestamp();
        keybd_event(0x12, 0, 0, UIntPtr.Zero);
        keybd_event(0x56, 0, 0, UIntPtr.Zero);
        if (holdMs > 0) System.Threading.Thread.Sleep(holdMs);
        keybd_event(0x56, 0, 2, UIntPtr.Zero);
        keybd_event(0x12, 0, 2, UIntPtr.Zero);
        bool prev = IsWindowVisible(hwnd);
        if (prev) sb.AppendLine("t=0 visible=True (already open)");
        int firstVis = -1, lastVis = -1, firstHide = -1;
        long deadline = t0 + Stopwatch.Frequency * obsMs / 1000;
        while (Stopwatch.GetTimestamp() < deadline) {
            bool v = IsWindowVisible(hwnd);
            if (v != prev) {
                int t = (int)((Stopwatch.GetTimestamp() - t0) * 1000.0 / Stopwatch.Frequency);
                sb.AppendLine("t=" + t + "ms visible=" + v);
                if (v) { if (firstVis < 0) firstVis = t; lastVis = t; }
                else if (firstHide < 0) firstHide = t;
                prev = v;
            }
        }
        sb.AppendLine("firstVisible=" + firstVis + " lastVisible=" + lastVis + " firstHide=" + firstHide);
        return sb.ToString();
    }
}
'@

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [PP]::FindByPid('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }
Write-Host "popup hwnd=0x$($hwnd.ToString('X')) pid=$($proc.Id)"
[PP]::Run($hwnd, $HoldMs, $ObsMs)