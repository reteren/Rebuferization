# Stress test: change EVERY setting in one session, restart, verify nothing was
# lost. Then hand-edit settings.json to invalid values and verify clamping.
$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null
$journal = Join-Path $shot '08-stress-journal.txt'
$snap = Join-Path $PSScriptRoot 'snapshot'
New-Item -ItemType Directory -Path $snap -Force | Out-Null
$exe = (Resolve-Path (Join-Path $PSScriptRoot '..\..\src-tauri\target\debug\rebuffer.exe')).Path
$testItems = Join-Path $snap 'test-items.txt'

function Eval($tid, $expr) { (& node "$PSScriptRoot\cdp.mjs" eval $tid $expr) -join "`n" }
function J($line) { $line | Tee-Object -FilePath $journal -Append }
function WaitW { Start-Sleep -Milliseconds 900 }
function SJson { Get-Content "$root\settings.json" -Raw | ConvertFrom-Json }
function Nav($tid, $label) { Eval $tid "(() => { [...document.querySelectorAll('nav button')].find(b => b.textContent.trim() === '$label')?.click(); return 'ok'; })()" | Out-Null; Start-Sleep -Milliseconds 400 }
function DB($q) { & sqlite3 "$root\rebuffer.db" $q }
function Targets { & node "$PSScriptRoot\cdp.mjs" targets }
function WaitTargets {
  $deadline = (Get-Date).AddSeconds(40)
  while ((Get-Date) -lt $deadline) {
    try { $t = Targets; if ($LASTEXITCODE -eq 0 -and $t) { return $t } } catch {}
    Start-Sleep -Seconds 1
  }
  throw 'no CDP targets'
}
function Launch {
  $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
  Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe)
}
function KillApp { Get-Process -Name rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 2 }

J "=== 08 stress start $(Get-Date -Format o) ==="
$itemsBefore = DB "SELECT COUNT(*) FROM items"

# ---- Phase A: change every setting through the UI ----
$t = WaitTargets
$s = ($t | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
$sid = $s.id
$itemsBefore = DB "SELECT COUNT(*) FROM items"

# General
Nav $sid 'General'
Eval $sid "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'rec'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval $sid "window.dispatchEvent(new KeyboardEvent('keydown', {key:'K', code:'KeyK', ctrlKey:true, altKey:true, shiftKey:true, bubbles:true, cancelable:true}))" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Aggressive mode')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Launch on startup')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Start silently')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Auto-paste after copy')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Paste as plain text by default')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Close the popup after copying')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
# capture: off then on (exercise both)
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Clipboard capture')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Clipboard capture')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW

# Storage
Nav $sid 'Storage'
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][0]; i.value = '5'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][1]; i.value = '16'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][2]; i.value = '512'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Notify when the store nears its cap')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW

# Appearance
Nav $sid 'Appearance'
Eval $sid "(() => { const r = [...document.querySelectorAll('.content input[type=radio]')].find(x => x.parentElement.textContent.includes('Fixed size')); r.click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const i = document.querySelector('input[aria-label=\"Fixed width\"]'); i.value = '900'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Eval $sid "(() => { const i = document.querySelector('input[aria-label=\"Fixed height\"]'); i.value = '600'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')].find(x => x.closest('.field')?.textContent.includes('Grid zoom')); i.value = '5'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Show relative age')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const sel = document.querySelector('.content select'); sel.value = 'large'; sel.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Animate GIF')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Reduce motion')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const i = document.querySelector('.content input[type=color]'); i.value = '#00ff88'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW

# Privacy
Nav $sid 'Privacy'
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Respect apps')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
Eval $sid "(() => { const i = document.querySelector('.content input[type=text]'); i.value = 'stressapp.exe'; i.dispatchEvent(new Event('input', {bubbles:true})); [...document.querySelectorAll('.content button')].find(x => x.textContent.trim() === 'Add').click(); return 'added'; })()" | Out-Null
WaitW

& node "$PSScriptRoot\cdp.mjs" shot $sid "$shot\08-stress-all-changed.png"
$f = SJson
J "STRESS final settings.json:"
$f | ConvertTo-Json -Depth 5 | ForEach-Object { J "STRESS   $_" }

