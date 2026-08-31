# W35 item 7: cold start — launch to tray-ready on the 10,000-item store.
# Launch via the CDP-capable task (WebView2 149), timestamp the app's own log
# milestones against the launch instant, then confirm the hotkey actually
# works and that a tray icon exists.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\..\scripts\win.psm1')

$log = Join-Path $env:APPDATA 'Rebuffer\logs\rebuffer.log.2026-08-30'

Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 4

$launchUtc = [DateTime]::UtcNow
schtasks /run /tn "rbf-cdp" | Out-Null

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class KE35 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
'@

# poll for the NEW 'hotkey Alt+V registered' line (must be after launch)
$hotLine = $null
for ($i = 0; $i -lt 120 -and -not $hotLine; $i++) {
    Start-Sleep -Milliseconds 500
    $lines = Get-Content $log -ErrorAction SilentlyContinue
    foreach ($ln in $lines) {
        if ($ln -match 'hotkey Alt') {
            $t = [DateTimeOffset]::Parse([regex]::Match($ln, '^\S+').Value).UtcDateTime
            if ($t -ge $launchUtc) { $hotLine = $ln; break }
        }
    }
}
$tsHot = if ($hotLine) { [DateTimeOffset]::Parse([regex]::Match($hotLine, '^\S+').Value).UtcDateTime } else { $null }
if (-not $tsHot) { Write-Output "RESULT coldstart=FAIL hotkey-not-ready-in-60s"; exit 1 }

$startLine = $null
foreach ($ln in (Get-Content $log)) {
    if ($ln -match 'rebuffer starting') {
        $t = [DateTimeOffset]::Parse([regex]::Match($ln, '^\S+').Value).UtcDateTime
        if ($t -ge $launchUtc) { $startLine = $ln }
    }
}
$tsStart = if ($startLine) { [DateTimeOffset]::Parse([regex]::Match($startLine, '^\S+').Value).UtcDateTime } else { $null }

$launchToHot = [math]::Round(($tsHot - $launchUtc).TotalMilliseconds)
$startToHot = if ($tsStart) { [math]::Round(($tsHot - $tsStart).TotalMilliseconds) } else { -1 }
$launchToStart = if ($tsStart) { [math]::Round(($tsStart - $launchUtc).TotalMilliseconds) } else { -1 }
Write-Output "launch_utc=$($launchUtc.ToString('yyyy-MM-ddTHH:mm:ss.fffZ'))"
Write-Output "starting_log=$tsStart"
Write-Output "hotkey_log=$tsHot"
Write-Output "launch_to_starting_ms=$launchToStart"
Write-Output "starting_to_hotkey_ms=$startToHot"
Write-Output "launch_to_hotkey_ms=$launchToHot"

# hotkey actually works?
Start-Sleep -Milliseconds 800
$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "hotkey_works=NO (no popup hwnd)" }
else {
    [KE35]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
    [KE35]::keybd_event(0x56, 0, 0, [UIntPtr]::Zero)
    [KE35]::keybd_event(0x56, 0, 2, [UIntPtr]::Zero)
    [KE35]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    $vis = $false
    for ($i = 0; $i -lt 40 -and -not $vis; $i++) { Start-Sleep -Milliseconds 50; $vis = [KE35]::IsWindowVisible($hwnd) }
    Write-Output "hotkey_works=$vis"
}

# tray icon present? UIA over the Shell_TrayWnd notification toolbar.
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$trayFound = $false
$trayName = ''
try {
    $trayRoot = [System.Windows.Automation.AutomationElement]::RootElement
    $trayCond = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, 'Shell_TrayWnd')
    $tray = $trayRoot.FindFirst([System.Windows.Automation.TreeScope]::Children, $trayCond)
    if ($tray) {
        $all = $tray.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($el in $all) {
            $n = $el.Current.Name
            if ($n -and $n -match 'Rebuffer') { $trayFound = $true; $trayName = $n; break }
        }
    }
} catch { Write-Output "tray_check_error=$($_.Exception.Message)" }
Write-Output "tray_icon_found=$trayFound tooltip=$trayName"