# For every settings control: drive it through the real DOM, then read
# %APPDATA%\Rebuffer\settings.json and confirm the field changed.
# Appends verdicts to shots/04-write-journal.txt.
$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null
$journal = Join-Path $shot '04-write-journal.txt'
$targets = & node "$PSScriptRoot\cdp.mjs" targets
$s = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
if (-not $s) { throw 'settings target not found' }
$sid = $s.id

function Eval($expr) { (& node "$PSScriptRoot\cdp.mjs" eval $sid $expr) -join "`n" }
function J($line) { $line | Tee-Object -FilePath $journal -Append }
function WaitW { Start-Sleep -Milliseconds 900 }
function SJson { Get-Content "$root\settings.json" -Raw | ConvertFrom-Json }
function Nav($label) { Eval "(() => { [...document.querySelectorAll('nav button')].find(b => b.textContent.trim() === '$label')?.click(); return 'ok'; })()" | Out-Null; Start-Sleep -Milliseconds 400 }
function Check($name, $actual, $expected) {
  $ok = ($actual -eq $expected)
  J ("WRITE {0} -> expected={1} actual={2} {3}" -f $name, $expected, $actual, $(if ($ok) {'PASS'} else {'FAIL'}))
  return $ok
}
function ClickToggle($label) {
  Eval "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('$label')); if (!l) return 'NO_TOGGLE'; l.querySelector('input').click(); return 'clicked'; })()"
}
function SetNumber($idx, $value) {
  Eval "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][$idx]; if (!i) return 'NO_INPUT'; i.value = '$value'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()"
}

J "=== 04 write tests start $(Get-Date -Format o) ==="

# ---- General ----
Nav 'General'

# G1 hotkey: record Alt+Shift+Z
Eval "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'recording'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'Z', code:'KeyZ', altKey:true, shiftKey:true, bubbles:true, cancelable:true}))" | Out-Null
WaitW
$f = SJson
Check 'hotkey.binding record Alt+Shift+Z' $f.hotkey.binding 'Alt+Shift+Z'
# restore to Alt+V
Eval "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'recording'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'V', code:'KeyV', altKey:true, bubbles:true, cancelable:true}))" | Out-Null
WaitW
$f = SJson
Check 'hotkey.binding restore Alt+V' $f.hotkey.binding 'Alt+V'

# G1b reserved chord Win+V -> inline explanation, no write
Eval "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'recording'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'v', code:'KeyV', metaKey:true, bubbles:true, cancelable:true}))" | Out-Null
Start-Sleep -Milliseconds 500
$err = Eval "document.querySelector('.content .error')?.textContent ?? 'NO_ERROR'"
J "WRITE hotkey reserved Win+V inline msg: $err"
Check 'hotkey.binding unchanged after reserved' (SJson).hotkey.binding 'Alt+V'
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'Escape', code:'Escape', bubbles:true, cancelable:true}))" | Out-Null

# G1c bare key warning
Eval "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'recording'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'p', code:'KeyP', bubbles:true, cancelable:true}))" | Out-Null
Start-Sleep -Milliseconds 500
$warn = Eval "document.querySelector('.content .hint')?.textContent ?? 'NO_HINT'"
J "WRITE hotkey bare-key warning: $warn"
$f = SJson
Check 'hotkey.binding bare P recorded' $f.hotkey.binding 'P'
Eval "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'recording'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'V', code:'KeyV', altKey:true, bubbles:true, cancelable:true}))" | Out-Null
WaitW
Check 'hotkey.binding restored again' (SJson).hotkey.binding 'Alt+V'

# G1d unusable key
Eval "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'recording'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'Tab', code:'Tab', bubbles:true, cancelable:true}))" | Out-Null
Start-Sleep -Milliseconds 500
$err2 = Eval "document.querySelector('.content .error')?.textContent ?? 'NO_ERROR'"
J "WRITE hotkey unusable-key inline msg: $err2"
Eval "window.dispatchEvent(new KeyboardEvent('keydown', {key:'Escape', code:'Escape', bubbles:true, cancelable:true}))" | Out-Null

