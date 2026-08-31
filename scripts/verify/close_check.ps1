# W35 item 3: close/copy behaviors verified against the REAL window.
# For each action: open the popup via Alt+V, dispatch the interaction via CDP,
# then check IsWindowVisible and (for copies) the clipboard.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '..\..\scripts\win.psm1')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class KE35b {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
'@
$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)

function Open-Popup {
    if (-not [KE35b]::IsWindowVisible($hwnd)) {
        [KE35b]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [KE35b]::keybd_event(0x56, 0, 0, [UIntPtr]::Zero)
        [KE35b]::keybd_event(0x56, 0, 2, [UIntPtr]::Zero); [KE35b]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
    }
    for ($i = 0; $i -lt 60 -and -not [KE35b]::IsWindowVisible($hwnd); $i++) { Start-Sleep -Milliseconds 50 }
    Start-Sleep -Milliseconds 1200
    return [KE35b]::IsWindowVisible($hwnd)
}

function Run-Action([string]$a) {
    $out = node (Join-Path $PSScriptRoot 'act_once.mjs') $a 2>$null | Select-Object -Last 1
    Start-Sleep -Milliseconds 1000
    $closed = -not [KE35b]::IsWindowVisible($hwnd)
    return [pscustomobject]@{ Action = $a; Output = $out; Closed = $closed }
}

# 1. plain click copies and closes (empty selection)
$null = Open-Popup
$r = Run-Action 'clickcopy'
$cb = (Get-Clipboard -Raw -ErrorAction SilentlyContinue) ?? ''
$expect = ($r.Output | ConvertFrom-Json).text
Write-Output "plain-click: closed=$($r.Closed) clipboardMatches=$($cb.StartsWith($expect.Substring(0,20))) expected='$($expect.Substring(0,30))...'"

# 2. Enter copies the focused item and closes
$null = Open-Popup
$r2 = Run-Action 'enter'
$cb2 = (Get-Clipboard -Raw -ErrorAction SilentlyContinue) ?? ''
Write-Output "enter: closed=$($r2.Closed) clipboardLen=$($cb2.Length)"

# 3. Esc closes
$null = Open-Popup
$r3 = Run-Action 'esc'
Write-Output "esc: closed=$($r3.Closed)"