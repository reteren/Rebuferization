# Crops a region of a PNG, upscales, and OCRs it. For verifying small UI
# details (badges, labels) that full-page OCR smears together.
param(
    [string]$Path,
    [int]$X, [int]$Y, [int]$W, [int]$H,
    [int]$Scale = 8
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$src = [System.Drawing.Bitmap]::new($Path)
$cw = [Math]::Min($W, $src.Width - $X)
$ch = [Math]::Min($H, $src.Height - $Y)
$crop = $src.Clone((New-Object System.Drawing.Rectangle($X, $Y, $cw, $ch)), $src.PixelFormat)
$src.Dispose()

$w = $cw * $Scale; $h = $ch * $Scale
$big = [System.Drawing.Bitmap]::new($w, $h)
$g = [System.Drawing.Graphics]::FromImage($big)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
$g.DrawImage($crop, 0, 0, $w, $h)
$g.Dispose(); $crop.Dispose()
$tmp = Join-Path $env:TEMP ("ocr_{0}.png" -f ([guid]::NewGuid().ToString('N')))
$big.Save($tmp, [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()

Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]

function Await($WinRtTask, $ResultType) {
    $asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
    })[0]
    $asTask = $asTaskGeneric.MakeGenericMethod($ResultType)
    $netTask = $asTask.Invoke($null, @($WinRtTask))
    $netTask.Wait(-1) | Out-Null
    $netTask.Result
}

$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($tmp)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
$result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
foreach ($line in $result.Lines) {
    Write-Output $line.Text
}
Remove-Item $tmp -ErrorAction SilentlyContinue