# G2 aggressiveMode
ClickToggle 'Aggressive mode' | Out-Null
WaitW
Check 'hotkey.aggressiveMode on' (SJson).hotkey.aggressiveMode $true
ClickToggle 'Aggressive mode' | Out-Null
WaitW
Check 'hotkey.aggressiveMode off' (SJson).hotkey.aggressiveMode $false

# G3 launchOnStartup off/on
ClickToggle 'Launch on startup' | Out-Null
WaitW
Check 'behavior.launchOnStartup off' (SJson).behavior.launchOnStartup $false
ClickToggle 'Launch on startup' | Out-Null
WaitW
Check 'behavior.launchOnStartup on' (SJson).behavior.launchOnStartup $true

# G4 silentStart off/on
ClickToggle 'Start silently' | Out-Null
WaitW
Check 'behavior.silentStart off' (SJson).behavior.silentStart $false
ClickToggle 'Start silently' | Out-Null
WaitW
Check 'behavior.silentStart on' (SJson).behavior.silentStart $true

# G5 captureEnabled off/on
ClickToggle 'Clipboard capture' | Out-Null
WaitW
Check 'behavior.captureEnabled off' (SJson).behavior.captureEnabled $false
ClickToggle 'Clipboard capture' | Out-Null
WaitW
Check 'behavior.captureEnabled on' (SJson).behavior.captureEnabled $true

# G6 autoPaste
ClickToggle 'Auto-paste after copy' | Out-Null
WaitW
Check 'behavior.autoPaste on' (SJson).behavior.autoPaste $true
ClickToggle 'Auto-paste after copy' | Out-Null
WaitW
Check 'behavior.autoPaste off' (SJson).behavior.autoPaste $false

# G7 pasteAsPlainText
ClickToggle 'Paste as plain text by default' | Out-Null
WaitW
Check 'behavior.pasteAsPlainText on' (SJson).behavior.pasteAsPlainText $true
ClickToggle 'Paste as plain text by default' | Out-Null
WaitW
Check 'behavior.pasteAsPlainText off' (SJson).behavior.pasteAsPlainText $false

# G8 closeOnCopy
ClickToggle 'Close the popup after copying' | Out-Null
WaitW
Check 'behavior.closeOnCopy off' (SJson).behavior.closeOnCopy $false
ClickToggle 'Close the popup after copying' | Out-Null
WaitW
Check 'behavior.closeOnCopy on' (SJson).behavior.closeOnCopy $true

# ---- Storage ----
Nav 'Storage'
# S1 retentionDays 7
SetNumber 0 7 | Out-Null
WaitW
Check 'storage.retentionDays 7' (SJson).storage.retentionDays 7
# S2 maxItemBytes 32
SetNumber 1 32 | Out-Null
WaitW
Check 'storage.maxItemBytes 32MB' (SJson).storage.maxItemBytes (32 * 1024 * 1024)
# S3 maxStoreBytes 64
SetNumber 2 64 | Out-Null
WaitW
Check 'storage.maxStoreBytes 64MB' (SJson).storage.maxStoreBytes (64 * 1024 * 1024)
# S4 notifyWhenFull
ClickToggle 'Notify when the store nears its cap' | Out-Null
WaitW
Check 'storage.notifyWhenFull off' (SJson).storage.notifyWhenFull $false
ClickToggle 'Notify when the store nears its cap' | Out-Null
WaitW
Check 'storage.notifyWhenFull on' (SJson).storage.notifyWhenFull $true

