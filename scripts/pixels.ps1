# Pixel-level analysis of a popup screenshot: dominant colors, background
# tone, and where the bright/dark regions are. Helps decide whether the popup
# shows the app UI (dark theme) or a browser error page (usually light).
param([string]$Path)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$bmp = [System.Drawing.Bitmap]::new($Path)
$w = $bmp.Width; $h = $bmp.Height
Write-Output ("size: {0}x{1}" -f $w, $h)

$hist = @{}
$step = 3
for ($y = 0; $y -lt $h; $y += $step) {
    for ($x = 0; $x -lt $w; $x += $step) {
        $c = $bmp.GetPixel($x, $y)
        $lum = [int](0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B)
        $bucket = [Math]::Floor($lum / 16) * 16
        $key = "{0}-{1}-{2}" -f ([int]($c.R / 48) * 48), ([int]($c.G / 48) * 48), ([int]($c.B / 48) * 48)
        $hist[$key] = 1 + ($hist[$key] ?? 0)
    }
}
$total = $hist.Values | Measure-Object -Sum
Write-Output ("buckets: {0}" -f $hist.Count)
$hist.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 12 | ForEach-Object {
    $pct = 100.0 * $_.Value / $total.Sum
    Write-Output ("{0,6:N1}%  rgb~{1}" -f $pct, $_.Key)
}

$mid = $bmp.GetPixel([int]($w/2), [int]($h/2))
Write-Output ("center pixel: R={0} G={1} B={2}" -f $mid.R, $mid.G, $mid.B)

$rows = @()
for ($y = 0; $y -lt $h; $y += 20) {
    $lumSum = 0.0; $n = 0
    for ($x = 0; $x -lt $w; $x += 10) {
        $c = $bmp.GetPixel($x, $y)
        $lumSum += 0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B
        $n++
    }
    $rows += ("y={0,4}: avg lum {1,5:N0}" -f $y, ($lumSum / $n))
}
$rows | ForEach-Object { Write-Output $_ }
$bmp.Dispose()