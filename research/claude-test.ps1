# Opens the Claude page with the real mouse, clicks the first session to open its popup,
# and lets Hilal save pictures of both (HILAL_DUMP) into research\claude.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [int]$Row = 0)
$out = 'K:\hilal\research\claude'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class C {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
}
'@
[C]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
$env:HILAL_DUMP = $out; Start-Process $Exe; Remove-Item Env:HILAL_DUMP
Start-Sleep 5
$sc = [C]::GetDpiForSystem() / 96.0; $cx = [C]::GetSystemMetrics(0) / 2
function At($x, $y) { [C]::SetCursorPos([int]($cx + ($x - 210) * $sc), [int]($y * $sc)) | Out-Null }
function Click($x, $y, $ms = 900) { At $x $y; Start-Sleep -Milliseconds 250; [C]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 60; [C]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
[C]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
for ($i = 0; $i -lt 5; $i++) { Click 398 69 }        # next page x5 -> Claude
Start-Sleep 1
Click 200 (46 + 31 * $Row) 1500                      # a session row -> popup
At 200 260; Start-Sleep 2                             # rest on the popup
[C]::SetCursorPos($cx, 700) | Out-Null; Start-Sleep 1
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Get-ChildItem $out -Filter '*.png' | Where-Object Name -match 'claude' | Select-Object -ExpandProperty Name
