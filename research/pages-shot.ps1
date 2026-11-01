# Opens the notch and steps through every page with the next arrow; with HILAL_DUMP set,
# Hilal saves one PNG of each page into research\pages.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [int]$Pages = 6)
$out = 'K:\hilal\research\pages'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
}
'@
[Q]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
$env:HILAL_DUMP = $out; Start-Process $Exe; Remove-Item Env:HILAL_DUMP
Start-Sleep 5
$sc = [Q]::GetDpiForSystem() / 96.0; $cx = [Q]::GetSystemMetrics(0) / 2
[Q]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
$ax = [int]($cx + 188 * $sc); $ay = [int](69 * $sc)
for ($i = 0; $i -lt $Pages; $i++) {
  [Q]::SetCursorPos($ax, $ay) | Out-Null; Start-Sleep -Milliseconds 3200
  [Q]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [Q]::mouse_event(4, 0, 0, 0, 0)
}
Start-Sleep 1; [Q]::SetCursorPos($cx, 600) | Out-Null; Start-Sleep 1
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Get-ChildItem $out -Filter *.png | Where-Object Name -notmatch 'mid-|press|releas' | Select-Object -ExpandProperty Name
