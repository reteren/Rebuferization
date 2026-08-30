# Effect tests: does a changed setting change live behaviour?
#   - hotkey: UI change -> log shows re-registration -> real chord press opens popup
#   - captureEnabled: UI toggle -> real clipboard copy lands / does not land in the DB
#   - accent: UI change -> popup computed --accent
#   - launchOnStartup: UI toggle -> HKCU Run value appears / disappears
#   - retentionDays/maxStoreBytes: UI change -> janitor policy (run_cleanup_now with null)
#   - storage meter: legend numbers vs sqlite reality
$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null
$journal = Join-Path $shot '05-effect-journal.txt'
$snap = Join-Path $PSScriptRoot 'snapshot'
New-Item -ItemType Directory -Path $snap -Force | Out-Null
$testItems = Join-Path $snap 'test-items.txt'
$targets = & node "$PSScriptRoot\cdp.mjs" targets
$s = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
$p = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*index.html*' } | Select-Object -First 1)
if (-not $s -or -not $p) { throw 'targets not found' }
$sid = $s.id; $pid = $p.id

function Eval($tid, $expr) { (& node "$PSScriptRoot\cdp.mjs" eval $tid $expr) -join "`n" }
function J($line) { $line | Tee-Object -FilePath $journal -Append }
function WaitW { Start-Sleep -Milliseconds 900 }
function SJson { Get-Content "$root\settings.json" -Raw | ConvertFrom-Json }
function Nav($label) { Eval $sid "(() => { [...document.querySelectorAll('nav button')].find(b => b.textContent.trim() === '$label')?.click(); return 'ok'; })()" | Out-Null; Start-Sleep -Milliseconds 400 }
function DB($q) { & sqlite3 "$root\rebuffer.db" $q }
function LogTail { (Get-Content (Get-ChildItem "$root\logs\rebuffer.log.*" | Sort-Object LastWriteTime | Select-Object -Last 1).FullName -Tail 200) }
function MarkItem($id, $tag) { "$tag`t$id" | Add-Content $testItems }
function ClickToggleCapture {
  Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Clipboard capture')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
}
function ClickToggle($label) {
  Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('$label')); if (!l) return 'NO_TOGGLE'; l.querySelector('input').click(); return 'clicked'; })()"
}

J "=== 05 effect tests start $(Get-Date -Format o) ==="

# ---- E1 hotkey: re-register + real press ----
Nav 'General'
Eval $sid "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'rec'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval $sid "window.dispatchEvent(new KeyboardEvent('keydown', {key:'Z', code:'KeyZ', ctrlKey:true, altKey:true, shiftKey:true, bubbles:true, cancelable:true}))" | Out-Null
WaitW
$f = SJson
J ("EFFECT hotkey binding now: {0} {1}" -f $f.hotkey.binding, $(if ($f.hotkey.binding -eq 'Ctrl+Alt+Shift+Z') {'PASS'} else {'FAIL'}))
$reg = LogTail | Select-String 'hotkey .* registered' | Select-Object -Last 1
J "EFFECT log re-registration: $reg"
if ($reg -match 'Ctrl\+Alt\+Shift\+Z registered') { J 'EFFECT hotkey re-register log PASS' } else { J 'EFFECT hotkey re-register log FAIL' }
# real press of the new chord
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Ctrl+Alt+Shift+Z'
Start-Sleep -Seconds 2
$fired = LogTail | Select-String 'hotkey fired' | Select-Object -Last 1
J "EFFECT log after real press: $fired"
$vis = Eval $pid "document.visibilityState"
J "EFFECT popup visibility after real chord: $vis"
if ($fired -and $vis -eq 'visible') { J 'EFFECT real chord -> popup PASS' } else { J 'EFFECT real chord -> popup FAIL' }
if ($vis -eq 'visible') { & "$PSScriptRoot\sendkeys.ps1" -Chord 'Ctrl+Alt+Shift+Z'; Start-Sleep -Seconds 1 }
# restore Alt+V
Eval $sid "(() => { [...document.querySelectorAll('.content button')].find(b => b.textContent.trim() === 'Record').click(); return 'rec'; })()" | Out-Null
Start-Sleep -Milliseconds 300
Eval $sid "window.dispatchEvent(new KeyboardEvent('keydown', {key:'V', code:'KeyV', altKey:true, bubbles:true, cancelable:true}))" | Out-Null
WaitW
$reg2 = LogTail | Select-String 'hotkey .* registered' | Select-Object -Last 1
J "EFFECT hotkey restored: $reg2"

