# Where the notch goes (Nova Island's way): hanging from the top (the default), floating,
# standing on the right or left edge, and moved by dragging it with the mouse. For each:
# where the window sits, the notch's size closed and after resting the mouse on it, and a
# picture (HILAL_DUMP). Settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q2 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
}
'@
[Q2]::SetProcessDPIAware() | Out-Null
$sc = [Q2]::GetDpiForSystem() / 96.0; $W = [Q2]::GetSystemMetrics(0); $H = [Q2]::GetSystemMetrics(1)
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\position'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-pos-test" -Force

function Start-With($anchor, $docked, $vertical, $left, $name) {
  $j = Get-Content "$st.before-pos-test" -Raw | ConvertFrom-Json
  $j.settings | Add-Member -Force anchor $anchor
  $j.settings | Add-Member -Force docked $docked
  $j.settings | Add-Member -Force vertical $vertical
  $j.settings | Add-Member -Force side_left $left
  $j.settings | Add-Member -Force shrink_over_maximized $false
  $j.settings | Add-Member -Force glass $false
  [IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
  [Q2]::SetCursorPos(1100, 700) | Out-Null
  $d = "$out\$name"; New-Item -ItemType Directory -Force $d | Out-Null; Get-ChildItem $d -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
  $env:HILAL_DUMP = $d; $env:HILAL_FPS = "$d\log.txt"; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
  Start-Sleep 6
}
function Box { $h = [Q2]::FindWindow('HilalNotch', 'Hilal'); $r = New-Object Q2+RECT; [Q2]::GetWindowRect($h, [ref]$r) | Out-Null; $g = New-Object Q2+RECT; [Q2]::GetWindowRgnBox($h, [ref]$g) | Out-Null; ,@($r, $g) }
function Report($name) {
  $b = Box; $r = $b[0]; $g = $b[1]
  $nx = $r.L + ($g.L + $g.R) / 2; $ny = $r.T + ($g.T + $g.B) / 2
  "{0,-9} window {1},{2} .. {3},{4}   notch {5}x{6} at {7},{8}" -f $name, $r.L, $r.T, $r.R, $r.B, [int](($g.R - $g.L) / $sc), [int](($g.B - $g.T) / $sc), [int]$nx, [int]$ny
  [Q2]::SetCursorPos([int]$nx, [int]$ny) | Out-Null; Start-Sleep -Milliseconds 1500
  $b = Box; $g = $b[1]
  "          after resting the mouse on it: {0}x{1} at x {2}..{3}" -f [int](($g.R - $g.L) / $sc), [int](($g.B - $g.T) / $sc), ($b[0].L + $g.L), ($b[0].L + $g.R)
  [Q2]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
}
function Stop-It { Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1 }

Start-With $null $true $false $false 'top'; Report 'top'; Stop-It
Start-With @(500, 300) $false $false $false 'floating'; Report 'floating'; Stop-It
Start-With @($W, 500) $false $true $false 'right'; Report 'right'; Stop-It
Start-With @(0, 400) $false $true $true 'left'; Report 'left'; Stop-It

# Dragging: from the top center down to (700, 420), then back near the top middle.
Start-With $null $true $false $false 'drag'
$b = Box; $r = $b[0]; $g = $b[1]; $x = [int]($r.L + ($g.L + $g.R) / 2); $y = [int]($r.T + ($g.T + $g.B) / 2)
function Drag($x0, $y0, $x1, $y1) {
  [Q2]::SetCursorPos($x0, $y0) | Out-Null; Start-Sleep -Milliseconds 300
  [Q2]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 100
  for ($i = 1; $i -le 30; $i++) { [Q2]::SetCursorPos([int]($x0 + ($x1 - $x0) * $i / 30), [int]($y0 + ($y1 - $y0) * $i / 30)) | Out-Null; Start-Sleep -Milliseconds 15 }
  Start-Sleep -Milliseconds 200; [Q2]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 600
}
Drag $x $y 700 420
$b = Box; "dragged   window {0},{1}   notch {2}x{3}" -f $b[0].L, $b[0].T, [int](($b[1].R - $b[1].L) / $sc), [int](($b[1].B - $b[1].T) / $sc)
[Q2]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
$b = Box; $r = $b[0]; $g = $b[1]; $x = [int]($r.L + ($g.L + $g.R) / 2); $y = [int]($r.T + ($g.T + $g.B) / 2)
Drag $x $y ([int]($W / 2 + 25)) 20
$b = Box; "back      window {0},{1}   notch {2}x{3}  (screen middle {4})" -f $b[0].L, $b[0].T, [int](($b[1].R - $b[1].L) / $sc), [int](($b[1].B - $b[1].T) / $sc), ($W / 2)
[Q2]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
Select-String -Path "$out\drag\log.txt" -Pattern '\[drag\]' | ForEach-Object Line
Stop-It
Copy-Item "$st.before-pos-test" $st -Force; Remove-Item "$st.before-pos-test"
"settings restored"
