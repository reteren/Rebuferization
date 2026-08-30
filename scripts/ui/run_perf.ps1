# W26 UI perf run orchestrator: stop app -> seed 10,000 items -> launch with
# WebView2 CDP port -> show popup -> drive the measurements via measure.mjs ->
# write out/results.jsonl + screenshots -> stop app -> verify the store count.
#
# Usage: .\run_perf.ps1
# Requires: node 22+, sqlite3, seed-tool built, rebuffer debug binary built.

$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot '..\..'
$exe = Join-Path $root 'src-tauri\target\debug\rebuffer.exe'
$out = Join-Path $PSScriptRoot 'out'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$jsonl = Join-Path $out 'results.jsonl'
if (Test-Path $jsonl) { Remove-Item $jsonl -Force }

if (-not (Test-Path $exe)) { throw "rebuffer.exe not found: $exe" }

function Step([string]$name, [scriptblock]$body) {
    Write-Host "=== $name ==="
    try {
        & $body
    } catch {
        $line = "{0}`tERROR`t{1}" -f $name, $_.Exception.Message
        Add-Content -Path $jsonl -Value $line
        Write-Host "  ERROR: $($_.Exception.Message)"
    }
}

function Meas([string]$name, [string[]]$args) {
    $raw = & node (Join-Path $PSScriptRoot 'measure.mjs') @args 2>&1
    if ($LASTEXITCODE -ne 0) { throw "measure $name failed: $($raw -join '; ')" }
    $raw -join "`n" | Add-Content -Path $jsonl
    # echo compactly
    $last = $raw | Select-Object -Last 1
    if ($last -match '^\{') {
        $obj = $last | ConvertFrom-Json
        Write-Host "  $name -> $($obj | ConvertTo-Json -Compress -Depth 5)"
    } else {
        Write-Host "  $name -> $raw"
    }
}

# 1. Stop whatever instance is running (settings worker shares this store; the
#    app is restarted below and its stale state is never measured).
Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# 2. Seed immediately before measuring; the seed script verifies the count.
& (Join-Path $PSScriptRoot 'seed_perf.ps1')
if ($LASTEXITCODE -ne 0) { throw 'seeding failed' }

# 3. Launch with CDP.
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe)
Write-Host "launched $exe"

$deadline = (Get-Date).AddSeconds(60)
$targets = $null
while ((Get-Date) -lt $deadline) {
    try {
        $targets = & node (Join-Path $PSScriptRoot 'measure.mjs') targets 2>$null
        if ($LASTEXITCODE -eq 0 -and $targets) { break }
    } catch {}
    Start-Sleep -Milliseconds 800
}
if (-not $targets) { throw 'CDP targets never appeared' }
Write-Host "CDP up:"
$targets | ForEach-Object { Write-Host "  $_" }

# 4. Show the popup the way a user would: a second instance surfaces it via
#    the single-instance plugin (same path as the hotkey).
Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe)
Start-Sleep -Seconds 3

# 5. Wait for the grid to be populated.
$ready = $false
$deadline = (Get-Date).AddSeconds(45)
while ((Get-Date) -lt $deadline) {
    try {
        $state = & node (Join-Path $PSScriptRoot 'measure.mjs') state 2>$null
        if ($LASTEXITCODE -eq 0) {
            $obj = ($state | Select-Object -Last 1) | ConvertFrom-Json
            if ($obj.cards -gt 0) { $ready = $true; break }
        }
    } catch {}
    Start-Sleep -Milliseconds 1000
}
if (-not $ready) { throw 'popup grid never became ready' }
$state = (& node (Join-Path $PSScriptRoot 'measure.mjs') state | Select-Object -Last 1) | ConvertFrom-Json
Write-Host "popup ready: status='$($state.status)' cards=$($state.cards) scroll=$($state.scroll.scrollHeight) vis=$($state.visibility)"

# 6. Measurements.
Step 'state-baseline'  { Meas 'state-baseline' @('state') }
Step 'shot-baseline'   { Meas 'shot-baseline' @('shot', (Join-Path $out 'baseline.png')) }
Step 'mem-before'      { Meas 'mem-before' @('mem') }

Step 'scroll-slow-down' { Meas 'scroll-slow-down' @('frames', 'slow-down') }
Step 'scroll-fast-down' { Meas 'scroll-fast-down' @('frames', 'fast-down') }
Step 'headers-bottom'   { Meas 'headers-bottom' @('headers') }
Step 'shot-bottom'      { Meas 'shot-bottom' @('shot', (Join-Path $out 'bottom.png')) }
Step 'mem-after-down'   { Meas 'mem-after-down' @('mem') }

Step 'scroll-fast-up'   { Meas 'scroll-fast-up' @('frames', 'fast-up') }
Step 'headers-top'      { Meas 'headers-top' @('headers') }
Step 'mem-after-up'     { Meas 'mem-after-up' @('mem') }

Step 'scroll-middle'    { Meas 'scroll-middle' @('scrolldown', '0.35') }
Step 'headers-middle'   { Meas 'headers-middle' @('headers') }
Step 'shot-middle'      { Meas 'shot-middle' @('shot', (Join-Path $out 'middle.png')) }

Step 'zoom-in'          { Meas 'zoom-in' @('zoom', '5') }
Step 'headers-zoomed'   { Meas 'headers-zoomed' @('headers') }
Step 'shot-zoomed'      { Meas 'shot-zoomed' @('shot', (Join-Path $out 'zoomed.png')) }
Step 'zoom-out'         { Meas 'zoom-out' @('zoom', '5') }
Step 'mem-after-zoom'   { Meas 'mem-after-zoom' @('mem') }

Step 'search-type'      { Meas 'search-type' @('search', 'clipboard') }
Step 'search-clear'     { Meas 'search-clear' @('search-clear') }
Step 'mem-after-search' { Meas 'mem-after-search' @('mem') }

Step 'tabs'             { Meas 'tabs' @('tabs') }
Step 'state-final'      { Meas 'state-final' @('state') }

# 7. Stop and verify the store count one last time.
Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
$final = & sqlite3 (Join-Path $env:APPDATA 'Rebuffer\rebuffer.db') 'SELECT COUNT(*) FROM items;'
Write-Host "final store count: $final"
Add-Content -Path $jsonl -Value "store-count-after`t$final"

Write-Host "results written to $jsonl"