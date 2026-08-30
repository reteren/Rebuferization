# Prints "popup <hwnd>|settings <hwnd>" for the running rebuffer instance.
# Usage: powershell -File find-hwnds.ps1 [-Strict]  (Strict throws if windows missing)
param([switch]$Strict)
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class EnumW2 {
  public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
  [DllImport("user32.dll")]
  public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")]
  public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetWindowTextW(IntPtr hwnd, StringBuilder sb, int max);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetClassNameW(IntPtr hwnd, StringBuilder sb, int max);
  [DllImport("user32.dll")]
  public static extern bool IsWindowVisible(IntPtr hwnd);
}
"@
$proc = Get-Process -Name rebuffer -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) {
  if ($Strict) { throw 'no rebuffer process' } else { 'popup 0|settings 0'; exit 0 }
}
$targetPid = $proc.Id
$popup = 0; $settings = 0
$cb = [EnumW2+EnumProc]{
  param($hwnd, $l)
  $p = 0
  [EnumW2]::GetWindowThreadProcessId($hwnd, [ref]$p) | Out-Null
  if ($p -eq $targetPid) {
    $sb = New-Object System.Text.StringBuilder 256
    [EnumW2]::GetWindowTextW($hwnd, $sb, 256) | Out-Null
    $sc = New-Object System.Text.StringBuilder 256
    [EnumW2]::GetClassNameW($hwnd, $sc, 256) | Out-Null
    if ($sc.ToString() -eq 'Tauri Window') {
      $t = $sb.ToString()
      if ($t -eq 'Rebuffer') { $script:popup = $hwnd.ToInt64() }
      elseif ($t -like 'Rebuffer*Settings*') { $script:settings = $hwnd.ToInt64() }
    }
  }
  return $true
}
[EnumW2]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
if ($Strict -and ($script:popup -eq 0 -or $script:settings -eq 0)) { throw "windows missing popup=$script:popup settings=$script:settings" }
"popup $script:popup|settings $script:settings"