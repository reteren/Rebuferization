Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName WindowsBase
Write-Output "UIA root type: $([System.Windows.Automation.AutomationElement].FullName)"
Write-Output "Desktop element: $([System.Windows.Automation.AutomationElement]::RootElement.Current.Name)"
$p = Get-Process -Name rebuffer -ErrorAction SilentlyContinue | Select-Object -First 1
Write-Output "rebuffer pid: $($p.Id)"
if ($p) {
  $el = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
  Write-Output "main window handle: $($p.MainWindowHandle) name: $($el.Current.Name)"
}