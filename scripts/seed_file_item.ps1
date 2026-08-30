# Seeds one 'file' kind item (a real file, as a CF_HDROP capture would store it)
# so the popup's Files tab / file card rendering can be verified.
# RUN ONLY WHILE REBUFFER.EXE IS STOPPED. Idempotent: upserts the same row.
$ErrorActionPreference = 'Stop'

$storeRoot = Join-Path $env:APPDATA 'Rebuffer'
$db = Join-Path $storeRoot 'rebuffer.db'
$tool = Join-Path $PSScriptRoot 'seed-tool\target\release\seed-tool.exe'
if (-not (Test-Path $tool)) { throw "seed-tool not built: $tool" }
if (-not (Test-Path $db)) { throw "store db not found: $db" }

$src = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')) 'docs\DECISIONS.md'
if (-not (Test-Path $src)) { throw "source file not found: $src" }

$content = [System.IO.File]::ReadAllBytes($src)
$hash = (& $tool 'raw' $src).Trim()
$rel = "$($hash.Substring(0,2))/$($hash.Substring(2,2))/$hash"
$blobDir = Join-Path $storeRoot "blobs\$($hash.Substring(0,2))\$($hash.Substring(2,2))"
New-Item -ItemType Directory -Path $blobDir -Force | Out-Null
$blobPath = Join-Path $blobDir $hash
if (-not (Test-Path $blobPath)) { [System.IO.File]::WriteAllBytes($blobPath, $content) }

$file = [System.IO.Path]::GetFileName($src)
$ext = [System.IO.Path]::GetExtension($src).TrimStart('.').ToUpperInvariant()
$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$created = $now - 30 * 60000  # 30 min ago

$sql = @"
BEGIN;
INSERT INTO items (id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path, title, preview_text, ext, mime, byte_size, width, height, duration_ms, source_app, copy_count, created_at, first_seen_at, last_used_at, pinned)
VALUES (100, 'file', NULL, '$hash', '$rel', NULL, 0, NULL, NULL, '$file', '$ext', 'application/octet-stream', $($content.Length), NULL, NULL, NULL, 'seed-tool', 1, $created, $created, NULL, 0)
ON CONFLICT(id) DO UPDATE SET hash='$hash', blob_path='$rel', preview_text='$file', ext='$ext', byte_size=$($content.Length), created_at=$created;
DELETE FROM item_files WHERE item_id = 100;
INSERT INTO item_files (item_id, path, file_name, byte_size, position) VALUES (100, '$src', '$file', $($content.Length), 0);
COMMIT;
"@
$sqlFile = Join-Path $PSScriptRoot 'seed_file_item.sql.tmp'
[System.IO.File]::WriteAllText($sqlFile, $sql)
$null = & sqlite3 $db ".read `"$sqlFile`""
if ($LASTEXITCODE -ne 0) { throw "sqlite3 failed: $LASTEXITCODE" }
Write-Host "seeded file item id=100 ($file, $($content.Length) bytes)"