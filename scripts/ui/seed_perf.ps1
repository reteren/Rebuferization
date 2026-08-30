# Seeds 10,000 items of realistic variety for the UI perf run (W26):
#   ~5,600 text of differing lengths, 1,200 code, 1,400 links, 800 colours,
#   500 images with real PNG blobs + thumbnails, 500 files (with item_files).
# Uses the existing seed-tool (BLAKE3) for every content hash, the same way
# seed_items.ps1 does. Ages span 0..~29.5 days so every date group is
# populated and the sticky-header recycling path is exercised.
#
# RUN ONLY WHILE REBUFFER.EXE IS STOPPED (the DB is opened in WAL and the app
# holds a busy lock). Idempotent: wipes items/item_files/item_formats and the
# blob tree, then re-seeds. Prints the final row count — re-check it right
# before recording any number, the settings worker empties this store too.

$ErrorActionPreference = 'Stop'

$storeRoot = Join-Path $env:APPDATA 'Rebuffer'
$db = Join-Path $storeRoot 'rebuffer.db'
$tool = Join-Path (Join-Path $PSScriptRoot '..') 'seed-tool\target\release\seed-tool.exe'
$blobsRoot = Join-Path $storeRoot 'blobs'
$staging = Join-Path $PSScriptRoot '.staging'

if (-not (Test-Path $tool)) { throw "seed-tool not built: $tool" }
if (-not (Test-Path $db)) { throw "store db not found: $db" }
if (Get-Process rebuffer -ErrorAction SilentlyContinue) {
    throw 'rebuffer.exe is running — stop it before seeding (the app holds the DB lock)'
}

$sqlite = (Get-Command sqlite3 -ErrorAction SilentlyContinue).Source
if (-not $sqlite) { $sqlite = 'C:\msys64\mingw64\bin\sqlite3.exe' }
if (-not (Test-Path $sqlite)) { throw "sqlite3 not found" }

$total = 10000

# Deterministic RNG so re-runs produce the identical store.
$rng = [System.Random]::new(20260830)

# ---------------------------------------------------------------------------
# content pools
# ---------------------------------------------------------------------------

$words = @(
    'clipboard','history','renderer','pipeline','buffer','window','thread','layout','compositor',
    'paint','frame','sticky','header','virtual','scroll','tile','spacer','overscan','column',
    'pitch','group','today','yesterday','debounce','facet','thumb','blob','hash','cache','store',
    'query','index','fts','page','limit','offset','task','worker','coordinator','dispatch','session',
    'mutation','observer','promise','async','await','effect','derived','state','signal','reactive',
    'dom','node','element','style','class','count','total','bytes','percent','ratio','median',
    'worst','jank','stutter','smooth','fluid','fps','hertz','measure','record','report','finding',
    'verdict','target','meet','miss','degrade','acceptable','leak','recycle','reuse','patch','diff',
    'keyed','block','item','entity','row','cell','card','badge','label','title','note','snippet',
    'paragraph','sentence','word','string','number','value','field','table','constraint','trigger',
    'transaction','commit','journal','wal','checkpoint','lock','timeout','busy','error','warn',
    'info','debug','trace','fatal','panic','unwrap','expect','option','result','map','filter',
    'reduce','collect','iterate','loop','while','for','break','continue','return','yield','struct',
    'enum','trait','impl','module','function','const','static','let','mut','ref','clone','copy',
    'borrow','lifetime','generic','closure','iterator','adapter','into','from','catch','finally',
    'throw','new','delete','update','insert','select','where','order','limit','join','group',
    'aggregate','window','partition','rank','dense','first','last','previous','next','current'
)

function New-Sentence([int]$minWords, [int]$maxWords) {
    $n = $rng.Next($minWords, $maxWords + 1)
    $parts = for ($i = 0; $i -lt $n; $i++) { $words[$rng.Next($words.Count)] }
    $s = ($parts -join ' ')
    return $s.Substring(0, 1).ToUpperInvariant() + $s.Substring(1) + '.'
}

$domains = @(
    'github.com','developer.mozilla.org','docs.rs','news.ycombinator.com','stackoverflow.com',
    'crates.io','learn.microsoft.com','www.reddit.com','x.com','www.youtube.com','blog.rust-lang.org',
    'svelte.dev','vite.dev','www.typescriptlang.org','tailwindcss.com','www.figma.com','dribbble.com',
    'www.producthunt.com','lobste.rs','arxiv.org','pub.dev','www.npmjs.com','platform.openai.com'
)

