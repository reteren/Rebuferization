# Captures the popup window via PrintWindow(PW_RENDERFULLCONTENT), which asks
# the window to paint itself including WebView2 content. Saves PNG to
# scripts/<name>.png. Falls back to a screen-region copy if PrintWindow returns
# an all-black frame.
param(
    [string]$Name = 'popup',
    [switch]$FromScreen
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')
Add-Type -AssemblyName System.Drawing

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }
if (-not [Win32]::IsWindowVisible($hwnd)) { throw 'popup is not visible' }

$rect = New-Object Win32+RECT
$null = [Win32]::GetWindowRect($hwnd, [ref]$rect)
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
Write-Host "window rect: $($rect.Left),$($rect.Top) ${w}x${h}"

$out = Join-Path $PSScriptRoot "$Name.png"
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [Win32]::PrintWindow($hwnd, $hdc, [Win32]::PW_RENDERFULLCONTENT)
$g.ReleaseHdc($hdc)
$g.Dispose()

$nonBlack = 0
$total = 0
for ($y = 0; $y -lt $h; $y += 4) {
    for ($x = 0; $x -lt $w; $x += 4) {
        $c = $bmp.GetPixel($x, $y)
        $total++
        if ($c.R -gt 8 -or $c.G -gt 8 -or $c.B -gt 8) { $nonBlack++ }
    }
}
$pct = if ($total -gt 0) { [Math]::Round(100.0 * $nonBlack / $total, 1) } else { 0 }
Write-Host "PrintWindow ok=$ok non-black=$pct%"

if ($ok -and $pct -gt 5) {
    $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "saved $out"
} elseif ($FromScreen -or -not $ok -or $pct -le 5) {
    $bmp.Dispose()
    $screen = New-Object System.Drawing.Bitmap($w, $h)
    $g2 = [System.Drawing.Graphics]::FromImage($screen)
    $g2.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($w, $h)))
    $g2.Dispose()
    $screen.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "screen-copy fallback saved $out"
    $screen.Dispose()
}