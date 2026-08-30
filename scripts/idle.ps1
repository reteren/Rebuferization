# Idle RAM / CPU for Rebuffer: main process plus its WebView2 children.
# Samples for DurationSeconds after a SettleSeconds quiet period. CPU% is the
# fraction of ONE logical core consumed during each interval (100% would mean
# one core fully busy).
param(
    [int]$SettleSeconds = 20,
    [int]$DurationSeconds = 120,
    [int]$IntervalMs = 5000
)
$ErrorActionPreference = 'Stop'

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
Write-Host "app pid: $($proc.Id), settling for ${SettleSeconds}s..."
Start-Sleep -Seconds $SettleSeconds

function Get-Children([int]$rootPid) {
    $all = Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'"
    $kids = @()
    $frontier = @($rootPid)
    for ($round = 0; $round -lt 4 -and $frontier.Count -gt 0; $round++) {
        $new = @($all | Where-Object { $_.ParentProcessId -in $frontier })
        if ($new.Count -eq 0) { break }
        $kids += $new
        $frontier = @($new | ForEach-Object { $_.ProcessId })
    }
    return @($kids | ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue } | Where-Object { $_ })
}

$samples = New-Object System.Collections.Generic.List[object]
$last = @{ }
$lastCpu = $proc.CPU
$lastCpuAt = [DateTime]::UtcNow

$rounds = [Math]::Max(1, [Math]::Floor($DurationSeconds * 1000 / $IntervalMs))
for ($r = 1; $r -le $rounds; $r++) {
    Start-Sleep -Milliseconds $IntervalMs
    $p = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
    if (-not $p) { Write-Host "app exited mid-measurement"; break }
    $kids = Get-Children $proc.Id
    $now = [DateTime]::UtcNow
    $dt = ($now - $lastCpuAt).TotalSeconds
    $dCpu = $p.CPU - $lastCpu
    $cpuPctOneCore = if ($dt -gt 0) { 100.0 * $dCpu / $dt } else { 0 }
    $kidsRss = ($kids | Measure-Object WorkingSet64 -Sum).Sum
    $kidsPrivate = ($kids | Measure-Object PrivateMemorySize64 -Sum).Sum
    $samples.Add([pscustomobject]@{
        t = $now.ToString('HH:mm:ss')
        mainRssMB = [Math]::Round($p.WorkingSet64 / 1MB, 1)
        mainPrivateMB = [Math]::Round($p.PrivateMemorySize64 / 1MB, 1)
        kidsCount = $kids.Count
        kidsRssMB = [Math]::Round(($kidsRss ?? 0) / 1MB, 1)
        kidsPrivateMB = [Math]::Round(($kidsPrivate ?? 0) / 1MB, 1)
        totalRssMB = [Math]::Round(($p.WorkingSet64 + ($kidsRss ?? 0)) / 1MB, 1)
        cpuPctOneCore = [Math]::Round($cpuPctOneCore, 3)
    })
    $lastCpu = $p.CPU
    $lastCpuAt = $now
    $s = $samples[-1]
    Write-Host ("{0} mainRss={1} MB kidsRss={2} MB totalRss={3} MB cpu={4}% of one core" -f $s.t, $s.mainRssMB, $s.kidsRssMB, $s.totalRssMB, $s.cpuPctOneCore)
}

function Get-Median([double[]]$vals) {
    $s = @($vals | Sort-Object)
    if ($s.Count -eq 0) { return 0 }
    if ($s.Count % 2 -eq 1) { return $s[[Math]::Floor($s.Count / 2)] }
    return ($s[$s.Count / 2 - 1] + $s[$s.Count / 2]) / 2
}

Write-Host "---- summary (n=$($samples.Count))"
$mainRss = @($samples | ForEach-Object { $_.mainRssMB })
$totalRss = @($samples | ForEach-Object { $_.totalRssMB })
$cpu = @($samples | ForEach-Object { $_.cpuPctOneCore })
$mainMax = ($mainRss | Measure-Object -Maximum).Maximum
Write-Host ("main process RSS      : median {0} MB, max {1} MB  (target < 60 MB -> {2})" -f (Get-Median $mainRss), $mainMax, $(if ($mainMax -lt 60) { 'PASS' } else { 'MISS' }))
Write-Host ("total incl. WebView2  : median {0} MB, max {1} MB" -f (Get-Median $totalRss), ($totalRss | Measure-Object -Maximum).Maximum)
Write-Host ("idle CPU (1 core pct) : median {0} %, max {1} %" -f (Get-Median $cpu), ($cpu | Measure-Object -Maximum).Maximum)
Write-Host "logical cores on machine: $([Environment]::ProcessorCount)"