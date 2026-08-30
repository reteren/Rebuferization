# Tight measurement pass: assumes the store is already seeded at 10,000
# (verifies), kills any instance, relaunches via the CDP-capable path
# (scheduled task -> WebView2 149 fixed runtime, see docs/PERF-UI.md),
# shows the popup, resizes it, runs the full W26 sequence, saves
# out/results.jsonl + screenshots, and stops the app.
#
# Usage: .\run_measured.ps1

$ErrorActionPreference = 'Stop'
$root = Join-Path $PSScriptRoot '..\..'
$exe = Join-Path $root 'src-tauri\target\debug\rebuffer.exe'
$out = Join-Path $PSScriptRoot 'out'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$jsonl = Join-Path $out 'results.jsonl'
if (Test-Path $jsonl) { Remove-Item $jsonl -Force }

function Step([string]$name, [scriptblock]$body) {
    Write-Host "=== $name ==="
    try { & $body } catch {
        Add-Content -Path $jsonl -Value ("{0}`tERROR`t{1}" -f $name, $_.Exception.Message)
        Write-Host "  ERROR: $($_.Exception.Message)"
    }
}

function Meas([string]$name, [string[]]$cmdArgs) {
    $node = Join-Path $PSScriptRoot 'measure.mjs'
    $procArgs = @($node)
    foreach ($a in $cmdArgs) { $procArgs += $a }
    $raw = & node $procArgs 2>&1
    if ($LASTEXITCODE -ne 0) { throw "measure $name failed: $($raw -join '; ')" }
    $raw -join "`n" | Add-Content -Path $jsonl
    $last = $raw | Select-Object -Last 1
    if ($last -match '^\{') {
        $obj = $last | ConvertFrom-Json
        Write-Host "  $name -> $($obj | ConvertTo-Json -Compress -Depth 4)"
    } else { Write-Host "  $name -> $raw" }
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class FPH {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
    public static IntPtr FindByPid(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid) { var sb = new System.Text.StringBuilder(256); GetWindowText(h, sb, 256); if (sb.ToString() == "Rebuffer") { found = h; return false; } } return true; }, IntPtr.Zero);
        return found;
    }
    public static void Resize(IntPtr h) { SetWindowPos(h, IntPtr.Zero, 150, 80, 1100, 720, 0x0004); }
}
'@

# 0. Verify the seed.
$count = & sqlite3 (Join-Path $env:APPDATA 'Rebuffer\rebuffer.db') 'SELECT COUNT(*) FROM items;'
Write-Host "store count: $count"
if ([int]$count -ne 10000) { throw "store is not at 10,000 — re-seed first (seed_perf.ps1)" }

# 1. Take the app.
Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 3

# 2. Launch via the scheduled task (WebView2 149 + CDP port 9333; a direct
#    elevated Start-Process runs the 151 runtime which drops the port).
schtasks /run /tn "rbf-cdp" | Out-Null
Start-Sleep -Seconds 10

$deadline = (Get-Date).AddSeconds(30)
$cdp = $false
while ((Get-Date) -lt $deadline) {
    try { $null = Invoke-RestMethod 'http://127.0.0.1:9333/json/version' -TimeoutSec 2; $cdp = $true; break } catch {}
    Start-Sleep -Milliseconds 800
}
if (-not $cdp) { throw 'CDP port never came up' }
Write-Host "CDP up"

# 3. Show the popup through the real user path (Alt+V hotkey). The
#    second-instance single-instance-plugin trick is NOT usable here: the
#    second process starts its own WebView2 browser, fails to bind the CDP
#    port the first instance holds, and fail-fasts (0xc0000409) — which also
#    produces a WER crash event for rebuffer.exe.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class KE {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
'@
[KE]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
[KE]::keybd_event(0x56, 0, 0, [UIntPtr]::Zero)
[KE]::keybd_event(0x56, 0, 2, [UIntPtr]::Zero)
[KE]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Seconds 4

# 4. Resize the popup for a realistic viewport.
$p = Get-Process rebuffer | Select-Object -First 1
$h = [FPH]::FindByPid([uint32]$p.Id)
if ($h -ne [IntPtr]::Zero) { [FPH]::Resize($h) }
Start-Sleep -Seconds 2

# 5. Wait for the grid to populate.
$ready = $false
$deadline = (Get-Date).AddSeconds(40)
while ((Get-Date) -lt $deadline) {
    try {
        $state = (& node (Join-Path $PSScriptRoot 'measure.mjs') state 2>$null | Select-Object -Last 1) | ConvertFrom-Json
        if ($state.cards -gt 0 -and $state.status -match '10,000') { $ready = $true; break }
        if ($state.cards -gt 0) { $ready = $true; break }
    } catch {}
    Start-Sleep -Milliseconds 1000
}
if (-not $ready) { throw 'popup grid never became ready' }
Write-Host "popup ready: status='$($state.status)' cards=$($state.cards) scroll=$($state.scroll.scrollHeight)"

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

# 7. Stop and verify.
Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
$final = & sqlite3 (Join-Path $env:APPDATA 'Rebuffer\rebuffer.db') 'SELECT COUNT(*) FROM items;'
Add-Content -Path $jsonl -Value "store-count-after`t$final"
Write-Host "final store count: $final"
Write-Host "results: $jsonl"