# Opens the notch, presses and holds the next-page arrow, then lets go; with HILAL_DUMP
# set, Hilal saves the pressed and released frames. Then plays the chime test.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
$out = 'K:\hilal\research\press'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class P {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
}
'@
[P]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
$env:HILAL_DUMP = $out; Start-Process $Exe; Remove-Item Env:HILAL_DUMP
Start-Sleep 5   # past the welcome
$sc = [P]::GetDpiForSystem() / 96.0; $cx = [P]::GetSystemMetrics(0) / 2
[P]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
# The next-page arrow sits at x = CX - 210 + 398 dp, y = 69 dp.
[P]::SetCursorPos([int]($cx + 188 * $sc), [int](69 * $sc)) | Out-Null; Start-Sleep -Milliseconds 600
[P]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 400
[P]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 800
[P]::SetCursorPos($cx, 600) | Out-Null; Start-Sleep 1
Get-ChildItem $out -Filter *.png | Select-Object -ExpandProperty Name
