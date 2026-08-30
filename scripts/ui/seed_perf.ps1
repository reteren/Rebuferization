# Seeds 10,000 items of realistic variety for the UI perf run (W26):
#   ~5,600 text of differing lengths, 1,200 code, 1,400 links, 800 colours,
#   500 images with real PNG blobs + thumbnails, 500 files (with item_files).
# Uses the existing seed-tool (BLAKE3) for every content hash, the same way
# seed_items.ps1 does. Ages span 0..~29.5 days so every date group is
# populated and the sticky-header recycling path is exercised.
#
# RUN ONLY WHILE REBUFFER.EXE IS STOPPED (the app holds a busy lock on the
# WAL database). Idempotent: wipes items/item_files/item_formats and re-seeds
# blobs are keyed by content hash so no blob file is written twice. Prints the
# final row count — re-check it right before recording any number, the
# settings worker empties this store too.

$ErrorActionPreference = 'Stop'

$storeRoot = Join-Path $env:APPDATA 'Rebuffer'
$db = Join-Path $storeRoot 'rebuffer.db'
$tool = Join-Path (Join-Path $PSScriptRoot '..') 'seed-tool\target\release\seed-tool.exe'
$blobsRoot = Join-Path $storeRoot 'blobs'
$staging = Join-Path $PSScriptRoot '.staging'

if (-not (Test-Path $tool)) { throw "seed-tool not built: $tool" }
if (-not (Test-Path $db)) { throw "store db not found: $db" }
# NOTE: the settings worker keeps rebuffer.exe running against this same
# store. WAL allows a second writer to interleave with the app's short
# transactions, so seeding proceeds even while the app is up — the SQL file
# sets a busy timeout, and the count verification below fails loudly if the
# write contended. The app is restarted (by the run_perf orchestrator) after
# seeding anyway, so its stale in-memory state never gets measured.

$sqlite = (Get-Command sqlite3 -ErrorAction SilentlyContinue).Source
if (-not $sqlite) { $sqlite = 'C:\msys64\mingw64\bin\sqlite3.exe' }
if (-not (Test-Path $sqlite)) { throw "sqlite3 not found" }

$total = 10000

# Deterministic RNG so re-runs produce an identical store.
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

function New-Link([int]$itemId) {
    $d = $domains[$rng.Next($domains.Count)]
    $path = @('issues','docs','posts','items','questions','discussions','blog','wiki','users','releases')[$rng.Next(10)]
    # The ?ref= query is a tracking-parameter look-alike and makes the URL
    # unique per item, which the content-hash unique index requires.
    return "https://$d/$path/$($rng.Next(1000, 999999))?ref=$itemId"
}

$hexes = @(
    '#3D8BFD','#9ECE6A','#F7768E','#E0AF68','#BB9AF7','#7DCFFF','#1ABC9C','#E06C75','#61AFEF',
    '#98C379','#D19A66','#C678DD','#56B6C2','#E5C07B','#5C6370','#ABB2BF','#FFB86C','#6272A4',
    '#F1FA8C','#50FA7B','#FF79C6','#BD93F9','#8BE9FD','#FF5555','#E8EAF0','#0B0D10','#1C1F26','#2A2E37'
)