# ---- Appearance ----
Nav 'Appearance'
# A1 sizeMode fixed + fixed size
Eval "(() => { const r = [...document.querySelectorAll('.content input[type=radio]')].find(x => x.parentElement.textContent.includes('Fixed size')); r.click(); return 'ok'; })()" | Out-Null
WaitW
Check 'window.sizeMode fixed' (SJson).window.sizeMode 'fixed'
$fw = Eval "(() => { const i = document.querySelector('input[aria-label=\"Fixed width\"]'); i.value = '800'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Check 'window.fixed.width 800' (SJson).window.fixed.width 800
# A2 back to percent + percentOfMonitor
Eval "(() => { const r = [...document.querySelectorAll('.content input[type=radio]')].find(x => x.parentElement.textContent.includes('Percent of monitor')); r.click(); return 'ok'; })()" | Out-Null
WaitW
Check 'window.sizeMode percent' (SJson).window.sizeMode 'percent'
$pv = Eval "(() => { const i = [...document.querySelectorAll('.content input[type=number]')].find(x => x.closest('.field')?.textContent.includes('Percent of monitor')); i.value = '60'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Check 'window.percentOfMonitor 60' (SJson).window.percentOfMonitor 60
# A3 zoomStep 4
$zv = Eval "(() => { const i = [...document.querySelectorAll('.content input[type=number]')].find(x => x.closest('.field')?.textContent.includes('Grid zoom')); i.value = '4'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Check 'window.zoomStep 4' (SJson).window.zoomStep 4
# A4 showAge
ClickToggle 'Show relative age on cards' | Out-Null
WaitW
Check 'appearance.showAge off' (SJson).appearance.showAge $false
ClickToggle 'Show relative age on cards' | Out-Null
WaitW
Check 'appearance.showAge on' (SJson).appearance.showAge $true
# A5 formatLabelSize
Eval "(() => { const sel = document.querySelector('.content select'); sel.value = 'large'; sel.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Check 'appearance.formatLabelSize large' (SJson).appearance.formatLabelSize 'large'
Eval "(() => { const sel = document.querySelector('.content select'); sel.value = 'medium'; sel.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Check 'appearance.formatLabelSize medium' (SJson).appearance.formatLabelSize 'medium'
# A6 animateGifs
ClickToggle 'Animate GIF previews' | Out-Null
WaitW
Check 'appearance.animateGifs off' (SJson).appearance.animateGifs $false
ClickToggle 'Animate GIF previews' | Out-Null
WaitW
Check 'appearance.animateGifs on' (SJson).appearance.animateGifs $true
# A7 reduceMotion
ClickToggle 'Reduce motion' | Out-Null
WaitW
Check 'appearance.reduceMotion on' (SJson).appearance.reduceMotion $true
ClickToggle 'Reduce motion' | Out-Null
WaitW
Check 'appearance.reduceMotion off' (SJson).appearance.reduceMotion $false
# A8 accent
Eval "(() => { const i = document.querySelector('.content input[type=color]'); i.value = '#ff00ff'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Check 'appearance.accent #ff00ff' (SJson).appearance.accent '#ff00ff'
Eval "(() => { const i = document.querySelector('.content input[type=color]'); i.value = '#7aa2ff'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
Check 'appearance.accent restored' (SJson).appearance.accent '#7aa2ff'

# ---- Privacy ----
Nav 'Privacy'
ClickToggle 'Respect apps' | Out-Null
WaitW
Check 'privacy.respectClipboardFlags off' (SJson).privacy.respectClipboardFlags $false
ClickToggle 'Respect apps' | Out-Null
WaitW
Check 'privacy.respectClipboardFlags on' (SJson).privacy.respectClipboardFlags $true
# blockedProcesses add
Eval "(() => { const i = document.querySelector('.content input[type=text]'); i.value = 'testproc.exe'; i.dispatchEvent(new Event('input', {bubbles:true})); const b = [...document.querySelectorAll('.content button')].find(x => x.textContent.trim() === 'Add'); b.click(); return 'added'; })()" | Out-Null
WaitW
$bp = (SJson).privacy.blockedProcesses
Check 'privacy.blockedProcesses +testproc.exe' ($bp -contains 'testproc.exe') $true
# remove it
Eval "(() => { const b = document.querySelector('button[aria-label=\"Remove testproc.exe\"]'); b.click(); return 'removed'; })()" | Out-Null
WaitW
$bp = (SJson).privacy.blockedProcesses
Check 'privacy.blockedProcesses -testproc.exe' ($bp -contains 'testproc.exe') $false

J "=== 04 write tests done ==="