# Export/import verification at command level (the native file dialogs cannot
# be driven headlessly; the commands behind the buttons are what is tested).
$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null
$journal = Join-Path $shot '07-export-import-journal.txt'
$work = Join-Path $env:TEMP 'rbf-verify'
New-Item -ItemType Directory -Path $work -Force | Out-Null
$archive = Join-Path $work 'export.rbx'
$targets = & node "$PSScriptRoot\cdp.mjs" targets
$s = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
if (-not $s) { throw 'settings target not found' }
$sid = $s.id

function Eval($expr) { (& node "$PSScriptRoot\cdp.mjs" eval $sid $expr) -join "`n" }
function J($line) { $line | Tee-Object -FilePath $journal -Append }
function DB($q) { & sqlite3 "$root\rebuffer.db" $q }

J "=== 07 export/import start $(Get-Date -Format o) ==="

$countBefore = DB "SELECT COUNT(*) FROM items"
$copyCountsBefore = DB "SELECT id, copy_count FROM items ORDER BY id"
J "rows before: $countBefore"

# ---- Export ----
Remove-Item $archive -ErrorAction SilentlyContinue
$r = & node "$PSScriptRoot\cdp.mjs" invoke $sid 'export_data' ("{`"path`":`"$($archive.Replace('\','/'))`"}")
J "export invoke: $r"
Start-Sleep -Seconds 1
if (-not (Test-Path $archive)) { J 'EXPORT archive not produced FAIL'; exit 1 }
$unzip = Join-Path $work 'unzipped'
Remove-Item $unzip -Recurse -ErrorAction SilentlyContinue
& python -c "import zipfile,sys; zipfile.ZipFile(r'$archive').extractall(r'$unzip')"
$entries = (Get-ChildItem $unzip -Recurse -File | ForEach-Object { $_.FullName.Substring($unzip.Length + 1).Replace('\','/') })
$entries | ForEach-Object { J "EXPORT entry: $_" }
$need = @('settings.json','manifest.json','items.jsonl')
foreach ($n in $need) {
  if ($entries -contains $n) { J "EXPORT has $n PASS" } else { J "EXPORT has $n FAIL" }
}
$blobCount = ($entries | Where-Object { $_ -like 'blobs/*' }).Count
J "EXPORT blobs/ entries: $blobCount"
if ($blobCount -gt 0) { J 'EXPORT blobs tree present PASS' } else { J 'EXPORT blobs tree present FAIL' }
$manifest = Get-Content (Join-Path $unzip 'manifest.json') -Raw | ConvertFrom-Json
J "EXPORT manifest: itemCount=$($manifest.itemCount) version=$($manifest.version) format=$($manifest.format)"
$lineCount = (Get-Content (Join-Path $unzip 'items.jsonl') | Where-Object { $_.Trim() }).Count
J "EXPORT items.jsonl lines: $lineCount"
if ($manifest.itemCount -eq $lineCount -and [int]$manifest.itemCount -eq [int]$countBefore) {
  J 'EXPORT manifest/items consistent PASS'
} else { J 'EXPORT manifest/items consistent FAIL' }

# ---- Import merge: everything already present -> skip hashes, bump copy_count ----
$r2 = & node "$PSScriptRoot\cdp.mjs" invoke $sid 'import_data' ("{`"path`":`"$($archive.Replace('\','/'))`",`"mode`":`"merge`"}")
J "import merge invoke: $r2"
Start-Sleep -Seconds 1
$countAfter = DB "SELECT COUNT(*) FROM items"
J "rows after merge import: $countAfter (before $countBefore)"
if ([int]$countAfter -eq [int]$countBefore) { J 'IMPORT merge skips existing hashes PASS' } else { J 'IMPORT merge skips existing hashes FAIL' }
$copyCountsAfter = DB "SELECT id, copy_count FROM items ORDER BY id"
J "copy_count before/after (first 6):"
$ccb = @($copyCountsBefore); $cca = @($copyCountsAfter)
for ($i = 0; $i -lt [Math]::Min(6, $ccb.Count); $i++) {
  J "  before: $($ccb[$i])  after: $($cca[$i])"
}

# ---- Import replace: wipe + re-import ----
$r3 = & node "$PSScriptRoot\cdp.mjs" invoke $sid 'import_data' ("{`"path`":`"$($archive.Replace('\','/'))`",`"mode`":`"replace`"}")
J "import replace invoke: $r3"
Start-Sleep -Seconds 1
$countReplace = DB "SELECT COUNT(*) FROM items"
J "rows after replace import: $countReplace"
if ([int]$countReplace -eq [int]$countBefore) { J 'IMPORT replace restores full history PASS' } else { J 'IMPORT replace restores full history FAIL' }
$integrity = DB "PRAGMA integrity_check"
J "db integrity after import: $integrity"
if ($integrity -match '^ok') { J 'IMPORT integrity PASS' } else { J 'IMPORT integrity FAIL' }

J "=== 07 done ==="