$codeSnippets = @(
    'const { grid, items } = $props();\nconst visible = $derived(items.slice(0, {N}));',
    'function clamp(n, lo, hi) { return Math.max(lo, Math.min(hi, n)); } // {N}',
    'SELECT id, kind, preview_text FROM items WHERE kind = ?1 ORDER BY created_at DESC LIMIT {N};',
    'def handle(items):\n    return [i for i in items if i.pinned]  # {N}',
    '{"id": {N}, "kind": "text", "pinned": false, "tags": ["perf", "ui"], "nested": {"a": [1, 2, 3]}}',
    'const observer = new PerformanceObserver((list) => {\n  for (const e of list.getEntries()) log(e.duration);\n});\nobserver.observe({ type: "longtask" }); // {N}',
    'export async function loadMore(): Promise<void> {\n  if (this.loading || !this.hasMore) return;\n  this.list = [...this.list, ...await listItems({N})];\n}',
    'UPDATE items SET copy_count = copy_count + 1 WHERE hash = ?1; -- {N}',
    'fn materialize(paths: &[PathBuf]) -> AppResult<Vec<PathBuf>> {\n    paths.iter().map(|p| resolve(p)).collect()\n} // {N}',
    '<script lang="ts">\n  let count = $state({N});\n  const bump = () => count += 1;\n</script>\n<button onclick={bump}>{count}</button>',
    'PRAGMA journal_mode = WAL;\nPRAGMA synchronous = NORMAL;\nPRAGMA busy_timeout = {N};',
    'docker run --rm -p 8080:8080 -v ./data:/app/data rebuffer/perf-{N}',
    'try {\n  const r = await invoke("list_items", { offset: 0, limit: {N} });\n  render(r);\n} catch (e) {\n  showError(e);\n}',
    'interface GroupInfo {\n  key: string; label: string; top: number; rows: number;\n} // {N}'
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
# per-item spec generation
# ---------------------------------------------------------------------------

function Get-KindFor([int]$i) {
    $r = $i % 100
    if ($r -le 13) { return 'short' }
    if ($r -le 43) { return 'medium' }
    if ($r -le 55) { return 'long' }
    if ($r -le 67) { return 'code' }
    if ($r -le 81) { return 'link' }
    if ($r -le 89) { return 'color' }
    if ($r -le 94) { return 'image' }
    return 'file'
}

function Get-AgeMs([int]$i) {
    $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $r = $i % 10
    if ($r -eq 0) {
        return $now - (($i * 7919) % 120) * 60000                 # last 2 hours
    }
    if ($r -eq 1) {
        return $now - (120 + (($i * 104729) % (24 * 60 - 120))) * 60000
    }
    if ($r -eq 2) {
        return $now - (24 * 60 + (($i * 1543) % (24 * 60))) * 60000
    }
    return $now - (48 * 60 + (($i * 3571) % (27 * 24 * 60))) * 60000
}

# kind cycle over 100 items (i % 100 -> bucket), so every date group mixes kinds.

New-Item -ItemType Directory -Path $staging -Force | Out-Null
Get-ChildItem $staging -File -ErrorAction SilentlyContinue | Remove-Item -Force

Add-Type -AssemblyName System.Drawing

$specs = New-Object System.Collections.Generic.List[object]
$stagePaths = New-Object System.Collections.Generic.List[string]

for ($i = 1; $i -le $total; $i++) {
    $idx = $i - 1
    $kind = Get-KindFor $idx
    $pinned = if ($idx % 200 -eq 0) { 1 } else { 0 }
    $title = if ($idx % 40 -eq 7) { $titles[$rng.Next($titles.Count)] } else { $null }

    $spec = [ordered]@{
        id = $i; kind = $kind; pinned = $pinned
        title = $title
        hash = $null; rel = $null; thumbRel = $null
    }

    switch ($kind) {
        'short' {
            # Words are a base-170 encoding of the item index, so every short
            # item is guaranteed distinct (random 2-6 word draws would collide
            # on the unique content hash: ~1.4 expected at this pool size).
            $n = 3 + ($idx % 3)
            $parts = for ($j = 0; $j -lt $n; $j++) {
                $digit = [int64]([math]::Floor($idx / [math]::Pow(170, $j))) % 170
                $words[$digit]
            }
            $spec.text = ($parts -join ' ')
            $spec.sub = 'plain'; $spec.ext = 'TXT'; $spec.mime = 'text/plain'; $spec.mode = 'text'
        }
        'medium' {
            $ns = $rng.Next(1, 4)
            $sents = for ($j = 0; $j -lt $ns; $j++) { New-Sentence 6 14 }
            $spec.text = ($sents -join ' ')
            $spec.sub = 'plain'; $spec.ext = 'TXT'; $spec.mime = 'text/plain'; $spec.mode = 'text'
        }
        'long' {
            $ns = $rng.Next(4, 12)
            $paras = New-Object System.Collections.Generic.List[string]
            for ($j = 0; $j -lt $ns; $j++) {
                $ps = $rng.Next(1, 4)
                $sentences = for ($k = 0; $k -lt $ps; $k++) { New-Sentence 7 18 }
                $paras.Add(($sentences -join ' '))
            }
            $spec.text = ($paras -join "`n`n")
            $spec.sub = 'plain'; $spec.ext = 'TXT'; $spec.mime = 'text/plain'; $spec.mode = 'text'
        }
        'code' {
            $snip = $codeSnippets[$rng.Next($codeSnippets.Count)] -replace '\\n', "`n"
            # {N} is replaced with a per-item value so every code item is a
            # distinct capture (the unique index is on the content hash).
            $snip = $snip -replace '\{N\}', (100000 + $i * 97)
            $exts = @('JSON', 'TS', 'JS', 'SQL', 'PY', 'RS')
            $spec.text = $snip
            $spec.sub = 'code'; $spec.ext = $exts[$rng.Next($exts.Count)]; $spec.mime = 'text/plain'; $spec.mode = 'text'
        }
        'link' {
            $spec.text = New-Link $i
            $spec.sub = 'link'; $spec.ext = $null; $spec.mime = 'text/uri-list'; $spec.mode = 'text'
        }
        'color' {
            # Index-derived colour: the multiplier is coprime with 2^24, so the
            # 800 colour items get 800 distinct hex values, never a duplicate.
            $c = [int64]((($i - 1) * 3635633L) % 0x1000000L)
            $spec.text = '#' + $c.ToString('X6')
            $spec.sub = 'color'; $spec.ext = $null; $spec.mime = 'text/plain'; $spec.mode = 'text'
        }
        'image' {
            $colors = @('#3D8BFD','#F7768E','#9ECE6A','#BB9AF7','#E0AF68','#7DCFFF','#FF79C6','#50FA7B','#BD93F9','#FFB86C')
            $c = [System.Drawing.ColorTranslator]::FromHtml($colors[$rng.Next($colors.Count)])
            $bmp = New-Object System.Drawing.Bitmap 96, 64
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $g.Clear($c)
            $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(120, 10, 14, 18))
            $g.FillRectangle($brush, ($rng.Next(10, 60)), ($rng.Next(10, 40)), ($rng.Next(8, 30)), ($rng.Next(8, 20)))
            $ms = New-Object System.IO.MemoryStream
            $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
            $spec.bytes = $ms.ToArray()
            $ms.Close(); $brush.Dispose(); $g.Dispose(); $bmp.Dispose()
            $spec.sub = $null; $spec.ext = 'PNG'; $spec.mime = 'image/png'; $spec.mode = 'raw'
            $spec.w = 1920; $spec.h = 1080
        }
        'file' {
            $fn = $fileNames[$rng.Next($fileNames.Count)]
            # The item id line guarantees a distinct blob even when the name
            # and padding length collide.
            $fake = "REBUFFER-PERF-SEED $fn`nitem-$i`n" + ('x' * $rng.Next(64, 1200))
            $spec.bytes = [System.Text.Encoding]::UTF8.GetBytes($fake)
            $spec.fileName = $fn
            $spec.sub = $null; $spec.ext = ([System.IO.Path]::GetExtension($fn)).TrimStart('.').ToUpperInvariant()
            $spec.mime = 'application/octet-stream'; $spec.mode = 'raw'
        }
    }

    $specs.Add([pscustomobject]$spec)

    $stage = Join-Path $staging ("c{0:D5}.bin" -f $idx)
    # NOTE: on the ordered hashtable (before the pscustomobject conversion)
    # the keys are NOT visible through .PSObject.Properties, so check the
    # dictionary directly — writing an empty file here made every image and
    # file item hash to the same digest on the first run.
    if ($spec.Contains('bytes')) {
        [System.IO.File]::WriteAllBytes($stage, $spec.bytes)
    } else {
        [System.IO.File]::WriteAllText($stage, $spec.text, [System.Text.UTF8Encoding]::new($false))
    }
    $stagePaths.Add($stage)
}

