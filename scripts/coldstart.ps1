# Cold start: launch rebuffer.exe and timestamp the observable milestones.
# t0 = process start. Then poll for (a) any top-level window titled "Rebuffer"
# (the hidden popup: WebView2 up), (b) the settings window, and correlate the
# app's own log line "hotkey ... registered" (last backend step before tray
# install) by matching its timestamp against t0.
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')

Get-Process rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800

$exe = Join-Path $PSScriptRoot '..\src-tauri\target\debug\rebuffer.exe'
$log = Join-Path $env:APPDATA 'Rebuffer\logs'

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $exe -PassThru
$tProc = $sw.ElapsedMilliseconds
Write-Host "process spawned: $tProc ms (pid $($proc.Id))"

$popupMs = -1; $settingsMs = -1
$poll = 0
while ($poll -lt 20000) {
    if ($proc.HasExited) { Write-Host "process exited early (code $($proc.ExitCode))"; break }
    if ($popupMs -lt 0 -and [Win32]::FindAnyWindowByTitle('Rebuffer', $proc.Id) -ne [IntPtr]::Zero) {
        $popupMs = $sw.ElapsedMilliseconds
        Write-Host "popup window exists (hidden): $popupMs ms"
    }
    if ($settingsMs -lt 0 -and [Win32]::FindAnyWindowByTitle('Rebuffer — Settings', $proc.Id) -ne [IntPtr]::Zero) {
        $settingsMs = $sw.ElapsedMilliseconds
        Write-Host "settings window exists (hidden): $settingsMs ms"
    }
    if ($popupMs -ge 0 -and $settingsMs -ge 0) { break }
    Start-Sleep -Milliseconds 10
    $poll += 10
}

$elapsed = $sw.ElapsedMilliseconds
Write-Host "----"
Write-Host ("process spawn       : {0} ms" -f $tProc)
Write-Host ("popup window exists : {0} ms" -f $popupMs)
Write-Host ("settings window     : {0} ms" -f $settingsMs)
Write-Host ("elapsed at last poll: {0} ms" -f $elapsed)

$logFile = Get-ChildItem $log -Filter 'rebuffer.log.*' -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($logFile) {
    $t0Utc = [DateTime]::UtcNow.AddMilliseconds(-$sw.ElapsedMilliseconds)
    $startLine = $null; $hotkeyLine = $null
    for ($try = 0; $try -lt 5 -and -not $hotkeyLine; $try++) {
        Start-Sleep -Milliseconds 400
        $lines = Get-Content $logFile.FullName -Tail 40
        $cand = $lines | Where-Object { [DateTimeOffset]::Parse([regex]::Match($_, '^\S+').Value).UtcDateTime -ge $t0Utc }
        $startLine = $cand | Where-Object { $_ -match 'rebuffer starting' } | Select-Object -Last 1
        $hotkeyLine = $cand | Where-Object { $_ -match 'hotkey .* registered' } | Select-Object -Last 1
    }
    if ($startLine -and $hotkeyLine) {
        $tsStart = [DateTimeOffset]::Parse([regex]::Match($startLine, '^\S+').Value).UtcDateTime
        $tsHot = [DateTimeOffset]::Parse([regex]::Match($hotkeyLine, '^\S+').Value).UtcDateTime
        Write-Host ("log 'rebuffer starting'  : {0} ms after spawn" -f ($tsStart - $t0Utc).TotalMilliseconds)
        Write-Host ("log 'hotkey registered'  : {0} ms after spawn (tray install follows immediately)" -f ($tsHot - $t0Utc).TotalMilliseconds)
        Write-Host ("'starting' -> 'hotkey'   : {0} ms" -f ($tsHot - $tsStart).TotalMilliseconds)
    } else {
        Write-Host "log lines for this launch not found (log may be from an earlier launch)"
    }
}