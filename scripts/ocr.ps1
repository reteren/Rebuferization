# OCR of a PNG screenshot via Windows.Media.Ocr (WinRT), run under Windows
# PowerShell 5.1. Upscales 3x for better small-text recognition.
param([string]$Path, [int]$Scale = 3)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

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

$src = [System.Drawing.Bitmap]::new($Path)
$w = $src.Width * $Scale; $h = $src.Height * $Scale
$big = [System.Drawing.Bitmap]::new($w, $h)
$g = [System.Drawing.Graphics]::FromImage($big)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
$g.DrawImage($src, 0, 0, $w, $h)
$g.Dispose(); $src.Dispose()
$tmp = Join-Path $env:TEMP ("ocr_{0}.png" -f ([guid]::NewGuid().ToString('N')))
$big.Save($tmp, [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()

$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($tmp)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])

$engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
$result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
Write-Output ("OCR language: {0}; lines: {1}" -f $engine.RecognizerLanguage.LanguageTag, $result.Lines.Count)
foreach ($line in $result.Lines) {
    try {
        $r = $line.Words[0].BoundingRect
        $x = [int]($r.X) / $Scale
        $y = [int]($r.Y) / $Scale
        Write-Output ("[{0,4}x{1,4}] {2}" -f $x, $y, $line.Text)
    } catch {
        Write-Output ("[?????] {0}" -f $line.Text)
    }
}
Remove-Item $tmp -ErrorAction SilentlyContinue