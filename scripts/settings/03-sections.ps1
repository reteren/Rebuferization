# Click each section nav button, screenshot the settings window, and capture
# the section's heading + control inventory as evidence.
$ErrorActionPreference = 'Stop'
$shot = Join-Path $PSScriptRoot 'shots'
New-Item -ItemType Directory -Path $shot -Force | Out-Null

$targets = & node "$PSScriptRoot\cdp.mjs" targets
$settings = ($targets | ConvertFrom-Json | Where-Object { $_.url -like '*settings.html*' } | Select-Object -First 1)
if (-not $settings) { throw 'settings target not found' }

$sections = @('General', 'Storage', 'Appearance', 'Privacy', 'Data', 'About')
foreach ($s in $sections) {
  $click = "(() => { const btns = [...document.querySelectorAll('nav button')]; const b = btns.find(x => x.textContent.trim() === '$s'); if (!b) return 'NO_BTN'; b.click(); return 'CLICKED'; })()"
  $r = & node "$PSScriptRoot\cdp.mjs" eval $settings.id $click
  Start-Sleep -Milliseconds 700
  $info = & node "$PSScriptRoot\cdp.mjs" eval $settings.id "(() => { const h = document.querySelector('.content h2'); const controls = [...document.querySelectorAll('.content input, .content select, .content button, .content .meter, .content .legend li')].map(e => e.tagName + (e.getAttribute('aria-label') ? '[' + e.getAttribute('aria-label') + ']' : '') + (e.type ? ':' + e.type : '') + ':' + (e.textContent || e.value || '').trim().replace(/\s+/g, ' ').slice(0, 60)).join('\n'); return (h ? h.textContent : 'NO_H2') + '\n---CONTROLS---\n' + controls; })()"
  $safe = $s.ToLower()
  $info | Out-File (Join-Path $PSScriptRoot "shots\03-$safe-inventory.txt") -Encoding utf8
  & node "$PSScriptRoot\cdp.mjs" shot $settings.id "$shot\03-$safe.png"
  Write-Host "section $s : $r | $($info.Split("`n")[0])"
}