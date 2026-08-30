# Restore the pre-test state: settings.json, HKCU Run key, test items, temp
# export archive. Then leave the app stopped (the other worker can relaunch).
$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$snap = Join-Path $PSScriptRoot 'snapshot'
$testItems = Join-Path $snap 'test-items.txt'

Get-Process -Name rebuffer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 2

# settings.json
Copy-Item (Join-Path $snap 'settings.before.json') "$root\settings.json" -Force
Write-Host 'settings.json restored'

# Run key
$before = Get-Content (Join-Path $snap 'runkey.before.txt') -ErrorAction SilentlyContinue
Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Rebuffer' -ErrorAction SilentlyContinue
if ($before) {
  Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'Rebuffer' -Value $before
  Write-Host "Run key restored: $before"
} else {
  Write-Host 'Run key absent before the run; removed.'
}

# delete test items we created (rows only; orphan blobs are swept by startup_sweep)
if (Test-Path $testItems) {
  $ids = Get-Content $testItems | ForEach-Object { ($_ -split "`t")[1] } | Where-Object { $_ } | Sort-Object -Unique
  if ($ids) {
    $list = ($ids -join ',')
    $rows = & sqlite3 "$root\rebuffer.db" "SELECT COUNT(*) FROM items WHERE id IN ($list)"
    & sqlite3 "$root\rebuffer.db" "DELETE FROM items WHERE id IN ($list)"
    Write-Host "deleted $rows test rows (ids: $list)"
  }
}

# temp export artifacts
Remove-Item (Join-Path $env:TEMP 'rbf-verify') -Recurse -ErrorAction SilentlyContinue
Write-Host 'temp export artifacts removed'