# ---- restart and verify persistence ----
KillApp
Launch
$t2 = WaitTargets
$s2 = ($t2 | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
$sid2 = $s2.id
Start-Sleep -Seconds 3
$f2 = SJson
J "STRESS after restart settings.json:"
$f2 | ConvertTo-Json -Depth 5 | ForEach-Object { J "STRESS   $_" }
$match = ($f2.hotkey.binding -eq 'Ctrl+Alt+Shift+K') -and ($f2.behavior.launchOnStartup -eq $false) -and ($f2.behavior.silentStart -eq $false) -and ($f2.storage.retentionDays -eq 5) -and ($f2.window.sizeMode -eq 'fixed') -and ($f2.window.fixed.width -eq 900) -and ($f2.appearance.accent -eq '#00ff88') -and ($f2.privacy.blockedProcesses -contains 'stressapp.exe')
J "STRESS all settings persisted: $match"
if ($match) { J 'STRESS persistence PASS' } else { J 'STRESS persistence FAIL' }
$itemsAfter = DB "SELECT COUNT(*) FROM items"
J "STRESS items before/after restart: $itemsBefore / $itemsAfter"
if ([int]$itemsBefore -eq [int]$itemsAfter) { J 'STRESS item count stable PASS' } else { J 'STRESS item count stable FAIL' }
$int = DB "PRAGMA integrity_check"
J "STRESS db integrity: $int"
$hotkeyLine = Get-Content (Get-ChildItem "$root\logs\rebuffer.log.*" | Sort-Object LastWriteTime | Select-Object -Last 1).FullName -Tail 30 | Select-String 'registered'
J "STRESS hotkey after restart: $($hotkeyLine | Select-Object -Last 1)"
$ui = Eval $sid2 "(() => { const h = document.querySelector('.content h2')?.textContent; const box = document.querySelector('.hotkey-box')?.textContent.trim(); return 'h2=' + h + ' hotkey=' + box; })()"
J "STRESS UI after restart: $ui"

# ---- Phase B: invalid hand-edit of settings.json ----
KillApp
# B1: valid JSON, out-of-range values -> must clamp on load
$bad = Get-Content "$root\settings.json" -Raw | ConvertFrom-Json
$bad.storage.retentionDays = 999
$bad.appearance.accent = 'not-a-colour'
$bad.appearance.formatLabelSize = 'huge'
$bad.window.zoomStep = 99
$bad.window.percentOfMonitor = 999
$bad | ConvertTo-Json -Depth 6 | Set-Content "$root\settings.json" -Encoding utf8
J "STRESS B1 wrote invalid-but-valid-JSON settings"
Launch
$t3 = WaitTargets
$s3 = ($t3 | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
Start-Sleep -Seconds 3
$r = & node "$PSScriptRoot\cdp.mjs" invoke $s3.id 'get_settings' '{}'
J "STRESS B1 get_settings: $r"
$parsed = $r | ConvertFrom-Json
if ($parsed.storage.retentionDays -eq 30 -and $parsed.appearance.accent -eq '#7aa2ff' -and $parsed.appearance.formatLabelSize -eq 'medium' -and $parsed.window.zoomStep -eq 5 -and $parsed.window.percentOfMonitor -eq 100) {
  J 'STRESS B1 clamps rather than refuses to start PASS'
} else { J 'STRESS B1 clamps rather than refuses to start FAIL' }
KillApp

# B2: malformed JSON tail -> must fall back to defaults, still start
$good = Get-Content (Join-Path $snap 'settings.before.json') -Raw
Set-Content "$root\settings.json" ($good + "`n} } not-json-tail") -Encoding utf8 -NoNewline
J 'STRESS B2 wrote malformed JSON tail'
Launch
$t4 = WaitTargets
$s4 = ($t4 | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
Start-Sleep -Seconds 3
$r2 = & node "$PSScriptRoot\cdp.mjs" invoke $s4.id 'get_settings' '{}'
J "STRESS B2 get_settings: $r2"
$logWarn = Get-Content (Get-ChildItem "$root\logs\rebuffer.log.*" | Sort-Object LastWriteTime | Select-Object -Last 1).FullName -Tail 40 | Select-String 'not valid JSON|unreadable'
J "STRESS B2 log: $($logWarn | Select-Object -Last 1)"
if ($r2 -match '"version":1' -and $logWarn) { J 'STRESS B2 malformed JSON -> defaults, app starts PASS' } else { J 'STRESS B2 malformed JSON -> defaults, app starts FAIL' }
KillApp

# restore the good snapshot so the app is usable again
Copy-Item (Join-Path $snap 'settings.before.json') "$root\settings.json" -Force
J 'STRESS settings.json restored from snapshot'
J "=== 08 stress done ==="