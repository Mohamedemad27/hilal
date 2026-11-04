# Looks at Nova Island running: screenshots of the top of the screen while it's closed,
# hovered and clicked open, and of each of its tabs. Pictures go to research\nova-look.
param([int[]]$Tabs = @())
$out = 'K:\hilal\research\nova-look'; New-Item -ItemType Directory -Force $out | Out-Null
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class N1 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
'@
[N1]::SetProcessDPIAware() | Out-Null
$W = [N1]::GetSystemMetrics(0); $cx = $W / 2
function Shot($name, $h = 420) { $b = New-Object System.Drawing.Bitmap 900, $h; $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen([int]($cx - 450), 0, 0, 0, $b.Size); $b.Save("$out\$name.png"); $g.Dispose(); $b.Dispose() }
function Click($x, $y, $ms = 900) { [N1]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 200; [N1]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 50; [N1]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
[N1]::SetCursorPos(1700, 900) | Out-Null; Start-Sleep 1
Shot 'closed' 120
[N1]::SetCursorPos([int]$cx, 12) | Out-Null; Start-Sleep -Milliseconds 1200
Shot 'hover'
Click $cx 18 1500
Shot 'open'
"saved to $out"
if ($args -contains 'open-key') {
  Add-Type -AssemblyName System.Windows.Forms
  [N1]::SetCursorPos(1700, 900) | Out-Null; Start-Sleep -Milliseconds 500
  [System.Windows.Forms.SendKeys]::SendWait('^%i'); Start-Sleep -Milliseconds 1500
  Shot 'open-key' 520
}
