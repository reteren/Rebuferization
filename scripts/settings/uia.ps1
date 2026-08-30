# UI Automation helpers (System.Windows.Automation) for WebView2 windows.
# Runs under Windows PowerShell 5.1 (native .NET Framework assemblies).
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName WindowsBase

function Get-UiaRoot([IntPtr]$hwnd) {
  [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
}

function Get-UiaAll($root) {
  $root.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition)
}

function Find-UiaByName($root, [string]$name, [string]$type) {
  $cond = New-Object System.Windows.Automation.AndCondition(
    (New-Object System.Windows.Automation.PropertyCondition(
      [System.Windows.Automation.AutomationElement]::NameProperty, $name)),
    (New-Object System.Windows.Automation.PropertyCondition(
      [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
      [System.Windows.Automation.ControlType]::$type)))
  $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
}

function Find-UiaByNameLike($root, [string]$namePart, [string]$type) {
  $out = New-Object System.Collections.ArrayList
  $all = Get-UiaAll $root
  foreach ($el in $all) {
    if ($el.Current.Name -like "*$namePart*" -and $el.Current.ControlType.ProgrammaticName -eq "ControlType.$type") {
      [void]$out.Add($el)
    }
  }
  ,$out
}

function Invoke-UiaButton($el) {
  $inv = $null
  if ($el.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$inv)) {
    $inv.Invoke()
    return 'invoked'
  }
  return 'no-invoke-pattern'
}

function Toggle-UiaCheckbox($el) {
  $tg = $null
  if ($el.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$tg)) {
    $tg.Toggle()
    return 'toggled'
  }
  return 'no-toggle-pattern'
}

function Get-UiaToggleState($el) {
  $tg = $null
  if ($el.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$tg)) {
    return $tg.Current.ToggleState
  }
  return 'no-toggle'
}

function Set-UiaValue($el, [string]$value) {
  $vp = $null
  if ($el.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$vp)) {
    $vp.SetValue($value)
    return 'set'
  }
  return 'no-value-pattern'
}

function Select-UiaComboItem($combo, [string]$text) {
  $exp = $null
  if ($combo.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$exp)) {
    $exp.Expand()
    Start-Sleep -Milliseconds 400
  }
  $all = Get-UiaAll $combo
  foreach ($el in $all) {
    if ($el.Current.ControlType.ProgrammaticName -eq 'ControlType.ListItem' -and $el.Current.Name -match $text) {
      $sel = $null
      if ($el.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$sel)) {
        $sel.Select()
        return "selected $($el.Current.Name)"
      }
    }
  }
  $exp.Collapse() 2>$null
  return 'item-not-found'
}

function Get-UiaDump($root) {
  $sb = New-Object System.Text.StringBuilder
  $i = 0
  $all = Get-UiaAll $root
  foreach ($el in $all) {
    $r = $el.Current.BoundingRectangle
    $name = $el.Current.Name -replace "[\r\n]+", ' '
    [void]$sb.AppendLine(("#{0} {1} name='{2}' rect={3},{4},{5},{6} enabled={7}" -f $i,
      $el.Current.ControlType.ProgrammaticName, $name, [int]$r.X, [int]$r.Y, [int]$r.Width, [int]$r.Height, $el.Current.IsEnabled))
    $i++
  }
  $sb.ToString()
}

function Get-UiaTypeCount($root, [string]$typeName) {
  $n = 0
  foreach ($el in (Get-UiaAll $root)) {
    if ($el.Current.ControlType.ProgrammaticName -eq "ControlType.$typeName") { $n++ }
  }
  $n
}

function Get-UiaNthByType($root, [string]$typeName, [int]$n) {
  $i = 0
  foreach ($el in (Get-UiaAll $root)) {
    if ($el.Current.ControlType.ProgrammaticName -eq "ControlType.$typeName") {
      if ($i -eq $n) { return $el }
      $i++
    }
  }
  $null
}

function Get-UiaElementCenter($el) {
  $r = $el.Current.BoundingRectangle
  ,@([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
}