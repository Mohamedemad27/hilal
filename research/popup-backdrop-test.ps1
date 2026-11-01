# Puts a plain white window behind where the Claude popup opens, opens the popup, then
# turns that window black: Hilal saves the popup 1 s (over white) and 4 s (over black)
# after opening, and logs if the glass's screen capture drops.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
$out = 'K:\hilal\research\claude'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter 'claude-popup*' -ErrorAction SilentlyContinue | Remove-Item
$log = 'K:\hilal\research\popup-backdrop.log'; Remove-Item $log -ErrorAction SilentlyContinue
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class P9 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
}
'@
[P9]::SetProcessDPIAware() | Out-Null
$sc = [P9]::GetDpiForSystem() / 96.0; $cx = [P9]::GetSystemMetrics(0) / 2
$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = 'None'; $form.StartPosition = 'Manual'; $form.ShowInTaskbar = $false
$form.Bounds = New-Object System.Drawing.Rectangle ([int]($cx - 300 * $sc)), ([int](120 * $sc)), ([int](600 * $sc)), ([int](500 * $sc))
$form.BackColor = [System.Drawing.Color]::White
$form.Show(); [System.Windows.Forms.Application]::DoEvents()
function Pump($ms) { $sw = [Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt $ms) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 } }
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Pump 1000
$env:HILAL_DUMP = $out; $env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
Pump 5000
function Click($x, $y, $ms = 900) { [P9]::SetCursorPos([int]$x, [int]$y) | Out-Null; Pump 250; [P9]::mouse_event(2,0,0,0,0); Pump 60; [P9]::mouse_event(4,0,0,0,0); Pump $ms }
[P9]::SetCursorPos($cx, 5) | Out-Null; Pump 1200
for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188 * $sc) (69 * $sc) }
Pump 1000
Click ($cx - 10 * $sc) (46 * $sc) 300                 # open the popup
[P9]::SetCursorPos([int]$cx, [int](300 * $sc)) | Out-Null
Pump 2200
$form.BackColor = [System.Drawing.Color]::Black; $form.Refresh()
Pump 3500
[P9]::SetCursorPos([int]($cx + 700 * $sc), [int](900 * $sc)) | Out-Null; Pump 1500
$form.Close()
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Get-ChildItem $out -Filter 'claude-popup-*s.png' | Select-Object -ExpandProperty Name
"capture drops: " + @(Select-String -Path $log -Pattern 'capture lost' -ErrorAction SilentlyContinue).Count
