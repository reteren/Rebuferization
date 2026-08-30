# Re-creates 'image' kind rows for PNG blobs already on disk (after a store
# reset wiped the items table). Blob + webp thumb files are preserved by the
# janitor only while referenced, so run this BEFORE starting the app.
# RUN ONLY WHILE REBUFFER.EXE IS STOPPED. Idempotent.
$ErrorActionPreference = 'Stop'

$storeRoot = Join-Path $env:APPDATA 'Rebuffer'
$db = Join-Path $storeRoot 'rebuffer.db'
$thumbsDir = Join-Path $storeRoot 'blobs\thumbs'
if (-not (Test-Path $db)) { throw "store db not found: $db" }
if (-not (Test-Path $thumbsDir)) { throw "no thumbs dir: $thumbsDir" }

$thumbs = Get-ChildItem $thumbsDir -Filter '*.webp' | Sort-Object LastWriteTime -Descending | Select-Object -First 8
if ($thumbs.Count -eq 0) { throw 'no thumbnail files on disk' }

$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$inserts = New-Object System.Collections.Generic.List[string]
$i = 0
foreach ($t in $thumbs) {
    $hash = $t.BaseName
    if ($hash.Length -lt 64) { continue }
    $rel = "$($hash.Substring(0,2))/$($hash.Substring(2,2))/$hash"
    $blob = Join-Path $storeRoot "blobs\$rel"
    if (-not (Test-Path $blob)) { Write-Host "skip $hash (blob missing)"; continue }
    $size = (Get-Item $blob).Length
    $i++
    $created = $now - ($i * 7 * 60000)   # 7..56 min ago
    $inserts.Add(@"
INSERT INTO items (id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path, title, preview_text, ext, mime, byte_size, width, height, duration_ms, source_app, copy_count, created_at, first_seen_at, last_used_at, pinned)
VALUES ($(900 + $i), 'image', NULL, '$hash', '$rel', '$($t.Name)', 0, NULL, NULL, NULL, 'PNG', 'image/png', $size, NULL, NULL, NULL, 'seed-tool', 1, $created, $created, NULL, 0)
ON CONFLICT(id) DO UPDATE SET hash='$hash', blob_path='$rel', thumb_path='$($t.Name)', byte_size=$size, created_at=$created;
"@)
}
if ($inserts.Count -eq 0) { throw 'no image rows to insert' }

$sqlFile = Join-Path $PSScriptRoot 'seed_png.sql.tmp'
[System.IO.File]::WriteAllText($sqlFile, ($inserts -join "`n"))
$null = & sqlite3 $db ".read `"$sqlFile`""
if ($LASTEXITCODE -ne 0) { throw "sqlite3 failed: $LASTEXITCODE" }
Write-Host "seeded $($inserts.Count) image items (ids 901+)"