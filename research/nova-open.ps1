# Opens Nova Island with its shortcut (Ctrl+Alt+I) and screenshots the top of the screen.
# With -Click x,y (screen px) it clicks there afterwards (a tab, say) and shoots again.
param([string]$Name = 'open', [int[]]$Click = @(), [int]$Height = 560, [switch]$NoKey)
$out = 'K:\hilal\research\nova-look'; New-Item -ItemType Directory -Force $out | Out-Null
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class N2 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
'@
[N2]::SetProcessDPIAware() | Out-Null
$cx = [N2]::GetSystemMetrics(0) / 2
function Shot($n) { $b = New-Object System.Drawing.Bitmap 1000, $Height; $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen([int]($cx - 500), 0, 0, 0, $b.Size); $b.Save("$out\$n.png"); $g.Dispose(); $b.Dispose() }
if (-not $NoKey) {
  [N2]::SetCursorPos([int]$cx, 300) | Out-Null; Start-Sleep -Milliseconds 300
  [System.Windows.Forms.SendKeys]::SendWait('^%i'); Start-Sleep -Milliseconds 1500
}
if ($Click.Count -eq 2) {
  [N2]::SetCursorPos($Click[0], $Click[1]) | Out-Null; Start-Sleep -Milliseconds 200
  [N2]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 50; [N2]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds 1200
}
Shot $Name
"$out\$Name.png  (the picture's left edge is screen x $([int]($cx - 500)))"
