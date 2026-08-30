# 06 — the missing link between effects (05) and export/import (07).
# Answers the parts of the brief that no other script exercises:
#   A. Backend hotkey refusal: update_settings with a chord Windows owns must
#      fail at rebind and roll the hotkey section back, leaving settings.json
#      showing the OLD binding (commands.rs validates the chord before
#      persisting). The frontend blocks Win+<letter> before the backend, so
#      this is only reachable by invoking the command directly.
#   B. Aggressive mode claims the same chord via the LL hook and it registers.
#   C. Window size settings take effect on the REAL popup window: switch to
#      fixed size, press the real hotkey, read GetWindowRect natively.
#   D. External edit of settings.json hot-reloads in the backend (SPEC 7:
#      "hot-reloaded on change") and what the UI does with it.
$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null
$journal = Join-Path $shot '06-hardened-journal.txt'
$targets = & node "$PSScriptRoot\cdp.mjs" targets
$s = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
$p = ($targets | ConvertFrom-Json | Where-Object { $_.url -notlike '*settings.html*' } | Select-Object -First 1)
if (-not $s -or -not $p) { throw 'targets not found' }
$sid = $s.id; $pupid = $p.id

function Eval($tid, $expr) { (& node "$PSScriptRoot\cdp.mjs" eval $tid $expr) -join "`n" }
function Invoke($tid, $cmd, $json) {
  $b64 = 'b64:' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
  (& node "$PSScriptRoot\cdp.mjs" invoke $tid $cmd $b64) -join "`n"
}
function J($line) { $line | Tee-Object -FilePath $journal -Append }
function WaitW { Start-Sleep -Milliseconds 900 }
function SJson { Get-Content "$root\settings.json" -Raw | ConvertFrom-Json }
function LogTail { (Get-Content (Get-ChildItem "$root\logs\rebuffer.log.*" | Sort-Object LastWriteTime | Select-Object -Last 1).FullName -Tail 100) }
function Nav($label) { Eval $sid "(() => { [...document.querySelectorAll('nav button')].find(b => b.textContent.trim() === '$label')?.click(); return 'ok'; })()" | Out-Null; Start-Sleep -Milliseconds 400 }
function Check($name, $cond, $detail) {
  $ok = [bool]$cond
  J ("06 {0} -> {1} {2}" -f $name, $(if ($ok) {'PASS'} else {'FAIL'}), $detail)
  return $ok
}
function PopupHwnd {
  # FindWindowW is unreliable in this shell environment; EnumWindows works.
  if (-not ('F6' -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class F6 {
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int m);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [StructLayout(LayoutKind.Sequential)]
  public struct R { public int L, T, Rt, B; }
}
"@
  }
  $targetPid = (Get-Process -Name rebuffer -ErrorAction SilentlyContinue | Select-Object -First 1).Id
  $h = [IntPtr]::Zero
  $cb = [F6+EnumProc]{
    param($h2, $l)
    $p = 0
    [F6]::GetWindowThreadProcessId($h2, [ref]$p) | Out-Null
    if ($p -eq $targetPid) {
      $sb = New-Object System.Text.StringBuilder 128
      [F6]::GetWindowTextW($h2, $sb, 128) | Out-Null
      if ($sb.ToString() -eq 'Rebuffer') { $script:h = $h2 }
    }
    return $true
  }
  [F6]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
  if ($script:h -eq [IntPtr]::Zero) { J '06 C popup window not found via EnumWindows'; return ,@(0, 0, 0) }
  $r = New-Object F6+R
  [F6]::GetWindowRect($script:h, [ref]$r) | Out-Null
  ,@($script:h, ($r.Rt - $r.L), ($r.B - $r.T))
}

J "=== 06 hardened tests start $(Get-Date -Format o) ==="
$binding0 = (SJson).hotkey.binding
J "06 baseline hotkey binding: $binding0"
Nav 'General'

# ---- A. backend refusal + rollback ----
$res = Invoke $sid 'update_settings' '{"patch":{"hotkey":{"binding":"Win+V"}}}'
J "06 A invoke Win+V (aggressive off): $res"
$after = (SJson).hotkey.binding
J "06 A settings.json binding after refused: $after"
Check 'A refused Win+V leaves old binding in file' ($after -eq $binding0) "file=$after want=$binding0"
$uiBox = Eval $sid "document.querySelector('.hotkey-box')?.textContent.trim()"
J "06 A hotkey box after refused: $uiBox"
Check 'A UI hotkey box unchanged after refusal' ($uiBox -eq $binding0) "ui=$uiBox want=$binding0"

