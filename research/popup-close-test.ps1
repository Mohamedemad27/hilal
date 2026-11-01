# Opens the first Claude session's popup with the real mouse, then clicks its close
# button, checking the popup window's visibility after each step. Hilal's log goes to
# research\popup.log.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class P2 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
'@
[P2]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
$log = 'K:\hilal\research\popup.log'
Remove-Item $log -ErrorAction SilentlyContinue
$env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_FPS
Start-Sleep 5
$sc = [P2]::GetDpiForSystem() / 96.0; $cx = [P2]::GetSystemMetrics(0) / 2
function Click($x, $y, $ms = 900) { [P2]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 250; [P2]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 60; [P2]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
[P2]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188 * $sc) (69 * $sc) }
Start-Sleep 1
Click ($cx - 10 * $sc) (46 * $sc) 1500
$h = [P2]::FindWindow('HilalPopup', $null); "after opening: visible=" + [P2]::IsWindowVisible($h)
[P2]::SetCursorPos([int]$cx, [int](300 * $sc)) | Out-Null; Start-Sleep 1
Click ($cx + 194 * $sc) (176 * $sc)
"after close button: visible=" + [P2]::IsWindowVisible($h)
[P2]::SetCursorPos($cx, 700) | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Select-String -Path $log -Pattern '\[dbg\]' -ErrorAction SilentlyContinue | ForEach-Object Line
