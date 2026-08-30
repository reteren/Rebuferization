# Seeds the store with items at controlled ages so the popup shows a real
# grid: Today / Yesterday / older groups, and the 14m/3h/2d/1h age badges.
# RUN ONLY WHILE REBUFFER.EXE IS STOPPED. Idempotent: re-running updates the
# same rows in place instead of duplicating them.
$ErrorActionPreference = 'Stop'

$storeRoot = Join-Path $env:APPDATA 'Rebuffer'
$db = Join-Path $storeRoot 'rebuffer.db'
$tool = Join-Path $PSScriptRoot 'seed-tool\target\release\seed-tool.exe'
$dataDir = Join-Path $PSScriptRoot 'seed-data'
New-Item -ItemType Directory -Path $dataDir -Force | Out-Null
if (-not (Test-Path $tool)) { throw "seed-tool not built: $tool" }
if (-not (Test-Path $db)) { throw "store db not found: $db" }

function Write-TextContent([string]$name, [string]$body) {
    $path = Join-Path $dataDir $name
    [System.IO.File]::WriteAllText($path, $body, [System.Text.UTF8Encoding]::new($false))
    return $path
}

function B3([string]$mode, [string]$path) {
    return (& $tool $mode $path).Trim()
}

function Invoke-Sql([string]$sql) {
    $sqlFile = Join-Path $PSScriptRoot 'seed_items.sql.tmp'
    [System.IO.File]::WriteAllText($sqlFile, $sql)
    $null = & sqlite3 $db ".read `"$sqlFile`""
    if ($LASTEXITCODE -ne 0) { throw "sqlite3 failed: $LASTEXITCODE" }
}

# ---- content -------------------------------------------------------------

$long = @"
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. This paragraph is long enough that the seven-line preview panel must clamp it with an ellipsis, proving the text card really renders seven lines of roughly fifteen characters per line before cutting off the rest of the copy.
"@

$json = @'
{
  "schema": "rebuffer-seed",
  "version": 1,
  "items": [
    { "kind": "text", "note": "seed for the code sub-kind preview" },
    { "kind": "text", "note": "this line exists so the block is taller than one screen" },
    { "kind": "image", "note": "not actually stored, just structure" }
  ],
  "retentionDays": 30,
  "maxStoreBytes": null
}
'@

$plain14m = Write-TextContent 'plain14m.txt' 'The quick brown fox jumps over the lazy dog while the sun sets over the harbour and the gulls wheel home.'
$plain3h  = Write-TextContent 'plain3h.txt'  "Meeting notes: W17 measurement run, owner is the perf worker. Deliverables are the latency figure in DECISIONS.md, runtime numbers in PERF.md, and an honest rendering verdict for the popup."
$code26h  = Write-TextContent 'code26h.json' $json
$link1h   = Write-TextContent 'link1h.txt'   'https://github.com/anomalyco/opencode/issues'
$color4d  = Write-TextContent 'color4d.txt'  '#3D8BFD'
$long2d   = Write-TextContent 'long2d.txt'   $long

# ---- hashes + blob files -------------------------------------------------

$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()

$items = @(
    @{ name='plain14m'; file=$plain14m; mode='text'; ext='TXT';  sub='plain'; ageMin=14;  preview=$plain14m },
    @{ name='plain3h';  file=$plain3h;  mode='text'; ext='TXT';  sub='plain'; ageMin=180; preview=$plain3h },
    @{ name='code26h';  file=$code26h;  mode='text'; ext='JSON'; sub='code';  ageMin=1560; preview=$code26h },
    @{ name='link1h';   file=$link1h;   mode='text'; ext='TXT';  sub='link';  ageMin=60;   preview=$link1h },
    @{ name='color4d';  file=$color4d;  mode='text'; ext='TXT';  sub='color'; ageMin=5760; preview=$color4d },
    @{ name='long2d';   file=$long2d;   mode='text'; ext='TXT';  sub='plain'; ageMin=2880; preview=$long2d }
)

$inserts = New-Object System.Collections.Generic.List[string]
$i = 0
foreach ($it in $items) {
    $i++
    $content = [System.IO.File]::ReadAllBytes($it.file)
    $hash = B3 $it.mode $it.file
    $rel = "$($hash.Substring(0,2))/$($hash.Substring(2,2))/$hash"
    $blobDir = Join-Path $storeRoot "blobs\$($hash.Substring(0,2))\$($hash.Substring(2,2))"
    New-Item -ItemType Directory -Path $blobDir -Force | Out-Null
    [System.IO.File]::WriteAllBytes((Join-Path $blobDir $hash), $content)

    $created = $now - ($it.ageMin * 60000)
    $preview = [System.IO.File]::ReadAllText($it.file)
    if ($preview.Length -gt 200) { $preview = $preview.Substring(0, 200) }
    $previewEsc = $preview.Replace("'", "''")
    $title = $null

    $inserts.Add(@"
INSERT INTO items (id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path, title, preview_text, ext, mime, byte_size, width, height, duration_ms, source_app, copy_count, created_at, first_seen_at, last_used_at, pinned)
VALUES ($i, 'text', '$($it.sub)', '$hash', '$rel', NULL, 0, NULL, NULL, '$previewEsc', '$($it.ext)', 'text/plain', $($content.Length), NULL, NULL, NULL, 'seed-tool', 1, $created, $created, NULL, 0)
ON CONFLICT(id) DO UPDATE SET hash='$hash', blob_path='$rel', preview_text='$previewEsc', created_at=$created;
"@)
}

Invoke-Sql ($inserts -join "`n")
Write-Host "seeded $($inserts.Count) items at controlled ages"