# ---- B. aggressive mode claims Win+V via the LL hook ----
$res2 = Invoke $sid 'update_settings' '{"patch":{"hotkey":{"binding":"Win+V","aggressiveMode":true}}}'
J "06 B invoke Win+V aggressive: $res2"
$f = SJson
J "06 B settings after aggressive: binding=$($f.hotkey.binding) aggressive=$($f.hotkey.aggressiveMode)"
$reg = LogTail | Select-String 'hotkey .* registered' | Select-Object -Last 1
J "06 B log: $reg (note: rebind itself never logs; only startup registrations do)"
Check 'B aggressive Win+V registered' ($f.hotkey.binding -eq 'Win+V' -and $f.hotkey.aggressiveMode -eq $true) "file=$($f.hotkey.binding)/$($f.hotkey.aggressiveMode)"
# real Win+V press must fire OUR hook, not Windows clipboard history.
# The tracing non_blocking writer flushes with a long, variable delay, so poll
# for the line instead of reading once.
$logPath = (Get-ChildItem "$root\logs\rebuffer.log.*" | Sort-Object LastWriteTime | Select-Object -Last 1).FullName
$beforeCount = (Get-Content $logPath).Count
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Win+V'
$deadline = (Get-Date).AddSeconds(60)
$fired = $null
while ((Get-Date) -lt $deadline -and -not $fired) {
  Start-Sleep -Seconds 4
  $fired = (Get-Content $logPath | Select-Object -Skip $beforeCount) | Select-String 'hotkey fired' | Select-Object -Last 1
}
$vis = Eval $pupid "document.visibilityState"
J "06 B after real Win+V: fired=$fired popupVis=$vis"
Check 'B real Win+V fires our hook' ([bool]$fired) "fired=$fired"
# hide the popup again if it is showing
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Win+V'
Start-Sleep -Milliseconds 800
# restore Alt+V, aggressive off, through the real UI
Eval $sid "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'rec'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval $sid "window.dispatchEvent(new KeyboardEvent('keydown', {key:'V', code:'KeyV', altKey:true, bubbles:true, cancelable:true}))" | Out-Null
WaitW
$f = SJson
$ag = Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Aggressive mode')); if (l.querySelector('input').checked) { l.querySelector('input').click(); return 'toggled-off'; } return 'already-off'; })()"
WaitW
$f2 = SJson
J "06 B restore: binding=$($f.hotkey.binding) aggressive=$($f2.hotkey.aggressiveMode)"
Check 'B restored to Alt+V non-aggressive' ($f.hotkey.binding -eq 'Alt+V' -and $f2.hotkey.aggressiveMode -eq $false) "binding=$($f.hotkey.binding) agg=$($f2.hotkey.aggressiveMode)"

# ---- C. window size settings apply to the real popup ----
Nav 'Appearance'
# percent baseline
Eval $sid "(() => { const r = [...document.querySelectorAll('.content input[type=radio]')].find(x => x.parentElement.textContent.includes('Percent of monitor')); r.click(); return 'ok'; })()" | Out-Null
WaitW
$fixed = $false
$fixed = $null -ne (Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')].find(x => x.closest('.field')?.textContent.includes('Percent of monitor')); i.value = '50'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()")
Start-Sleep -Milliseconds 300
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Alt+V'
Start-Sleep -Seconds 1
$base = PopupHwnd
J "06 C popup rect at percent/50: w=$($base[1]) h=$($base[2])"
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Alt+V'
Start-Sleep -Milliseconds 600
# fixed mode
Eval $sid "(() => { const r = [...document.querySelectorAll('.content input[type=radio]')].find(x => x.parentElement.textContent.includes('Fixed size')); r.click(); return 'ok'; })()" | Out-Null
Start-Sleep -Milliseconds 400
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input')].find(x => x.getAttribute('aria-label') === 'Fixed width'); i.value = '760'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input')].find(x => x.getAttribute('aria-label') === 'Fixed height'); i.value = '520'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
Start-Sleep -Milliseconds 400
$f = SJson
J "06 C settings: sizeMode=$($f.window.sizeMode) fixed=$($f.window.fixed.width)x$($f.window.fixed.height)"
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Alt+V'
Start-Sleep -Seconds 1
$fx = PopupHwnd
J "06 C popup rect at fixed 760x520: w=$($fx[1]) h=$($fx[2])"
& node "$PSScriptRoot\cdp.mjs" shot $pupid "$shot\06-popup-fixed-size.png" | Out-Null
# DPI-agnostic check: fixed size must differ from percent size and be near 760x520
$tol = 40
$nearW = [Math]::Abs($fx[1] - 760) -le $tol
$nearH = [Math]::Abs($fx[2] - 520) -le $tol
$differs = $fx[1] -ne $base[1] -or $fx[2] -ne $base[2]
Check 'C fixed size applied to real popup window' ($nearW -and $nearH -and $differs) "w=$($fx[1]) h=$($fx[2]) vs 760x520, base was $($base[1])x$($base[2])"
# restore percent + 40
Eval $sid "(() => { const r = [...document.querySelectorAll('.content input[type=radio]')].find(x => x.parentElement.textContent.includes('Percent of monitor')); r.click(); return 'ok'; })()" | Out-Null
Start-Sleep -Milliseconds 400
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')].find(x => x.closest('.field')?.textContent.includes('Percent of monitor')); i.value = '40'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW

# ---- D. external edit hot-reloads in the backend (SPEC 7) ----
$ret0 = (SJson).storage.retentionDays
$raw = Get-Content "$root\settings.json" -Raw | ConvertFrom-Json
$raw.storage.retentionDays = 12
$raw | ConvertTo-Json -Depth 6 | Set-Content "$root\settings.json" -Encoding utf8
J "06 D hand-edited retentionDays -> 12"
Start-Sleep -Seconds 4
$gs = Invoke $sid 'get_settings' '{}'
$parsed = $gs | ConvertFrom-Json
J "06 D backend get_settings retentionDays: $($parsed.storage.retentionDays)"
Check 'D backend hot-reloads external edit' ($parsed.storage.retentionDays -eq 12) "got=$($parsed.storage.retentionDays)"
$uiVal = Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][0]; return i.value; })()"
J "06 D UI retention input after external edit: $uiVal"
if ($uiVal -eq '12') { J '06 D UI reflects external edit PASS (event or reload reached the frontend)' }
else { J '06 D UI does NOT reflect external edit until a patch/reload — frontend only re-syncs on settings-changed events' }
# restore through the UI (this also emits settings-changed and re-syncs the store)
Nav 'Storage'
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][0]; i.value = '$ret0'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
J "06 D retention restored to $((SJson).storage.retentionDays)"

J "=== 06 done ==="