# Precise bleed check: scans the known gap columns between cards (5-col grid
# at zoom 3: tile 116 + gap 10, grid starts at x=40) for any bright pixel.
param([string]$Path)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$bmp = [System.Drawing.Bitmap]::new($Path)
$w = $bmp.Width; $h = $bmp.Height

# find the grid start by scanning column luminance across the whole width at
# the middle of the first card row; then derive the 10px gaps
$cardLum = New-Object double[] $w
$row0 = 140; $row1 = $h - 1
for ($x = 0; $x -lt $w; $x++) {
    $s = 0.0; $n = 0
    for ($y = $row0; $y -lt $row1; $y += 3) {
        $c = $bmp.GetPixel($x, $y)
        $s += 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
        $n++
    }
    $cardLum[$x] = $s / $n
}
# gap columns: local minima ~10px wide with lum < 26
$gaps = @()
$x = 30
while ($x -lt $w - 20) {
    if ($cardLum[$x] -lt 26) {
        $x0 = $x
        while ($x -lt $w - 1 -and $cardLum[$x] -lt 26) { $x++ }
        $len = $x - $x0
        if ($len -ge 5 -and $len -le 20) { $gaps += [pscustomobject]@{ A = $x0; B = $x - 1 } }
    } else { $x++ }
}
Write-Output "gap columns detected: $($gaps | ForEach-Object { "$($_.A)-$($_.B)" })"

# for every gap, count bright pixels across the whole window height
foreach ($g in $gaps) {
    $hits = 0; $maxLum = 0; $deep = 0
    for ($y = 0; $y -lt $h; $y++) {
        for ($xx = $g.A + 1; $xx -le [Math]::Min($w - 2, $g.B - 1); $xx++) {
            $c = $bmp.GetPixel($xx, $y)
            $lum = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            if ($lum -gt 110) {
                $hits++
                if ($xx - $g.A - 1 -gt $deep) { $deep = $xx - $g.A - 1 }
            }
            if ($lum -gt $maxLum) { $maxLum = $lum }
        }
    }
    Write-Output ("gap x={0}-{1}: brightPx={2} maxLum={3} deepest={4}px -> {5}" -f $g.A, $g.B, $hits, [int]$maxLum, $deep, $(if ($hits -gt 0) { 'SUSPECT' } else { 'clean' }))
}
$bmp.Dispose()