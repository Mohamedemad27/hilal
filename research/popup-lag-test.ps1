# Opens the Claude popup and rests the mouse on it for a few seconds while Hilal logs
# its frame timing (HILAL_FPS), to see whether anything stalls while the popup is up.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [int]$Rest = 6)
$log = 'K:\hilal\research\popup-lag.log'; Remove-Item $log -ErrorAction SilentlyContinue
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class P6 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
}
'@
[P6]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
$env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_FPS
Start-Sleep 5
$sc = [P6]::GetDpiForSystem() / 96.0; $cx = [P6]::GetSystemMetrics(0) / 2
function Click($x, $y, $ms = 900) { [P6]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 250; [P6]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 60; [P6]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
[P6]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188 * $sc) (69 * $sc) }
Start-Sleep 1
"--- popup opens" | Out-File -Append -Encoding utf8 $log
Click ($cx - 10 * $sc) (46 * $sc) 300
# Wander over the popup.
for ($i = 0; $i -lt $Rest * 10; $i++) { [P6]::SetCursorPos([int]($cx - 150 * $sc + ($i % 30) * 10 * $sc), [int]((250 + ($i % 7) * 20) * $sc)) | Out-Null; Start-Sleep -Milliseconds 100 }
"--- mouse leaves" | Out-File -Append -Encoding utf8 $log
[P6]::SetCursorPos([int]($cx + 600 * $sc), [int](700 * $sc)) | Out-Null; Start-Sleep 2
$p = Get-Process hilal; "responding: " + $p.Responding
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Get-Content $log | Where-Object { $_ -match 'fps  max gap|---' } | Select-Object -Last 16
