# Nova mode against Classic Hilal. Nova mode with Ramadan forced (HILAL_RAMADAN_TEST): a
# picture of the prayer page. Classic, with Nova's settings saved (vertical, a dhikr due,
# Ctrl+Alt+H): the notch is back at the top center, no reminder comes, the shortcut does
# nothing, and pictures of the prayer page and of Settings. Settings are put back.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q5 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
}
'@
[Q5]::SetProcessDPIAware() | Out-Null
$W = [Q5]::GetSystemMetrics(0); $cx = $W / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\modes'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-modes-test" -Force
function Setup($nova, $vertical) {
  $j = Get-Content "$st.before-modes-test" -Raw | ConvertFrom-Json
  $j.settings | Add-Member -Force nova $nova
  $j.settings | Add-Member -Force vertical $vertical
  $j.settings | Add-Member -Force side_left $false
  $j.settings | Add-Member -Force anchor $(if ($vertical) { @($W, 500) } else { $null })
  $j.settings | Add-Member -Force docked $true
  $j.water | Add-Member -Force enabled $false
  $j.settings | Add-Member -Force glass $false
  $j.settings | Add-Member -Force shrink_over_maximized $false
  [IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
}
function Size { $h = [Q5]::FindWindow('HilalNotch', 'Hilal'); $r = New-Object Q5+RECT; [Q5]::GetWindowRect($h, [ref]$r) | Out-Null; $g = New-Object Q5+RECT; [Q5]::GetWindowRgnBox($h, [ref]$g) | Out-Null; "window x {0}..{1} y {2}   notch {3}x{4}" -f $r.L, $r.R, $r.T, ($g.R - $g.L), ($g.B - $g.T) }
function ToPrayer {
  # Open the notch at the top center and step three pages on (Music, Pomodoro, Water, Prayer).
  [Q5]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
  for ($i = 0; $i -lt 3; $i++) { [Q5]::SetCursorPos([int]($cx + 188), 69) | Out-Null; Start-Sleep -Milliseconds 900; [Q5]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [Q5]::mouse_event(4, 0, 0, 0, 0) }
  Start-Sleep -Milliseconds 1500
}
function Press { foreach ($v in 0x11, 0x12, 0x48) { [Q5]::keybd_event($v, 0, 0, [UIntPtr]::Zero) }; Start-Sleep -Milliseconds 50; foreach ($v in 0x48, 0x12, 0x11) { [Q5]::keybd_event($v, 0, 2, [UIntPtr]::Zero) } }

# Nova mode, Ramadan.
Setup $true $false
$d = "$out\nova-ramadan"; New-Item -ItemType Directory -Force $d | Out-Null; Get-ChildItem $d -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
[Q5]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $d; $env:HILAL_RAMADAN_TEST = '1'; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_RAMADAN_TEST
Start-Sleep 5; ToPrayer
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
"nova ramadan pictures: " + ((Get-ChildItem $d -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')

# Classic, with Nova's settings saved.
Setup $false $true
$d = "$out\classic"; New-Item -ItemType Directory -Force $d | Out-Null; Get-ChildItem $d -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
[Q5]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $d; $env:HILAL_REMINDER_TEST = '2'; $env:HILAL_FPS = "$d\log.txt"; $env:HILAL_SETTINGS_TEST = '1'; Start-Process $Exe
Remove-Item Env:HILAL_DUMP, Env:HILAL_REMINDER_TEST, Env:HILAL_FPS, Env:HILAL_SETTINGS_TEST
Start-Sleep 6
"classic:  " + (Size) + "   (screen middle $cx)"
$h = [Q5]::FindWindow('HilalSettings', 'Hilal Settings'); $r = New-Object Q5+RECT; [Q5]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T); $gr = [System.Drawing.Graphics]::FromImage($bmp); $gr.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size); $gr.Dispose(); $bmp.Save("$d\settings.png"); $bmp.Dispose()
[Q5]::SetCursorPos($r.L + 100, $r.T + 106) | Out-Null; Start-Sleep -Milliseconds 300; [Q5]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [Q5]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 600
$bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T); $gr = [System.Drawing.Graphics]::FromImage($bmp); $gr.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size); $gr.Dispose(); $bmp.Save("$d\settings-prayer.png"); $bmp.Dispose()
[Q5]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep -Milliseconds 300
Press; Start-Sleep -Milliseconds 1200
"after Ctrl+Alt+H: " + (Size)
ToPrayer
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
"classic pictures: " + ((Get-ChildItem $d -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
Select-String -Path "$d\log.txt" -Pattern '\[reminder\]|\[hotkey\]|\[mode\]' | ForEach-Object Line
Copy-Item "$st.before-modes-test" $st -Force; Remove-Item "$st.before-modes-test"
"settings restored"
