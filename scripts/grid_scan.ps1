# Detects the card grid in a 1:1 popup capture and checks for text bleeding
# past card right edges, plus locates orange/blue regions (badges, color card).
param([string]$Path, [int]$BandY0 = 160, [int]$BandY1 = 280)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$bmp = [System.Drawing.Bitmap]::new($Path)
$w = $bmp.Width; $h = $bmp.Height
Write-Output ("size: {0}x{1}" -f $w, $h)

# per-column mean luminance in the band
$colLum = New-Object double[] $w
for ($x = 0; $x -lt $w; $x++) {
    $s = 0.0; $n = 0
    for ($y = $BandY0; $y -lt $BandY1; $y += 2) {
        $c = $bmp.GetPixel($x, $y)
        $s += 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
        $n++
    }
    $colLum[$x] = $s / $n
}

# classify: gap (low lum, <28) vs card (>28). Print a compact strip.
$run = ''; $segments = @()
$prev = '?'; $start = 0
for ($x = 0; $x -lt $w; $x++) {
    $cls = if ($colLum[$x] -lt 28) { '.' } else { '#' }
    if ($cls -ne $prev) {
        if ($prev -ne '?') { $segments += [pscustomobject]@{ From = $start; To = $x - 1; Cls = $prev } }
        $start = $x; $prev = $cls
    }
}
$segments += [pscustomobject]@{ From = $start; To = $w - 1; Cls = $prev }
foreach ($s in $segments) {
    $len = $s.To - $s.From + 1
    $run += ("{0}:{1}-{2}({3}) " -f $s.Cls, $s.From, $s.To, $len)
}
Write-Output ("segments: $run")

# card right edges = end of a '#' segment, next segment a '.' of ~10px
$cardEdges = @()
for ($i = 0; $i -lt $segments.Count - 1; $i++) {
    if ($segments[$i].Cls -eq '#' -and $segments[$i + 1].Cls -eq '.') {
        $cardEdges += $segments[$i].To
    }
}
Write-Output ("candidate card right edges: $($cardEdges -join ', ')")

# for each card edge, scan the gap (edge+1 .. edge+9) for bright pixels
foreach ($e in $cardEdges) {
    $hits = 0; $maxLum = 0; $deepest = 0
    for ($y = $BandY0; $y -lt $BandY1; $y++) {
        for ($x = $e + 1; $x -le [Math]::Min($w - 1, $e + 9); $x++) {
            $c = $bmp.GetPixel($x, $y)
            $lum = 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
            if ($lum -gt 120) { $hits++; if ($x - $e -gt $deepest) { $deepest = $x - $e } }
            if ($lum -gt $maxLum) { $maxLum = $lum }
        }
    }
    Write-Output ("edge at x={0}: bright-pixels-in-gap={1} maxLum={2} deepest={3}px -> {4}" -f $e, $hits, [int]$maxLum, $deepest, $(if ($hits -gt 0) { 'BLEED?' } else { 'clean' }))
}

# locate orange and bright-blue clusters
$orange = 0; $blue = 0; $orangeAt = @()
for ($y = 0; $y -lt $h; $y += 2) {
    for ($x = 0; $x -lt $w; $x += 2) {
        $c = $bmp.GetPixel($x, $y)
        if ($c.R -gt 180 -and $c.G -gt 90 -and $c.G -lt 200 -and $c.B -lt 90) {
            $orange++; if ($orangeAt.Count -lt 20) { $orangeAt += "($x,$y)" }
        }
    }
}
Write-Output "orange px (sampled): $orange  at: $($orangeAt -join ' ')"
$bmp.Dispose()