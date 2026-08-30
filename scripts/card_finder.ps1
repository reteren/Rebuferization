# Finds card rectangles in a popup screenshot: scans for runs of the card
# surface color (distinct from the window background) and prints their bounds.
param([string]$Path)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$bmp = [System.Drawing.Bitmap]::new($Path)
$w = $bmp.Width; $h = $bmp.Height

# classify each pixel: is it "card surface" (dark but lighter than the page bg)?
function Is-CardPixel([int]$x, [int]$y) {
    $c = $bmp.GetPixel($x, $y)
    $r = $c.R; $g = $c.G; $b = $c.B
    # page bg ~ #121418 (18,20,24); card surface ~ #1c1f26 (28,31,38)
    if ($r -ge 22 -and $r -le 42 -and $g -ge 24 -and $g -le 46 -and $b -ge 30 -and $b -le 52) { return $true }
    # scrim/border variations
    if ($r -ge 20 -and $r -le 60 -and $g -ge 22 -and $g -le 64 -and $b -ge 28 -and $b -le 70 -and [Math]::Abs($r - $g) -lt 14 -and [Math]::Abs($g - $b) -lt 14) { return $true }
    return $false
}

# row profile: fraction of card pixels per row
for ($y = 0; $y -lt $h; $y += 3) {
    $cardPx = 0; $n = 0
    for ($x = 0; $x -lt $w; $x += 3) { if (Is-CardPixel $x $y) { $cardPx++ }; $n++ }
    $frac = $cardPx / $n
    if ($frac -gt 0.35) { Write-Output ("y={0,4}: card {1,3:P0}" -f $y, $frac) }
}
$bmp.Dispose()