function New-Link {
    $d = $domains[$rng.Next($domains.Count)]
    $path = @('issues','docs','posts','items','questions','discussions','blog','wiki','users','releases')[$rng.Next(10)]
    return "https://$d/$path/$($rng.Next(1000, 999999))"
}

$hexes = @(
    '#3D8BFD','#9ECE6A','#F7768E','#E0AF68','#BB9AF7','#7DCFFF','#1ABC9C','#E06C75','#61AFEF',
    '#98C379','#D19A66','#C678DD','#56B6C2','#E5C07B','#5C6370','#ABB2BF','#FFB86C','#6272A4',
    '#F1FA8C','#50FA7B','#FF79C6','#BD93F9','#8BE9FD','#FF5555','#E8EAF0','#0B0D10','#1C1F26','#2A2E37'
)

$codeSnippets = @(
    'const { grid, items } = $props();\nconst visible = $derived(items.slice(0, 200));',
    'function clamp(n, lo, hi) { return Math.max(lo, Math.min(hi, n)); }',
    'SELECT id, kind, preview_text FROM items WHERE kind = ?1 ORDER BY created_at DESC LIMIT 200;',
    'def handle(items):\n    return [i for i in items if i.pinned]',
    '{"id": 42, "kind": "text", "pinned": false, "tags": ["perf", "ui"], "nested": {"a": [1, 2, 3]}}',
    'const observer = new PerformanceObserver((list) => {\n  for (const e of list.getEntries()) log(e.duration);\n});\nobserver.observe({ type: "longtask" });',
    'export async function loadMore(): Promise<void> {\n  if (this.loading || !this.hasMore) return;\n  this.list = [...this.list, ...await listItems(200)];\n}',
    'UPDATE items SET copy_count = copy_count + 1 WHERE hash = ?1;',
    'fn materialize(paths: &[PathBuf]) -> AppResult<Vec<PathBuf>> {\n    paths.iter().map(|p| resolve(p)).collect()\n}',
    '<script lang="ts">\n  let count = $state(0);\n  const bump = () => count += 1;\n</script>\n<button onclick={bump}>{count}</button>',
    'PRAGMA journal_mode = WAL;\nPRAGMA synchronous = NORMAL;\nPRAGMA busy_timeout = 5000;',
    'docker run --rm -p 8080:8080 -v ./data:/app/data rebuffer/perf',
    'try {\n  const r = await invoke("list_items", { offset: 0, limit: 200 });\n  render(r);\n} catch (e) {\n  showError(e);\n}',
    'interface GroupInfo {\n  key: string; label: string; top: number; rows: number;\n}'
)

$fileNames = @(
    'quarterly-report-2026-Q3.pdf','budget-2026.xlsx','deck-v2.pptx','photo-20260830.zip',
    'backup-config.tar.gz','release-notes.md','api-spec.yaml','design-tokens.json',
    'performance-audit.pdf','sprint-board.csv','invoice-0412.pdf','meeting-notes.docx',
    'wireframe-3.fig','build-log.txt','dataset-train.parquet','invoice-0413.pdf'
)

$titles = @(
    'Q3 planning notes','Reading list','API reference','Sprint 42 retro','Keyboard shortcuts',
    'Deploy checklist','Font pairing ideas','Component inventory','Onboarding doc','Release notes'
)

$sources = @('chrome.exe','vscode.exe','explorer.exe','powershell.exe','notepad.exe','msedge.exe','slack.exe','discord.exe','code.exe','winget.exe')

# ---------------------------------------------------------------------------
# per-item spec
# ---------------------------------------------------------------------------

# kind cycle over 100 items (i % 100 -> kind bucket)
function Get-KindFor([int]$i) {
    $r = $i % 100
    if ($r -le 13) { return 'short' }        # 14% short plain
    if ($r -le 43) { return 'medium' }       # 30% medium plain
    if ($r -le 55) { return 'long' }         # 12% long plain
    if ($r -le 67) { return 'code' }         # 12% code
    if ($r -le 81) { return 'link' }         # 14% link
    if ($r -le 89) { return 'color' }        # 8% colour
    if ($r -le 94) { return 'image' }        # 5% image
    return 'file'                            # 5% file
}

function Get-AgeMs([int]$i) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $r = $i % 10
    if ($r -eq 0) {
        # last two hours (Today)
        return $now - (($i * 7919) % 120) * 60000
    }
    if ($r -eq 1) {
        # the rest of today
        return $now - (120 + (($i * 104729) % (24 * 60 - 120))) * 60000
    }
    if ($r -eq 2) {
        # yesterday
        return $now - (24 * 60 + (($i * 1543) % (24 * 60))) * 60000
    }
    # days 2..29
    return $now - (48 * 60 + (($i * 3571) % (27 * 24 * 60))) * 60000
}

