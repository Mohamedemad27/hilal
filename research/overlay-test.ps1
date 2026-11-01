# Starts Hilal, pins the FPS overlay if needed, hovers it, drags it, and logs + dumps.
param([string]$Dump)
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Threading;
public static class M {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  public static void Move(int x, int y) { int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1); mouse_event(0x8001, x * 65535 / (sw - 1), y * 65535 / (sh - 1), 0, 0); }
  public static void Click(int x, int y) { Move(x, y); Thread.Sleep(150); mouse_event(2, 0, 0, 0, 0); mouse_event(4, 0, 0, 0, 0); }
  public static void Drag(int x0, int y0, int x1, int y1) {
    Move(x0, y0); Thread.Sleep(150); mouse_event(2, 0, 0, 0, 0); Thread.Sleep(100);
    for (int i = 1; i <= 20; i++) { Move(x0 + (x1 - x0) * i / 20, y0 + (y1 - y0) * i / 20); Thread.Sleep(15); }
    Thread.Sleep(100); mouse_event(4, 0, 0, 0, 0);
  }
}
'@
[M]::SetProcessDPIAware() | Out-Null
$stateFile = Join-Path $env:APPDATA 'Hilal\state.json'
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Remove-Item K:\hilal\research\fps.txt -ErrorAction SilentlyContinue
[M]::Move(900, 700)
$env:HILAL_FPS = 'K:\hilal\research\fps.txt'; $env:HILAL_DUMP = $Dump
Start-Process K:\hilal\target\release\hilal.exe
Remove-Item Env:HILAL_FPS, Env:HILAL_DUMP
Start-Sleep 5
$state = Get-Content $stateFile -Raw | ConvertFrom-Json
if (-not $state.settings.overlay) {
  # Pin it: open the notch, go back to the System page, click the pin.
  [M]::Move(960, 3); Start-Sleep -Milliseconds 900
  [M]::Click(772, 69); Start-Sleep -Milliseconds 900
  [M]::Click(1122, 38); Start-Sleep -Milliseconds 600
  [M]::Move(900, 700)
  $state = Get-Content $stateFile -Raw | ConvertFrom-Json
}
"pinned: $($state.settings.overlay)"
$pos = if ($state.settings.overlay_pos) { $state.settings.overlay_pos } else { @(1720, 190) }
Start-Sleep 6                                  # readings fill in; plain dump happens here
$hx = $pos[0] + 70; $hy = $pos[1] + 35
[M]::Move($hx, $hy); Start-Sleep 2             # hover: close button shows; hovered dump
[M]::Drag($hx, $hy, 1500, 520); Start-Sleep 1
[M]::Move(900, 700); Start-Sleep 1
$state = Get-Content $stateFile -Raw | ConvertFrom-Json
"after drag: pinned=$($state.settings.overlay) pos=$($state.settings.overlay_pos -join ',')"
