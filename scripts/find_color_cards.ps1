# Scans scroll frames for color cards: tiles whose preview interior is the
# uniform --swatch-empty (#3a3f4b) color.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$targets = 58, 63, 75   # rgb(58,63,75) = #3A3F4B
$all = Get-ChildItem (Join-Path $PSScriptRoot 'sc_*.png') | Sort-Object Name
foreach ($f in $all) {
    $bmp = [System.Drawing.Bitmap]::new($f.FullName)
    $w = $bmp.Width; $h = $bmp.Height
    $hits = New-Object System.Collections.Generic.List[string]
    for ($y = 100; $y -lt $h - 10; $y += 2) {
        for ($x = 30; $x -lt $w - 10; $x += 2) {
            $c = $bmp.GetPixel($x, $y)
            if ([Math]::Abs($c.R - 58) -le 6 -and [Math]::Abs($c.G - 63) -le 6 -and [Math]::Abs($c.B - 75) -le 6) {
                $hits.Add("$x,$y")
            }
        }
    }
    if ($hits.Count -gt 0) {
        # cluster hits into cells: report the bounding box of each cluster
        $xs = @($hits | ForEach-Object { [int]($_ -split ',')[0] })
        $ys = @($hits | ForEach-Object { [int]($_ -split ',')[1] })
        $minX = ($xs | Measure-Object -Minimum).Minimum; $maxX = ($xs | Measure-Object -Maximum).Maximum
        $minY = ($ys | Measure-Object -Minimum).Minimum; $maxY = ($ys | Measure-Object -Maximum).Maximum
        Write-Output ("{0}: {1} swatch pixels, region x={2}-{3} y={4}-{5}" -f $f.Name, $hits.Count, $minX, $maxX, $minY, $maxY)
    } else {
        Write-Output ("{0}: no swatch pixels" -f $f.Name)
    }
    $bmp.Dispose()
}