# ---------------------------------------------------------------------------
# generate content + staging files
# ---------------------------------------------------------------------------

New-Item -ItemType Directory -Path $staging -Force | Out-Null
Get-ChildItem $staging -File -ErrorAction SilentlyContinue | Remove-Item -Force

$Add-Type -AssemblyName System.Drawing | Out-Null

$jobs = New-Object System.Collections.Generic.List[object]

for ($i = 1; $i -le $total; $i++) {
    $idx = $i - 1
    $kind = Get-KindFor $idx
    $ageMs = Get-AgeMs $idx
    $pinned = if ($idx % 200 -eq 0) { 1 } else { 0 }
    $title = if ($idx % 40 -eq 7) { $titles[$rng.Next($titles.Count)] } else { $null }

    switch ($kind) {
        'short' {
            $n = $rng.Next(2, 7)
            $parts = for ($j = 0; $j -lt $n; $j++) { $words[$rng.Next($words.Count)] }
            $text = ($parts -join ' ')
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'text'; sub = 'plain'; ext = 'TXT'; mime = 'text/plain'; text = $text; mode = 'text'; w = $null; h = $null })
        }
        'medium' {
            $ns = $rng.Next(1, 4)
            $text = for ($j = 0; $j -lt $ns; $j++) { (New-Sentence 6 14) } | Out-String
            $text = (($text -split "`r?`n") | Where-Object { $_.Trim() }) -join ' '
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'text'; sub = 'plain'; ext = 'TXT'; mime = 'text/plain'; text = $text; mode = 'text'; w = $null; h = $null })
        }
        'long' {
            $ns = $rng.Next(4, 12)
            $paras = New-Object System.Collections.Generic.List[string]
            for ($j = 0; $j -lt $ns; $j++) {
                $ps = $rng.Next(1, 4)
                $sentences = for ($k = 0; $k -lt $ps; $k++) { New-Sentence 7 18 }
                $paras.Add(($sentences -join ' '))
            }
            $text = $paras -join "`n`n"
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'text'; sub = 'plain'; ext = 'TXT'; mime = 'text/plain'; text = $text; mode = 'text'; w = $null; h = $null })
        }
        'code' {
            $snip = $codeSnippets[$rng.Next($codeSnippets.Count)] -replace '\\n', "`n"
            $exts = @('JSON', 'TS', 'JS', 'SQL', 'PY', 'RS')
            $ext = $exts[$rng.Next($exts.Count)]
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'text'; sub = 'code'; ext = $ext; mime = 'text/plain'; text = $snip; mode = 'text'; w = $null; h = $null })
        }
        'link' {
            $url = New-Link
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'text'; sub = 'link'; ext = $null; mime = 'text/uri-list'; text = $url; mode = 'text'; w = $null; h = $null })
        }
        'color' {
            $hex = $hexes[$rng.Next($hexes.Count)]
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'text'; sub = 'color'; ext = $null; mime = 'text/plain'; text = $hex; mode = 'text'; w = $null; h = $null })
        }
        'image' {
            $colors = @('#3D8BFD','#F7768E','#9ECE6A','#BB9AF7','#E0AF68','#7DCFFF','#FF79C6','#50FA7B','#BD93F9','#FFB86C')
            $c = [System.Drawing.ColorTranslator]::FromHtml($colors[$rng.Next($colors.Count)])
            $wpx = 96; $hpx = 64
            $bmp = New-Object System.Drawing.Bitmap $wpx, $hpx
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $g.Clear($c)
            # a second tone so adjacent thumbnails are distinguishable
            $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(120, 10, 14, 18))
            $g.FillRectangle($brush, ($rng.Next(10, 60)), ($rng.Next(10, 40)), ($rng.Next(8, 30)), ($rng.Next(8, 20)))
            $ms = New-Object System.IO.MemoryStream
            $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $bytes = $ms.ToArray()
            $ms.Close(); $brush.Dispose(); $g.Dispose(); $bmp.Dispose()
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'image'; sub = $null; ext = 'PNG'; mime = 'image/png'; bytes = $bytes; mode = 'raw'; w = 1920; h = 1080 })
        }
        'file' {
            $fn = $fileNames[$rng.Next($fileNames.Count)]
            $fake = "REBUFFER-PERF-SEED $fn`n" + ('x' * $rng.Next(64, 1200))
            $bytes = [System.Text.Encoding]::UTF8.GetBytes($fake)
            $ext = ([System.IO.Path]::GetExtension($fn)).TrimStart('.').ToUpperInvariant()
            $jobs.Add([pscustomobject]@{ id = $i; kind = 'file'; sub = $null; ext = $ext; mime = 'application/octet-stream'; bytes = $bytes; mode = 'raw'; w = $null; h = $null; fileName = $fn })
        }
    }
}

