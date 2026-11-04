# The download options window (issue #1), without downloading anything: a link on the
# clipboard, Paste and Download on the Video Download page (Classic, four pages after
# Music), then pictures of the window once the video's details are read, and of its Quality
# menu; then Cancel. The clipboard and the settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [string]$Link = 'https://www.youtube.com/watch?v=jNQXAC9IVRw')
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QG {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@
[QG]::SetProcessDPIAware() | Out-Null
$cx = [QG]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\dl-options'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-dlopt-test" -Force
$clipBefore = Get-Clipboard -Raw -ErrorAction SilentlyContinue
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $false
$j.settings | Add-Member -Force glass $false
$j.settings | Add-Member -Force shrink_over_maximized $false
$j.settings | Add-Member -Force hide_over_maximized $false
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
function Click($x, $y) { [QG]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 400; [QG]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QG]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 700 }
function Shot($h, $name) {
  $r = New-Object QG+RECT; [QG]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L + 40), ($r.B - $r.T + 160)
  $gr = [System.Drawing.Graphics]::FromImage($bmp); $gr.CopyFromScreen($r.L - 20, $r.T - 20, 0, 0, $bmp.Size); $gr.Dispose()
  $bmp.Save("$out\$name.png"); $bmp.Dispose()
}
Set-Clipboard -Value $Link
[QG]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_FPS = "$out\log.txt"; Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue; Start-Process $Exe; Remove-Item Env:HILAL_FPS
Start-Sleep 5
[QG]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1500
[QG]::SetCursorPos([int]($cx + 188), 69) | Out-Null; Start-Sleep 3
for ($i = 0; $i -lt 4; $i++) { Click ($cx + 188) 69 }
Start-Sleep 1
$x0 = $cx - 210
Click ($x0 + 350) 48          # Paste
Click ($x0 + 340) 86          # Download: the options
$h = [QG]::FindWindow('HilalDownload', 'Hilal Download')
"options window open: " + ($h -ne [IntPtr]::Zero -and [QG]::IsWindowVisible($h))
Start-Sleep 16
Shot $h 'options'
$r = New-Object QG+RECT; [QG]::GetWindowRect($h, [ref]$r) | Out-Null
# The Quality drop-down (third row), only while the options window is in front.
if ([QG]::GetForegroundWindow() -eq $h) {
  $k = 1.0
  Click ($r.L + 250 * $k) ($r.T + (70 + 2 * 38 + 19) * $k); Start-Sleep 1
  Shot $h 'quality-menu'
  [QG]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [QG]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 600
} else { "options window not in front: no menu opened" }
Click ($r.L + 230) ($r.B - 29)   # Cancel
"options window after Cancel: " + [QG]::IsWindowVisible($h)
[QG]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-dlopt-test" $st -Force; Remove-Item "$st.before-dlopt-test"
if ($clipBefore) { Set-Clipboard -Value $clipBefore } else { Set-Clipboard -Value $null }
Select-String -Path "$out\log.txt" -Pattern '\[action\] Dl|\[download\]' | ForEach-Object Line
"settings and clipboard restored"