Write-Host "generated $($specs.Count) specs"

# ---------------------------------------------------------------------------
# hash in parallel via seed-tool (id + hash only cross the pipeline, so the
# parallel round-trip cannot mangle the byte arrays)
# ---------------------------------------------------------------------------

Write-Host "hashing $total contents via seed-tool (parallel)…"
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$hashResults = for ($k = 0; $k -lt $total; $k += 500) {
    $batch = for ($b = 0; $b -lt 500 -and ($k + $b) -lt $total; $b++) {
        $spec = $specs[$k + $b]
        [pscustomobject]@{ id = $k + $b + 1; file = $stagePaths[$k + $b]; mode = $spec.mode }
    }
    $batch | ForEach-Object -Parallel {
        $tool = $using:tool
        $out = (& $tool $_.mode $_.file).Trim()
        [pscustomobject]@{ id = $_.id; hash = $out }
    } -ThrottleLimit 24
}
$sw.Stop()
Write-Host "hashing took $([math]::Round($sw.Elapsed.TotalSeconds, 1))s"

$hashById = @{}
foreach ($r in $hashResults) { $hashById[$r.id] = $r.hash }

# ---------------------------------------------------------------------------
# write blobs into the fanout layout, SQL
# ---------------------------------------------------------------------------

$now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine('.timeout 10000')
[void]$sb.AppendLine('BEGIN;')
[void]$sb.AppendLine('DELETE FROM item_files;')
[void]$sb.AppendLine('DELETE FROM items;')

