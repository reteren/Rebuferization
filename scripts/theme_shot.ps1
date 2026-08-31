param([string]$Name)
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using System.Drawing;
public static class TS {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool EnumWindows(Cb cb, IntPtr l);
  public delegate bool Cb(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RC r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  public struct RC { public int L,T,R,B; }
}
'@ -ReferencedAssemblies System.Drawing
Add-Type -AssemblyName System.Drawing
$p = Start-Process "C:\Rebuferization\src-tauri\target\release\rebuffer.exe" -PassThru
Start-Sleep -Seconds 7
[TS]::keybd_event(0x12,0,0,[UIntPtr]::Zero); [TS]::keybd_event(0x56,0,0,[UIntPtr]::Zero)
[TS]::keybd_event(0x56,0,2,[UIntPtr]::Zero); [TS]::keybd_event(0x12,0,2,[UIntPtr]::Zero)
Start-Sleep -Seconds 4
$global:rows=@(); $global:tp=$p.Id
$cb=[TS+Cb]{ param($h,$l)
  $pp=0; [void][TS]::GetWindowThreadProcessId($h,[ref]$pp)
  if ($pp -eq $global:tp -and [TS]::IsWindowVisible($h)) {
    $r=New-Object TS+RC; [void][TS]::GetWindowRect($h,[ref]$r)
    if (($r.R-$r.L) -gt 400) { $global:rows += [pscustomobject]@{H=$h;W=$r.R-$r.L;Ht=$r.B-$r.T} } }
  return $true }
[void][TS]::EnumWindows($cb,[IntPtr]::Zero)
$w=$global:rows | Select-Object -First 1
if ($w) {
  $bmp = New-Object System.Drawing.Bitmap $w.W,$w.Ht
  $g=[System.Drawing.Graphics]::FromImage($bmp); $hdc=$g.GetHdc()
  [void][TS]::PrintWindow($w.H,$hdc,2); $g.ReleaseHdc($hdc); $g.Dispose()
  $bmp.Save("C:\Rebuferization\scripts\theme_$Name.png")
  $sum=0;$n=0
  for($y=60;$y -lt $w.Ht-60;$y+=17){ for($x=60;$x -lt $w.W-60;$x+=17){
    $c=$bmp.GetPixel($x,$y); $sum += ($c.R+$c.G+$c.B)/3; $n++ } }
  "{0,-13} яркость {1,3:N0}/255" -f $Name, ($sum/$n)
  $bmp.Dispose()
} else { "$Name : окно не найдено" }
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
