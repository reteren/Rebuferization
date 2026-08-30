Add-Type -AssemblyName System.Drawing
$a = [System.Drawing.Bitmap].Assembly
Write-Output "assembly: $($a.FullName)"
Write-Output "location: $($a.Location)"
$a.GetTypes() | Where-Object { $_.FullName -match 'Imaging' -or $_.FullName -match 'ImageFormat' } | Select-Object -First 8 | ForEach-Object { Write-Output "type: $($_.FullName)" }
Write-Output "ImageFormat resolved: $([System.Drawing.ImageFormat])"