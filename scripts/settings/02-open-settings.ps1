# Open the settings window the way a user does: press the real Alt+V hotkey
# (SendInput -> RegisterHotKey -> popup shows), then click the gear button in
# the popup toolbar, which calls the show_settings_window command.
# Fallback: invoke show_settings_window directly through the Tauri internals bridge.

$ErrorActionPreference = 'Stop'
$root = $env:APPDATA + '\Rebuffer'
$snap = Join-Path $PSScriptRoot 'snapshot'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null

function Get-Targets { & node "$PSScriptRoot\cdp.mjs" targets }

$targets = Get-Targets
$popup = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*index.html*' } | Select-Object -First 1)
$settings = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
if (-not $popup) { throw 'popup target not found' }
if (-not $settings) { throw 'settings target not found' }
Write-Host "popup target: $($popup.id)"
Write-Host "settings target: $($settings.id)"

# Route A: real hotkey -> popup
& "$PSScriptRoot\sendkeys.ps1" -Chord 'Alt+V'
Start-Sleep -Seconds 2
$vis = & node "$PSScriptRoot\cdp.mjs" eval $popup.id "document.visibilityState + '|' + (document.hasFocus())"
Write-Host "popup after hotkey: $vis"
& node "$PSScriptRoot\cdp.mjs" shot $popup.id "$shot\00-popup-after-hotkey.png"

# Route A continues: click the gear (aria-label "Open settings")
& node "$PSScriptRoot\cdp.mjs" eval $popup.id "(() => { const b = document.querySelector('button[aria-label=\"Open settings\"]'); if (!b) return 'NO_GEAR'; b.click(); return 'CLICKED'; })()"
Start-Sleep -Seconds 2

$visSettings = & node "$PSScriptRoot\cdp.mjs" eval $settings.id "document.visibilityState + '|' + document.title"
Write-Host "settings after gear: $visSettings"

# Fallback: show_settings_window command via internals bridge
if ($visSettings -notlike 'visible*') {
  Write-Host 'gear route failed, falling back to show_settings_window invoke'
  $r = & node "$PSScriptRoot\cdp.mjs" invoke $popup.id 'show_settings_window' '{}'
  Write-Host "invoke result: $r"
  Start-Sleep -Seconds 2
  $visSettings = & node "$PSScriptRoot\cdp.mjs" eval $settings.id "document.visibilityState"
  Write-Host "settings after invoke: $visSettings"
}

if ($visSettings -notlike 'visible*') { throw 'settings window did not become visible' }

& node "$PSScriptRoot\cdp.mjs" shot $settings.id "$shot\01-settings-general.png"
Write-Host "settings window is on screen (route A: Alt+V -> popup gear)"