# The Claude popup with the real mouse and keyboard: it closes by itself when the mouse
# leaves; pinned, it stays; and its message box takes typing. (It doesn't press send:
# that would paste into the real Claude app.) Pictures go to research\claude.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
$out = 'K:\hilal\research\claude'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class P3 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
}
'@
[P3]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
$env:HILAL_DUMP = $out; Start-Process $Exe; Remove-Item Env:HILAL_DUMP
Start-Sleep 5
$sc = [P3]::GetDpiForSystem() / 96.0; $cx = [P3]::GetSystemMetrics(0) / 2
$h = [P3]::FindWindow('HilalPopup', 'Hilal Claude')
function Shown { [P3]::IsWindowVisible($h) }
function Click($x, $y, $ms = 900) { [P3]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 250; [P3]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 60; [P3]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
function OpenPopup($pages) {
  [P3]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
  for ($i = 0; $i -lt $pages; $i++) { Click ($cx + 188 * $sc) (69 * $sc) }
  Start-Sleep 1
  Click ($cx - 10 * $sc) (46 * $sc) 1200
  [P3]::SetCursorPos([int]$cx, [int](300 * $sc)) | Out-Null; Start-Sleep -Milliseconds 800
}
# 1. Leaves on its own.
OpenPopup 5
"opened: " + (Shown)
[P3]::SetCursorPos([int]($cx + 600 * $sc), [int](700 * $sc)) | Out-Null; Start-Sleep 2
"after the mouse left (unpinned): " + (Shown)
# 2. Pinned, it stays (the notch is still on the Claude page).
OpenPopup 0
Click ($cx - 220 * $sc + 382 * $sc) (176 * $sc)     # pin
[P3]::SetCursorPos([int]($cx + 600 * $sc), [int](700 * $sc)) | Out-Null; Start-Sleep 2
"after the mouse left (pinned): " + (Shown)
# 3. Typing in the box.
Click ($cx - 220 * $sc + 150 * $sc) (150 * $sc + 368 * $sc) 600
[System.Windows.Forms.SendKeys]::SendWait('hello from Hilal')
Start-Sleep 1
Copy-Item "$out\claude-popup.png" "$out\popup-first.png" -ErrorAction SilentlyContinue
"still shown while typing: " + (Shown)
Start-Sleep 1
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
