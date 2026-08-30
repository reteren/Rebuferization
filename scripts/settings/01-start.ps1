# Launch rebuffer with a WebView2 CDP debugging port and wait for targets.
# Uses the real show path: real Alt+V via SendInput -> popup -> gear button -> settings.

$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$snap = Join-Path $PSScriptRoot 'snapshot'
New-Item -ItemType Directory -Path $snap -Force | Out-Null

# 1. Snapshot pre-test state
Copy-Item "$root\settings.json" "$snap\settings.before.json" -Force
Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Rebuffer' -ErrorAction SilentlyContinue |
  Select-Object -ExpandProperty Rebuffer | Set-Content "$snap\runkey.before.txt" -NoNewline -ErrorAction SilentlyContinue

# 2. Ensure no instance is running
$running = Get-Process -Name rebuffer -ErrorAction SilentlyContinue
if ($running) {
  Write-Host "killing existing rebuffer pid(s): $($running.Id -join ',')"
  $running | Stop-Process -Force
  Start-Sleep -Seconds 2
}

# 3. Mark the log position so later steps can diff new lines
$log = Get-ChildItem "$root\logs\rebuffer.log.*" | Sort-Object LastWriteTime | Select-Object -Last 1
if ($log) { (Get-Content $log.FullName).Count | Set-Content "$snap\loglines.before.txt" }

# 4. Launch with CDP port
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333'
$exe = Join-Path $PSScriptRoot '..\..\src-tauri\target\debug\rebuffer.exe'
Start-Process -FilePath (Resolve-Path $exe) -WorkingDirectory (Split-Path (Resolve-Path $exe))
Write-Host "launched $exe"

# 5. Wait for CDP targets
$deadline = (Get-Date).AddSeconds(45)
$targets = $null
while ((Get-Date) -lt $deadline) {
  try {
    $targets = & node "$PSScriptRoot\cdp.mjs" targets
    if ($LASTEXITCODE -eq 0 -and $targets) { break }
  } catch {}
  Start-Sleep -Milliseconds 1000
}
if (-not $targets) { throw 'CDP targets never appeared' }
$targets | Out-File "$snap\targets.start.txt"
Write-Host "CDP up:"
$targets