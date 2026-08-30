# List all top-level windows of the rebuffer process with title, class, visibility.
Add-Type -AssemblyName UIAutomationClient
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class EnumW {
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
  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr hwnd, out RECT r);
  public struct RECT { public int L, T, R, B; }
}
"@

$targetPid = (Get-Process -Name rebuffer -ErrorAction SilentlyContinue | Select-Object -First 1).Id
Write-Output "pid=$targetPid"
$rows = New-Object System.Collections.ArrayList
$cb = [EnumW+EnumProc]{
  param($hwnd, $l)
  $p = 0
  [EnumW]::GetWindowThreadProcessId($hwnd, [ref]$p) | Out-Null
  if ($p -eq $targetPid) {
    $sb = New-Object System.Text.StringBuilder 256
    [EnumW]::GetWindowTextW($hwnd, $sb, 256) | Out-Null
    $sc = New-Object System.Text.StringBuilder 256
    [EnumW]::GetClassNameW($hwnd, $sc, 256) | Out-Null
    $r = New-Object EnumW+RECT
    [EnumW]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    $row = [pscustomobject]@{ Hwnd = $hwnd.ToInt64(); Title = $sb.ToString(); Class = $sc.ToString(); Visible = [EnumW]::IsWindowVisible($hwnd); W = $r.R - $r.L; H = $r.B - $r.T }
    [void]$rows.Add($row)
  }
  return $true
}
[EnumW]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$rows | Format-Table -AutoSize | Out-String -Width 240 | Write-Output