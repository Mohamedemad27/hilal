# Opens the taskbar's hidden-icons flyout, counts tray buttons named like "Hilal", screenshots
# the flyout, and closes it again.
param([string]$Shot = 'K:\hilal\research\tray-flyout.png')
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
$A = [System.Windows.Automation.AutomationElement]
$root = $A::RootElement
$chevron = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
  (New-Object System.Windows.Automation.PropertyCondition($A::NameProperty, 'Show hidden icons')))
if ($chevron) { $chevron.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke(); Start-Sleep -Milliseconds 900 }
$buttons = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
  (New-Object System.Windows.Automation.PropertyCondition($A::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)))
$hilal = @($buttons | Where-Object { $_.Current.Name -like 'Hilal*' })
"Hilal tray buttons: $($hilal.Count)"
$flyout = $root.FindFirst([System.Windows.Automation.TreeScope]::Children,
  (New-Object System.Windows.Automation.PropertyCondition($A::ClassNameProperty, 'NotifyIconOverflowWindow')))
if ($flyout) {
  $r = $flyout.Current.BoundingRectangle
  $bmp = New-Object System.Drawing.Bitmap ([int]$r.Width), ([int]$r.Height)
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen([int]$r.X, [int]$r.Y, 0, 0, $bmp.Size)
  $bmp.Save($Shot); "flyout saved"
}
if ($chevron) { $chevron.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke() }