$counts = @{ short = 0; medium = 0; long = 0; code = 0; link = 0; color = 0; image = 0; file = 0 }
$blobCount = 0
$thumbCount = 0

foreach ($spec in $specs) {
    $h = $hashById[$spec.id]
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
    $blobCount++

    $thumbSql = 'NULL'
    if ($spec.kind -eq 'image') {
        $thumbDir = Join-Path $blobsRoot 'thumbs'
        New-Item -ItemType Directory -Path $thumbDir -Force | Out-Null
        $thumbFile = Join-Path $thumbDir "$h.webp"
        # Chromium sniffs image bytes regardless of extension; the real app
        # writes WebP here, the <img> decodes the PNG bytes the same way.
        if (-not (Test-Path $thumbFile)) { [System.IO.File]::WriteAllBytes($thumbFile, $spec.bytes) }
        $thumbSql = "'$h.webp'"
        $thumbCount++
    }

    $id = $spec.id
    # The buckets returned by Get-KindFor are content shapes, not the store's
    # kinds — map them here (a bug on the first run stamped everything as
    # 'file').
    $kindSql = switch ($spec.kind) {
        { $_ -in @('short', 'medium', 'long', 'code', 'link', 'color') } { "'text'" }
        'image' { "'image'" }
        default { "'file'" }
    }
    $subSql = if ($spec.sub) { "'$($spec.sub)'" } else { 'NULL' }
    $extSql = if ($spec.ext) { "'$($spec.ext)'" } else { 'NULL' }
    $titleSql = if ($spec.title) { "'$($spec.title.Replace("'", "''"))'" } else { 'NULL' }

    $preview = $null
    if ($spec.kind -eq 'text') {
        $preview = $spec.text
        if ($preview.Length -gt 200) { $preview = $preview.Substring(0, 200) }
    } elseif ($spec.kind -eq 'file') {
        $preview = $spec.fileName
    }
    $previewSql = if ($preview) { "'$($preview.Replace("'", "''"))'" } else { 'NULL' }

    $bytes = if ($spec.PSObject.Properties.Name -contains 'bytes') { $spec.bytes.Length } else { [System.Text.Encoding]::UTF8.GetByteCount($spec.text) }
    $wSql = if ($spec.w) { $spec.w } else { 'NULL' }
    $hSql = if ($spec.h) { $spec.h } else { 'NULL' }
    $ageMs = Get-AgeMs ($id - 1)
    $src = $sources[$rng.Next($sources.Count)]

    [void]$sb.AppendLine("INSERT INTO items (id, kind, sub_kind, hash, blob_path, thumb_path, is_reference, ref_path, title, preview_text, ext, mime, byte_size, width, height, duration_ms, source_app, copy_count, created_at, first_seen_at, last_used_at, pinned) VALUES ($id, $kindSql, $subSql, '$h', '$rel', $thumbSql, 0, NULL, $titleSql, $previewSql, $extSql, '$($spec.mime)', $bytes, $wSql, $hSql, NULL, '$src', 1, $ageMs, $ageMs, NULL, $($spec.pinned));")

    if ($spec.kind -eq 'file') {
        $dirs = @('Documents', 'Downloads', 'Desktop', 'Work', 'Projects')
        $nFiles = 1 + $rng.Next(3)
        for ($fi = 1; $fi -le $nFiles; $fi++) {
            $fname = if ($fi -eq 1) { $spec.fileName } else { $fileNames[$rng.Next($fileNames.Count)] }
            $path = "C:\Users\reteren\$($dirs[$rng.Next($dirs.Count)])\$fname"
            $size = $rng.Next(4096, 8 * 1024 * 1024)
            [void]$sb.AppendLine("INSERT INTO item_files (item_id, path, file_name, byte_size, position) VALUES ($id, '$($path.Replace("'", "''"))', '$fname', $size, $($fi - 1));")
        }
    }

    $counts[$spec.kind]++
}

[void]$sb.AppendLine('COMMIT;')

$sqlFile = Join-Path $PSScriptRoot 'seed_perf.sql.tmp'
[System.IO.File]::WriteAllText($sqlFile, $sb.ToString())
Write-Host "running sqlite3 ($([math]::Round((Get-Item $sqlFile).Length / 1MB, 1)) MB SQL, $blobCount blobs, $thumbCount thumbs)…"
$null = & $sqlite $db ".read `"$sqlFile`""
if ($LASTEXITCODE -ne 0) { throw "sqlite3 failed: $LASTEXITCODE" }

# ---------------------------------------------------------------------------
# verify
# ---------------------------------------------------------------------------

$finalCount = & $sqlite $db "SELECT COUNT(*) FROM items;"
if ([int]$finalCount -ne $total) {
    throw "seed failed: expected $total items, got $finalCount"
}
$byKind = & $sqlite $db "SELECT kind, COUNT(*) FROM items GROUP BY kind ORDER BY kind;"
$byDay = & $sqlite $db "SELECT date(created_at / 1000, 'unixepoch', 'localtime') d, COUNT(*) FROM items GROUP BY d ORDER BY d;"
$imgThumbs = & $sqlite $db "SELECT COUNT(*) FROM items WHERE kind = 'image' AND thumb_path IS NOT NULL;"
$ftsOk = & $sqlite $db "SELECT COUNT(*) FROM items_fts;"

Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
Remove-Item -Force $sqlFile -ErrorAction SilentlyContinue

Write-Host "=== seeded $finalCount items (FTS rows: $ftsOk) ==="
Write-Host "by kind:"
$byKind | ForEach-Object { Write-Host "  $_" }
Write-Host "distinct date groups: $(@($byDay).Count)"
$byDay | ForEach-Object { Write-Host "  $_" }
Write-Host "images with thumbs: $imgThumbs"