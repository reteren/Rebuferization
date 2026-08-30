# Interactive driver over Win32 + UIA. Subcommands:
#   drv.ps1 win -Title "Rebuffer — Settings"
#   drv.ps1 shot -Hwnd <n> -Out <file.png>
#   drv.ps1 tree -Hwnd <n> [-Out <file>]
#   drv.ps1 click -Hwnd <n> -Name <name> [-Type Button]
#   drv.ps1 toggle -Hwnd <n> -Name <namePart>
#   drv.ps1 state -Hwnd <n> -Name <namePart>
#   drv.ps1 edits -Hwnd <n>
#   drv.ps1 setedit -Hwnd <n> -N <idx> -Value <v>
#   drv.ps1 combo -Hwnd <n> -Name <namePart> -Pick <value>
#   drv.ps1 keys -Chord "Alt+Shift+Z"
#   drv.ps1 clickpt -X <x> -Y <y>
param(
  [Parameter(Mandatory)][string]$Verb,
  [string]$Hwnd = '',
  [string]$Title,
  [string]$Name,
  [string]$Type = 'Button',
  [string]$Out,
  [string]$Value,
  [string]$Pick,
  [string]$Chord,
  [int]$N = 0,
  [int]$X = 0,
  [int]$Y = 0
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'win32.ps1')
. (Join-Path $PSScriptRoot 'uia.ps1')
Add-Type -AssemblyName System.Windows.Forms

function Get-Hwnd {
  if ($Hwnd -ne '') { return [IntPtr][int64]$Hwnd }
  $h = [RbfWin]::FindWindowW($null, $Title)
  if ($h -eq [IntPtr]::Zero) { throw "window not found: $Title" }
  return $h
}

switch ($Verb) {
  'win' {
    $h = Get-Win $Title
    "hwnd=$($h.ToInt64()) visible=$([RbfWin]::IsWindowVisible($h))"
  }
  'shot' {
    $h = Get-Hwnd
    $bytes = [RbfWin]::CaptureWindowBmp($h)
    if (-not $bytes) { throw 'capture failed' }
    [System.IO.File]::WriteAllBytes($Out, $bytes)
    $w = [BitConverter]::ToInt32($bytes, 18); $hh = [BitConverter]::ToInt32($bytes, 22)
    "saved $Out ($($w)x$($hh))"
  }
  'pixel' {
    # sample a window-relative pixel from a fresh capture: drv.ps1 pixel -Hwnd N -X 100 -Y 50
    $h = Get-Hwnd
    $bytes = [RbfWin]::CaptureWindowBmp($h)
    if (-not $bytes) { throw 'capture failed' }
    $w = [BitConverter]::ToInt32($bytes, 18)
    $hh = [BitConverter]::ToInt32($bytes, 22)
    if ($X -lt 0 -or $X -ge $w -or $Y -lt 0 -or $Y -ge $hh) { throw "pixel out of range ($X,$Y) vs ${w}x${hh}" }
    $off = 54 + ($Y * $w + $X) * 4
    "rgb=$($bytes[$off+2]),$($bytes[$off+1]),$($bytes[$off])  hex=#{0:x2}{1:x2}{2:x2}" -f $bytes[$off+2], $bytes[$off+1], $bytes[$off]
  }
  'tree' {
    $h = Get-Hwnd
    $root = Get-UiaRoot $h
    $dump = Get-UiaDump $root
    if ($Out) { $dump | Out-File $Out -Encoding utf8; "dumped to $Out ($($dump.Split("`n").Count) elements)" }
    else { $dump }
  }
  'click' {
    $h = Get-Hwnd
    $root = Get-UiaRoot $h
    $els = Find-UiaByName $root $Name $Type
    if ($els.Count -eq 0) { throw "element not found: $Name ($Type)" }
    $r = Invoke-UiaButton $els[0]
    "clicked '$Name': $r"
  }
  'toggle' {
    $h = Get-Hwnd
    $root = Get-UiaRoot $h
    $els = Find-UiaByNameLike $root $Name 'CheckBox'
    if ($els.Count -eq 0) { throw "checkbox not found: $Name" }
    Toggle-UiaCheckbox $els[0]
  }
  'state' {
    $h = Get-Hwnd
    $root = Get-UiaRoot $h
    $els = Find-UiaByNameLike $root $Name 'CheckBox'
    if ($els.Count -eq 0) { throw "checkbox not found: $Name" }
    Get-UiaToggleState $els[0]
  }
  'edits' {
    $h = Get-Hwnd
    $root = Get-UiaRoot $h
    $n = Get-UiaTypeCount $root 'Edit'
    for ($i = 0; $i -lt $n; $i++) {
      $el = Get-UiaNthByType $root 'Edit' $i
      $vp = $null
      $val = ''
      if ($el.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) { $val = $vp.Current.Value }
      "Edit#$i name='$($el.Current.Name)' value='$val'"
    }
  }
  'setedit' {
    $h = Get-Hwnd
    $root = Get-UiaRoot $h
    $el = Get-UiaNthByType $root 'Edit' $N
    if (-not $el) { throw "edit #$N not found" }
    $c = Get-UiaElementCenter $el
    [RbfWin]::ClickAt($c[0], $c[1]) | Out-Null
    Start-Sleep -Milliseconds 200
    Set-UiaValue $el $Value
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    "set Edit#$N to '$Value'"
  }
  'combo' {
    $h = Get-Hwnd
    $root = Get-UiaRoot $h
    $els = Find-UiaByNameLike $root $Name 'ComboBox'
    if ($els.Count -eq 0) { throw "combo not found: $Name" }
    Select-UiaComboItem $els[0] $Pick
  }
  'keys' {
    & (Join-Path $PSScriptRoot 'sendkeys.ps1') -Chord $Chord
  }
  'clickpt' {
    [RbfWin]::ClickAt($X, $Y) | Out-Null
    "clicked $X,$Y"
  }
  default { throw "unknown verb $Verb" }
}
