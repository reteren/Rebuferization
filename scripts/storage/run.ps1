# W38 orchestrator. Sequences the five storage-volume phases against a VHD it
# creates and owns (drive W:). Touches only: scripts/storage/, the W: VHD,
# and %TEMP%\opencode\w38. It never launches rebuffer.exe and never reads or
# writes %APPDATA%\Rebuffer, so the live user is undisturbed.
#
# Usage: .\run.ps1
$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$harness = Join-Path $here 'target\debug\storage-harness.exe'
$out = Join-Path $here 'out'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$log = Join-Path $out 'log.txt'
$marker = Join-Path $here 'phase3.marker.txt'
$temp = 'C:\Users\reteren\AppData\Local\Temp\opencode\w38'
New-Item -ItemType Directory -Path $temp -Force | Out-Null
Remove-Item -Force $log, $marker -ErrorAction SilentlyContinue

function Log([string]$m) { $m | Tee-Object -FilePath $log -Append | Write-Host }

function Run([string]$name, [string[]]$argsArr) {
    Log "### $name"
    $raw = & $harness @argsArr 2>&1
    if ($LASTEXITCODE -ne 0) { Log "  harness exit code: $LASTEXITCODE" }
    $raw | Tee-Object -FilePath $log -Append | Write-Host
}

function WaitFor([string]$file, [string]$needle, [int]$timeoutSec) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $file) {
            $c = Get-Content $file -Raw -ErrorAction SilentlyContinue
            if ($c -and $c -match $needle) { return $true }
        }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

function Cleanup-Before {
    Log "### cleanup-before"
    & (Join-Path $here 'vhd.ps1') remove
    Remove-Item -Recurse -Force "$temp\reloc-src", "$temp\reloc-src2", "$temp\reloc-dst" -ErrorAction SilentlyContinue
}

# Build the harness once.
if (-not (Test-Path $harness)) {
    Log "### build harness"
    & cargo build --offline --manifest-path (Join-Path $here 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'harness build failed' }
}

Cleanup-Before

# ============================================================================
# PHASE 1 — FULL VOLUME ON CAPTURE
# ============================================================================
Log '=== PHASE 1: full volume on capture ==='
& (Join-Path $here 'vhd.ps1') new
Run 'P1-seed-small' @('seed', 'W:\store', '5')
& (Join-Path $here 'vhd.ps1') fill -LeaveBytes 8192
$free = (& (Join-Path $here 'vhd.ps1') free) -join ''
Log "P1 free bytes after fill: $free"
Run 'P1-insert-1MB' @('insert', 'W:\store', '1048576')
Run 'P1-insert-4KB' @('insert', 'W:\store', '4096')
Run 'P1-insert-256B' @('insert', 'W:\store', '256')
& (Join-Path $here 'vhd.ps1') unfill

# ============================================================================
# PHASE 2 — JANITOR ON A FULL VOLUME
# ============================================================================
# Note: neither NTFS nor FAT32 lets a regular file drive free space below a
# ~128KB metadata floor, so "full" here means ~128KB free on a 128MB volume.
# Small deletes fit in that headroom; the interesting case is a delete whose
# WAL growth exceeds it, which the big-inline-format items below force.
Log '=== PHASE 2: janitor on a full volume ==='
# 2a. age sweep, plain small items, volume at its floor
Run 'P2a-seed-15' @('seed', 'W:\store', '15')
Run 'P2a-backdate-all-31d' @('backdate', 'W:\store', '31')
& (Join-Path $here 'vhd.ps1') fill -LeaveBytes 0 -Letter W
$free = (& (Join-Path $here 'vhd.ps1') free -Letter W) -join ''
Log "P2a free bytes after fill: $free"
Run 'P2a-cleanup-age-full' @('cleanup', 'W:\store', '30', 'none')
& (Join-Path $here 'vhd.ps1') unfill -Letter W

# 2b. age sweep, big-inline-format items (delete needs > floor worth of WAL)
Run 'P2b-seed-5-formats' @('seed-formats', 'W:\store', '5', '60000')
Run 'P2b-backdate-all-31d' @('backdate', 'W:\store', '31')
& (Join-Path $here 'vhd.ps1') fill -LeaveBytes 0 -Letter W
$free = (& (Join-Path $here 'vhd.ps1') free -Letter W) -join ''
Log "P2b free bytes after fill: $free"
Run 'P2b-cleanup-age-format-full' @('cleanup', 'W:\store', '30', 'none')
& (Join-Path $here 'vhd.ps1') unfill -Letter W

