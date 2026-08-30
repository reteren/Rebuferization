# 05b — focused re-verification of the E5 janitor flows. The first 05 run's
# E5 checks raced a Svelte re-render storm (8005-item janitor cleanup in flight
# while the number-input evals ran) and reported FAILs; this run re-proves each
# flow with a gentler policy (retention 30 + item aged 40 days) so the shared
# store is not wiped again, and journals every eval result.
$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null
$journal = Join-Path $shot '05b-reverify-journal.txt'
$snap = Join-Path $PSScriptRoot 'snapshot'
New-Item -ItemType Directory -Path $snap -Force | Out-Null
$testItems = Join-Path $snap 'test-items.txt'
$targets = & node "$PSScriptRoot\cdp.mjs" targets
$s = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
if (-not $s) { throw 'settings target not found' }
$sid = $s.id

function Eval($expr) { (& node "$PSScriptRoot\cdp.mjs" eval $sid $expr) -join "`n" }
function J($line) { $line | Tee-Object -FilePath $journal -Append }
function WaitW { Start-Sleep -Milliseconds 1000 }
function SJson { Get-Content "$root\settings.json" -Raw | ConvertFrom-Json }
function DB($q) { & sqlite3 "$root\rebuffer.db" $q }
function Nav($label) { Eval "(() => { [...document.querySelectorAll('nav button')].find(b => b.textContent.trim() === '$label')?.click(); return 'ok'; })()" | Out-Null; Start-Sleep -Milliseconds 500 }
function InvokeB64($cmd, $json) {
  $b64 = 'b64:' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
  (& node "$PSScriptRoot\cdp.mjs" invoke $sid $cmd $b64) -join "`n"
}
function SetNum($idx, $value) {
  Eval "(() => { const i = [...document.querySelectorAll('.content input[type=number]')][$idx]; if (!i) return 'NO_INPUT'; i.value = '$value'; i.dispatchEvent(new Event('change', {bubbles:true})); return 'set'; })()"
}
function MarkItem($id, $tag) { "$tag`t$id" | Add-Content $testItems }

J "=== 05b reverify start $(Get-Date -Format o) ==="
Nav 'Storage'
Start-Sleep -Milliseconds 800

# R1 retention policy: retention 30 + item aged 40 days must be janitor-killed
$r = SetNum 0 30
J "05b R1 set retention 30 via UI: $r -> file=$((SJson).storage.retentionDays)"
$oldText = "rbf-r1-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
Set-Clipboard -Value $oldText
Start-Sleep -Seconds 2
$oldRow = DB "SELECT id FROM items WHERE preview_text = '$oldText'"
$oldId = $oldRow.Split('|')[0]
MarkItem $oldId 'r1-retention'
J "05b R1 item id=$oldId"
DB "UPDATE items SET created_at = strftime('%s','now','-40 days')*1000 WHERE id = $oldId" | Out-Null
$res = InvokeB64 'run_cleanup_now' '{"olderThanDays":null}'
J "05b R1 janitor (policy) result: $res"
$gone = DB "SELECT COUNT(*) FROM items WHERE id = $oldId"
J "05b R1 aged item still present: $gone (want 0)"
if ([int]$gone -eq 0) { J '05b R1 retention policy reached the janitor PASS' } else { J '05b R1 retention policy reached the janitor FAIL' }

# R2 Clean now button with its own cleanDays input
$cnText = "rbf-r2-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
Set-Clipboard -Value $cnText
Start-Sleep -Seconds 2
$cnRow = DB "SELECT id FROM items WHERE preview_text = '$cnText'"
$cnId = $cnRow.Split('|')[0]
MarkItem $cnId 'r2-cleannow'
DB "UPDATE items SET created_at = strftime('%s','now','-40 days')*1000 WHERE id = $cnId" | Out-Null
$r = SetNum 3 30
J "05b R2 cleanDays 30: $r"
$click = Eval "(() => { const b = [...document.querySelectorAll('.content button')].find(x => x.textContent.trim() === 'Clean now'); if (!b) return 'NO_BTN'; b.click(); return 'clicked'; })()"
J "05b R2 clean now click: $click"
Start-Sleep -Seconds 2
$ok = Eval "document.querySelector('.usage .ok')?.textContent ?? 'NO_OK'"
J "05b R2 feedback: $ok"
$cnGone = DB "SELECT COUNT(*) FROM items WHERE id = $cnId"
J "05b R2 clean-now item still present: $cnGone (want 0)"
if ([int]$cnGone -eq 0 -and $ok -like 'Removed*') { J '05b R2 Clean now button deletes PASS' } else { J '05b R2 Clean now button deletes FAIL' }

# R3 maxStoreBytes cap: cap 1MB, capture 2MB, policy janitor must prune it
$r = SetNum 2 1
Start-Sleep -Milliseconds 500
J "05b R3 set maxStoreBytes 1MB: $r -> file=$((SJson).storage.maxStoreBytes)"
$big = 'x' * (2 * 1024 * 1024)
Set-Clipboard -Value $big
Start-Sleep -Seconds 3
$bigRow = DB "SELECT id, hash FROM items WHERE byte_size >= 2000000 ORDER BY id DESC LIMIT 1"
J "05b R3 2MB item row: $bigRow"
if ($bigRow) {
  $bigId = $bigRow.Split('|')[0]
  MarkItem $bigId 'r3-big'
  $res2 = InvokeB64 'run_cleanup_now' '{"olderThanDays":null}'
  J "05b R3 janitor (with cap) result: $res2"
  $bigGone = DB "SELECT COUNT(*) FROM items WHERE id = $bigId"
  J "05b R3 big item still present: $bigGone (want 0)"
  if ([int]$bigGone -eq 0) { J '05b R3 maxStoreBytes reached the janitor PASS' } else { J '05b R3 maxStoreBytes reached the janitor FAIL' }
} else {
  J '05b R3 2MB item was not captured — FAIL'
}
# restore cap to unlimited via UI
$r = SetNum 2 ''
Start-Sleep -Milliseconds 500
J "05b R3 maxStoreBytes restored: $((SJson).storage.maxStoreBytes)"

J "=== 05b done ==="