Write-Host "generated $($jobs.Count) specs"

# ---------------------------------------------------------------------------
# write staging files, hash in parallel via seed-tool, move into blobs
# ---------------------------------------------------------------------------

$stageFile = Join-Path $staging 'content.bin'
$jobCount = $jobs.Count

$jobsWithFile = New-Object System.Collections.Generic.List[object]
for ($j = 0; $j -lt $jobCount; $j++) {
    $spec = $jobs[$j]
    if ($spec.PSObject.Properties.Name -contains 'bytes') {
        [System.IO.File]::WriteAllBytes($stageFile, $spec.bytes)
    } else {
        $t = $spec.text
        if ($t.Length -gt 200) { $t = $t.Substring(0, 200) }
        [System.IO.File]::WriteAllText($stageFile, $spec.text, [System.Text.UTF8Encoding]::new($false))
    }
    $spec | Add-Member -NotePropertyName stageFile -NotePropertyValue $stageFile
    $jobsWithFile.Add($spec)
}

Write-Host "hashing $jobCount contents via seed-tool (parallel)…"
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$hashed = $jobsWithFile | ForEach-Object -Parallel {
    $tool = $using:tool
    $spec = $_
    $out = (& $tool $spec.mode $spec.stageFile).Trim()
    $spec | Add-Member -NotePropertyName hash -NotePropertyValue $out
    return $spec
} -ThrottleLimit 24
$sw.Stop()
Write-Host "hashing took $([math]::Round($sw.Elapsed.TotalSeconds, 1))s"

$blobCount = 0
$thumbCount = 0
foreach ($spec in $hashed) {
    $h = $spec.hash
    $rel = "$($h.Substring(0,2))/$($h.Substring(2,2))/$h"
    $blobDir = Join-Path $blobsRoot "$($h.Substring(0,2))\$($h.Substring(2,2))"
    New-Item -ItemType Directory -Path $blobDir -Force | Out-Null
    $dest = Join-Path $blobDir $h
    if (-not (Test-Path $dest)) {
        if ($spec.PSObject.Properties.Name -contains 'bytes') {
            [System.IO.File]::WriteAllBytes($dest, $spec.bytes)
        } else {
            [System.IO.File]::WriteAllText($dest, $spec.text, [System.Text.UTF8Encoding]::new($false))
        }
    }
    $spec | Add-Member -NotePropertyName rel -NotePropertyValue $rel
    $blobCount++
    if ($spec.kind -eq 'image') {
        $thumbDir = Join-Path $blobsRoot 'thumbs'
        New-Item -ItemType Directory -Path $thumbDir -Force | Out-Null
        $thumbPath = Join-Path $thumbDir "$h.webp"
        # Chromium sniffs image bytes regardless of extension; the real app
        # writes WebP here, the bytes decode the same way for the <img> tag.
        if (-not (Test-Path $thumbPath)) { [System.IO.File]::WriteAllBytes($thumbPath, $spec.bytes) }
        $spec | Add-Member -NotePropertyName thumbRel -NotePropertyValue "$h.webp"
        $thumbCount++
    }
}
Write-Host "wrote $blobCount blobs, $thumbCount thumbnails"

# ---------------------------------------------------------------------------
# SQL
# ---------------------------------------------------------------------------

$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine('BEGIN;')
[void]$sb.AppendLine('DELETE FROM item_files;')
[void]$sb.AppendLine('DELETE FROM items;')
[void]$sb.AppendLine('DELETE FROM sqlite_sequence WHERE name IN (''items'', ''item_files'', ''item_formats'');')

$counts = @{ short = 0; medium = 0; long = 0; code = 0; link = 0; color = 0; image = 0; file = 0 }

