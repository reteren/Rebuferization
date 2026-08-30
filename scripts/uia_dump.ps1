# Dump the UI Automation tree of the popup window. WebView2 exposes its page
# content (text + bounding rects) through UIA, so this shows what is actually
# rendered inside the window, error page or app UI.
param([switch]$Deep, [int]$MaxDepth = 6)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'win.psm1')

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$proc = Get-Process rebuffer -ErrorAction SilentlyContinue
if (-not $proc) { throw 'rebuffer.exe is not running' }
$hwnd = [Win32]::FindWindowByTitle('Rebuffer', $proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw 'popup window not found' }
if (-not [Win32]::IsWindowVisible($hwnd)) { throw 'popup is not visible; show it first' }

$root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)

function Dump([System.Windows.Automation.AutomationElement]$el, [int]$depth) {
    $pad = '  ' * $depth
    $name = $el.Current.Name
    $type = $el.Current.ControlType.ProgrammaticName -replace 'ControlType\.', ''
    $role = $el.Current.ClassName
    try { $rect = $el.Current.BoundingRectangle } catch { $rect = $null }
    $rectTxt = ''
    if ($rect -and -not $rect.IsEmpty) {
        $rectTxt = "[{0:N0},{1:N0} {2:N0}x{3:N0}]" -f $rect.X, $rect.Y, $rect.Width, $rect.Height
    }
    if ($name -or $type -ne 'Pane' -or $depth -le 1) {
        Write-Output ("{0}{1} {2} name='{3}'" -f $pad, $type, $rectTxt, ($name -replace "'", "''"))
    }
    if ($depth -ge $MaxDepth) { return }
    $children = $el.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    if ($children -and $children.Count -le 400) {
        foreach ($c in $children) { Dump $c ($depth + 1) }
    } else {
        Write-Output ("{0}... ({1} children)" -f $pad, $children.Count)
    }
}

Dump $root 0