# 2c. size-cap prune, big-inline-format items, volume at its floor
#     (cap must be BELOW the summed primary byte_size to actually trigger)
Run 'P2c-seed-5-formats' @('seed-formats', 'W:\store', '5', '60000')
& (Join-Path $here 'vhd.ps1') fill -LeaveBytes 0 -Letter W
$free = (& (Join-Path $here 'vhd.ps1') free -Letter W) -join ''
Log "P2c free bytes after fill: $free"
Run 'P2c-cleanup-cap-format-full' @('cleanup', 'W:\store', '30', '300')
& (Join-Path $here 'vhd.ps1') unfill -Letter W

# 2d. recovery: with space back, cleanup succeeds
Run 'P2d-cleanup-after-unfill' @('cleanup', 'W:\store', '30', '300')

# ============================================================================
# PHASE 3 — VOLUME DISCONNECTED WHILE RUNNING
# ============================================================================
Log '=== PHASE 3: volume disconnected while running ==='
Run 'P3-preseed' @('seed', 'W:\store', '20')
Remove-Item -Force "$out\p3.out.txt", "$out\p3.err.txt" -ErrorAction SilentlyContinue
$p = Start-Process -FilePath $harness `
    -ArgumentList @('disconnect', 'W:\store', $marker) `
    -RedirectStandardOutput "$out\p3.out.txt" `
    -RedirectStandardError "$out\p3.err.txt" `
    -PassThru -NoNewWindow
$ready = WaitFor "$out\p3.out.txt" 'READY_BEFORE_DISMOUNT' 30
if (-not $ready) {
    Log 'P3 harness never became ready; killing'
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
} else {
    Log 'P3 harness ready; dismounting'
    & (Join-Path $here 'vhd.ps1') dismount
    Set-Content -Path $marker -Value 'dismounted'
    # Wait for the harness to finish its post-dismount operations (it prints
    # post_dismount_reopen after the last one), then bring the volume back.
    $done = WaitFor "$out\p3.out.txt" 'post_dismount_reopen' 40
    if (-not $done) {
        Log 'P3 harness stuck after dismount; killing'
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    } else {
        Log 'P3 re-mounting'
        & (Join-Path $here 'vhd.ps1') mount
        Set-Content -Path $marker -Value 'remounted'
        Wait-Process -Id $p.Id -Timeout 60 -ErrorAction SilentlyContinue
    }
}
if (Test-Path "$out\p3.out.txt") {
    Log '--- P3 harness output ---'
    Get-Content "$out\p3.out.txt" | Tee-Object -FilePath $log -Append | Write-Host
}
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }

# ============================================================================
# PHASE 4 — VOLUME MISSING AT STARTUP
# ============================================================================
Log '=== PHASE 4: volume missing at startup ==='
& (Join-Path $here 'vhd.ps1') dismount
Run 'P4-store-open-missing-W' @('store-open', 'W:\store')
Run 'P4-store-open-missing-W-root' @('store-open', 'W:\')
Run 'P4-store-open-missing-Z' @('store-open', 'Z:\no-such-drive\store')
Run 'P4-store-open-missing-subdir' @('store-open', "$temp\no-such-dir\store")
& (Join-Path $here 'vhd.ps1') mount

# ============================================================================
# PHASE 5 — RELOCATION TO A TOO-SMALL TARGET
# ============================================================================
Log '=== PHASE 5: relocation ==='
$relocSrc = "$temp\reloc-src"
Run 'P5-seed-src' @('seed', $relocSrc, '40')
& (Join-Path $here 'vhd.ps1') fill -LeaveBytes 8192
$free = (& (Join-Path $here 'vhd.ps1') free) -join ''
Log "P5 target free bytes: $free"
Run 'P5-relocate-to-small-target' @('relocate', $relocSrc, 'W:\reloc-target')
Run 'P5-src-intact-after-refuse' @('store-open', $relocSrc)
# Re-run the same relocate but with the target directory PRE-CREATED — the
# free-space check (GetDiskFreeSpaceExW) behaves differently for an existing
# path, so this isolates whether SPEC 3.5's refusal works at all.
New-Item -ItemType Directory -Path 'W:\reloc-target-preexisting' -Force | Out-Null
Run 'P5-relocate-preexisting-target' @('relocate', $relocSrc, 'W:\reloc-target-preexisting')
Run 'P5-src-intact-after-preexisting' @('store-open', $relocSrc)
& (Join-Path $here 'vhd.ps1') unfill
$relocSrc2 = "$temp\reloc-src2"
Run 'P5-seed-src2' @('seed', $relocSrc2, '30')
Run 'P5-relocate-to-roomy-target' @('relocate', $relocSrc2, 'W:\reloc-target2')
Run 'P5-relocate-to-missing-drive' @('relocate', $relocSrc, 'Z:\no-such-drive\target')

# ============================================================================
# CLEANUP
# ============================================================================
Log '=== cleanup ==='
& (Join-Path $here 'vhd.ps1') remove
Remove-Item -Recurse -Force "$temp" -ErrorAction SilentlyContinue
Log "done. log: $log"