foreach ($spec in $hashed) {
    $id = $spec.id
    $kind = $spec.kind
    $sub = if ($spec.sub) { "'$($spec.sub)'" } else { 'NULL' }
    $ext = if ($spec.ext) { "'$($spec.ext)'" } else { 'NULL' }
    $mime = "'$($spec.mime)'"
    $hash = $spec.hash
    $rel = $spec.rel
    $thumb = if ($spec.PSObject.Properties.Name -contains 'thumbRel') { "'$($spec.thumbRel)'" } else { 'NULL' }
    $title = if ($titleVal = $spec.PSObject.Properties.Name -contains 'title') { 'NULL' } else { 'NULL' }
    $titleSql = if ($spec.PSObject.Properties.Name -contains 'title' -and $spec.title) { "'$($spec.title.Replace("'", "''"))'" } else { 'NULL' }

    $preview = $null
    if ($kind -eq 'text') {
        $preview = $spec.text
        if ($preview.Length -gt 200) { $preview = $preview.Substring(0, 200) }
    } elseif ($kind -eq 'file') {
        $preview = $spec.fileName
    }
    $previewSql = if ($preview) { "'$($preview.Replace("'", "''"))'" } else { 'NULL' }

    $bytes = if ($spec.PSObject.Properties.Name -contains 'bytes') { $spec.bytes.Length } else { [System.Text.Encoding]::UTF8.GetByteCount($spec.text) }
    $w = if ($spec.w) { $spec.w } else { 'NULL' }
    $h = if ($spec.h) { $spec.h } else { 'NULL' }
    $pinned = if ($id % 200 -eq 0) { 1 } else { 0 }
    $ageMs = Get-AgeMs ($id - 1)
    $src = $sources[$rng.Next($sources.Count)]

    $kindSql = if ($kind -eq 'text') { "'text'" } elseif ($kind -eq 'image') { "'image'" } else { "'file'" }

    [void]$sb.AppendLine("INSERT INTO items (id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path, title, preview_text, ext, mime, byte_size, width, height, duration_ms, source_app, copy_count, created_at, first_seen_at, last_used_at, pinned) VALUES ($id, $kindSql, $sub, '$hash', '$rel', $thumb, 0, NULL, $titleSql, $previewSql, $ext, $mime, $bytes, $w, $h, NULL, '$src', 1, $ageMs, $ageMs, NULL, $pinned);")

    if ($kind -eq 'file') {
        $dirs = @('Documents', 'Downloads', 'Desktop', 'Work', 'Projects')
        $nFiles = 1 + ($rng.Next(3))
        for ($fi = 1; $fi -le $nFiles; $fi++) {
            $fname = if ($fi -eq 1) { $spec.fileName } else { $fileNames[$rng.Next($fileNames.Count)] }
            $path = "C:\Users\reteren\$($dirs[$rng.Next($dirs.Count)])\$fname"
            $pathSql = $path.Replace("'", "''")
            $size = $rng.Next(4096, 8 * 1024 * 1024)
            [void]$sb.AppendLine("INSERT INTO item_files (item_id, path, file_name, byte_size, position) VALUES ($id, '$pathSql', '$fname', $size, $($fi - 1));")
        }
    }

    $counts[$kind]++
}

[void]$sb.AppendLine('COMMIT;')

$sqlFile = Join-Path $PSScriptRoot 'seed_perf.sql.tmp'
[System.IO.File]::WriteAllText($sqlFile, $sb.ToString())
Write-Host "running sqlite3 ($([math]::Round((Get-Item $sqlFile).Length / 1MB, 1)) MB SQL)…"
$null = & $sqlite $db ".read `"$sqlFile`""
if ($LASTEXITCODE -ne 0) { throw "sqlite3 failed: $LASTEXITCODE" }

# ---------------------------------------------------------------------------
# verify
# ---------------------------------------------------------------------------

$finalCount = & $sqlite $db "SELECT COUNT(*) FROM items;"
$byKind = & $sqlite $db "SELECT kind, COUNT(*) FROM items GROUP BY kind ORDER BY kind;"
$byDay = & $sqlite $db "SELECT date(created_at / 1000, 'unixepoch', 'localtime') d, COUNT(*) FROM items GROUP BY d ORDER BY d;"
$imgThumbs = & $sqlite $db "SELECT COUNT(*) FROM items WHERE kind = 'image' AND thumb_path IS NOT NULL;"

Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
Remove-Item -Force $sqlFile -ErrorAction SilentlyContinue

Write-Host "=== seeded $finalCount items ==="
Write-Host "by kind:"
$byKind | ForEach-Object { Write-Host "  $_" }
Write-Host "groups (days): $((@($byDay)).Count) distinct days, oldest to newest:"
$byDay | ForEach-Object { Write-Host "  $_" }
Write-Host "images with thumbs: $imgThumbs"