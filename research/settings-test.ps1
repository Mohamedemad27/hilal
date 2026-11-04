# Opens the Settings window (HILAL_SETTINGS_TEST), takes a picture of each page, clicks a
# few controls with the real mouse and shows what they changed in state.json. Settings are
# put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q4 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, int d, UIntPtr e);
  [DllImport("user32.dll")] public static extern int GetDpiForWindow(IntPtr h);
}
'@
[Q4]::SetProcessDPIAware() | Out-Null
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\settings'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-settings-test" -Force
$env:HILAL_SETTINGS_TEST = '1'; $env:HILAL_FPS = "$out\log.txt"; Start-Process $Exe; Remove-Item Env:HILAL_SETTINGS_TEST, Env:HILAL_FPS
Start-Sleep 5
$h = [Q4]::FindWindow('HilalSettings', 'Hilal Settings')
$r = New-Object Q4+RECT; [Q4]::GetWindowRect($h, [ref]$r) | Out-Null
$k = [Q4]::GetDpiForWindow($h) / 96.0
"window {0},{1} .. {2},{3}" -f $r.L, $r.T, $r.R, $r.B
function Shot($name) {
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $gr = [System.Drawing.Graphics]::FromImage($bmp); $gr.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size); $gr.Dispose()
  $bmp.Save("$out\$name.png"); $bmp.Dispose()
}
function Click($x, $y) {
  [Q4]::SetCursorPos([int]($r.L + $x * $k), [int]($r.T + $y * $k)) | Out-Null; Start-Sleep -Milliseconds 250
  [Q4]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [Q4]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 500
}
function Wheel($x, $y, $d) { [Q4]::SetCursorPos([int]($r.L + $x * $k), [int]($r.T + $y * $k)) | Out-Null; Start-Sleep -Milliseconds 200; [Q4]::mouse_event(0x800, 0, 0, $d, [UIntPtr]::Zero); Start-Sleep -Milliseconds 400 }
function State { Get-Content $st -Raw | ConvertFrom-Json }
Shot 'general'
# Over maximized windows: "Hide" (the last segment of the fifth row).
$s0 = State; Click 685 311; $s1 = State
"over maximized: hide {0} -> {1}" -f $s0.settings.hide_over_maximized, $s1.settings.hide_over_maximized
Click 579 311
# The Prayer & Dhikr page.
Click 100 90; Shot 'prayer'
$s0 = State; Click 692 197; $s1 = State
"hanafi: {0} -> {1}" -f $s0.prayer.hanafi, $s1.prayer.hanafi
$s0 = State; Click 697 243; $s1 = State
"hijri +: {0} -> {1}" -f $s0.prayer.hijri_adjust, $s1.prayer.hijri_adjust
$s0 = State; Click 697 429; $s1 = State
"every +: {0} -> {1}" -f $s0.reminders.minutes, $s1.reminders.minutes
Wheel 480 400 -360; Shot 'prayer-scrolled'
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-settings-test" $st -Force; Remove-Item "$st.before-settings-test"
Select-String -Path "$out\log.txt" -Pattern '\[settings\]' | ForEach-Object Line
"settings restored"