# ---- E2 captureEnabled ----
Nav 'General'
# capture OFF
ClickToggleCapture
WaitW
$off = (SJson).behavior.captureEnabled
J ("EFFECT captureEnabled off: {0} {1}" -f $off, $(if (-not $off) {'PASS'} else {'FAIL'}))
$offText = "rbf-capture-off-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
Set-Clipboard -Value $offText
Start-Sleep -Seconds 2
$found = DB "SELECT COUNT(*) FROM items WHERE preview_text = '$offText'"
J "EFFECT row landed while disabled: $found (want 0)"
if ([int]$found -eq 0) { J 'EFFECT capture disabled blocks copies PASS' } else { J 'EFFECT capture disabled blocks copies FAIL' }
# capture ON
ClickToggleCapture
WaitW
$on = (SJson).behavior.captureEnabled
J ("EFFECT captureEnabled on: {0} {1}" -f $on, $(if ($on) {'PASS'} else {'FAIL'}))
$onText = "rbf-capture-on-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
Set-Clipboard -Value $onText
Start-Sleep -Seconds 2
$row = DB "SELECT id, hash FROM items WHERE preview_text = '$onText'"
J "EFFECT row while enabled: $row"
if ($row) { J 'EFFECT capture enabled captures PASS'; MarkItem ($row.Split('|')[0]) 'capture-on' } else { J 'EFFECT capture enabled captures FAIL' }

# ---- E3 accent -> popup ----
Nav 'Appearance'
Eval $sid "(() => { const i = document.querySelector('.content input[type=color]'); i.value = '#ff00ff'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
J ("EFFECT accent in settings.json: {0}" -f (SJson).appearance.accent)
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Alt+V'
Start-Sleep -Seconds 2
$accent = Eval $pid "getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || 'UNSET'"
$addBtn = Eval $pid "getComputedStyle(document.querySelector('.add-btn')).backgroundColor"
$tabs = Eval $pid "(() => { const c = document.querySelector('.tabs .count'); return c ? getComputedStyle(c).color : 'no-count'; })()"
J "EFFECT popup computed --accent: $accent (want #ff00ff)"
J "EFFECT popup add-btn background: $addBtn (want rgb(255, 0, 255))"
if ($accent -eq '#ff00ff') { J 'EFFECT accent recolours popup PASS' } else { J 'EFFECT accent recolours popup FAIL — no live wiring found' }
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Alt+V'; Start-Sleep -Milliseconds 800
Eval $sid "(() => { const i = document.querySelector('.content input[type=color]'); i.value = '#7aa2ff'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW

# ---- E4 launchOnStartup <-> HKCU Run ----
function RunKey {
  (Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Rebuffer' -ErrorAction SilentlyContinue).Rebuffer
}
$before = RunKey
J "EFFECT Run key before: '$before'"
Nav 'General'
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Launch on startup')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
$afterOff = RunKey
J "EFFECT Run key after toggle off: '$afterOff'"
if (-not $afterOff) { J 'EFFECT launchOnStartup off removes Run key PASS' } else { J 'EFFECT launchOnStartup off removes Run key FAIL' }
Eval $sid "(() => { const l = [...document.querySelectorAll('label.toggle')].find(x => x.textContent.includes('Launch on startup')); l.querySelector('input').click(); return 'ok'; })()" | Out-Null
WaitW
$afterOn = RunKey
J "EFFECT Run key after toggle on: '$afterOn'"
if ($afterOn -match 'rebuffer') { J 'EFFECT launchOnStartup on restores Run key PASS' } else { J 'EFFECT launchOnStartup on restores Run key FAIL' }

# ---- E5 retentionDays + Clean now button + maxStoreBytes reach the janitor ----
Nav 'Storage'
# set retention 1 via UI
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][0]; i.value = '1'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
J ("EFFECT retentionDays set: {0}" -f (SJson).storage.retentionDays)
# age a fresh item 3 days so a 1-day retention kills it
$oldText = "rbf-retention-old-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
Set-Clipboard -Value $oldText
Start-Sleep -Seconds 2
$oldRow = DB "SELECT id, hash FROM items WHERE preview_text = '$oldText'"
J "EFFECT old-item row: $oldRow"
if ($oldRow) {
  $oldId = $oldRow.Split('|')[0]
  MarkItem $oldId 'retention-old'
  DB "UPDATE items SET created_at = strftime('%s','now','-3 days')*1000 WHERE id = $oldId" | Out-Null
  # invoke janitor with null days -> must use the policy (1 day) pushed from the UI change
  $res = & node "$PSScriptRoot\cdp.mjs" invoke $sid 'run_cleanup_now' '{"olderThanDays":null}'
  J "EFFECT janitor run (policy days) result: $res"
  $gone = DB "SELECT COUNT(*) FROM items WHERE id = $oldId"
  if ([int]$gone -eq 0) { J 'EFFECT retentionDays reached the janitor PASS' } else { J 'EFFECT retentionDays reached the janitor FAIL' }
}
# Clean now button path (uses its own cleanDays input)
$cleanDaysText = "rbf-cleannow-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
Set-Clipboard -Value $cleanDaysText
Start-Sleep -Seconds 2
$cnRow = DB "SELECT id FROM items WHERE preview_text = '$cleanDaysText'"
$cnId = $cnRow.Split('|')[0]
MarkItem $cnId 'cleannow'
DB "UPDATE items SET created_at = strftime('%s','now','-40 days')*1000 WHERE id = $cnId" | Out-Null
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][3]; i.value = '30'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
Start-Sleep -Milliseconds 400
Eval $sid "(() => { const b = [...document.querySelectorAll('.content button')].find(x => x.textContent.trim() === 'Clean now'); b.click(); return 'ok'; })()" | Out-Null
Start-Sleep -Seconds 2
$okMsg = Eval $sid "document.querySelector('.usage .ok')?.textContent ?? 'NO_MSG'"
J "EFFECT Clean now feedback: $okMsg"
$cnGone = DB "SELECT COUNT(*) FROM items WHERE id = $cnId"
if ([int]$cnGone -eq 0) { J 'EFFECT Clean now button deletes PASS' } else { J 'EFFECT Clean now button deletes FAIL' }
# maxStoreBytes: cap 1MB, insert a 2MB text item, janitor (null) must prune it
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][2]; i.value = '1'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
J ("EFFECT maxStoreBytes set: {0}" -f (SJson).storage.maxStoreBytes)
$big = 'x' * (2 * 1024 * 1024)
Set-Clipboard -Value $big
Start-Sleep -Seconds 3
$bigRow = DB "SELECT id, hash FROM items WHERE byte_size >= 2000000 ORDER BY id DESC LIMIT 1"
J "EFFECT 2MB item row: $bigRow"
if ($bigRow) {
  $bigId = $bigRow.Split('|')[0]
  MarkItem $bigId 'big-cap'
  $res2 = & node "$PSScriptRoot\cdp.mjs" invoke $sid 'run_cleanup_now' '{"olderThanDays":null}'
  J "EFFECT janitor run (with cap) result: $res2"
  $bigGone = DB "SELECT COUNT(*) FROM items WHERE id = $bigId"
  if ([int]$bigGone -eq 0) { J 'EFFECT maxStoreBytes reached the janitor PASS' } else { J 'EFFECT maxStoreBytes reached the janitor FAIL' }
}
# restore cap to unlimited via UI (empty input)
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][2]; i.value = ''; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
J ("EFFECT maxStoreBytes restored: {0}" -f (SJson).storage.maxStoreBytes)
# restore retention 30
Eval $sid "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][0]; i.value = '30'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()" | Out-Null
WaitW
J ("EFFECT retentionDays restored: {0}" -f (SJson).storage.retentionDays)

# ---- E6 storage meter vs sqlite ----
# add a file reference item for the meter
$filePath = (Resolve-Path 'docs\CONTRACTS.md').Path
Set-Clipboard -Path $filePath
Start-Sleep -Seconds 2
$fileRow = DB "SELECT id, kind FROM items WHERE is_reference = 1 ORDER BY id DESC LIMIT 1"
J "EFFECT file reference row: $fileRow"
if ($fileRow) { MarkItem ($fileRow.Split('|')[0]) 'file-ref' }
# image via STA clipboard
$img = & pwsh -NoProfile -STA -Command "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; `$b = New-Object System.Drawing.Bitmap(160,100); `$g = [System.Drawing.Graphics]::FromImage(`$b); `$g.Clear([System.Drawing.Color]::FromArgb(40,80,200)); `$g.Dispose(); [System.Windows.Forms.Clipboard]::SetImage(`$b); 'OK'" 2>&1
J "EFFECT image clipboard: $img"
Start-Sleep -Seconds 3
$imgRow = DB "SELECT id, kind FROM items WHERE kind = 'image' ORDER BY id DESC LIMIT 1"
J "EFFECT image row: $imgRow"
if ($imgRow) { MarkItem ($imgRow.Split('|')[0]) 'image' }
# refresh stats (the settings page fetches on load; force by re-navigating section)
Nav 'Storage'; Start-Sleep -Milliseconds 500
$legend = Eval $sid "[...document.querySelectorAll('.usage .legend li')].map(li => li.textContent.trim()).join(' | ')"
$usageHead = Eval $sid "document.querySelector('.usage-head')?.textContent.trim()"
J "EFFECT meter head: $usageHead"
J "EFFECT meter legend: $legend"
$truth = DB "SELECT kind || ':' || COUNT(*) || ':' || COALESCE(SUM(byte_size),0) FROM items GROUP BY kind ORDER BY kind"
J "EFFECT sqlite truth:"
$truth | ForEach-Object { J "EFFECT   $_" }

J "=== 05 effect tests done ==="