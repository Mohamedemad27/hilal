# The prayer city drop-down (issue #2): opens the Prayer page (four pages after Music in Nova
# mode), clicks the city pill, takes pictures of the list, scrolls it with the wheel, and
# picks Maadi. Settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QI {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
}
'@
[QI]::SetProcessDPIAware() | Out-Null
$cx = [QI]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\city-picker'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-city-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
"city before: " + $j.prayer.city
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force vertical $false
$j.settings | Add-Member -Force anchor $null
$j.settings | Add-Member -Force docked $true
$j.settings | Add-Member -Force shrink_over_maximized $false
$j.settings | Add-Member -Force hide_over_maximized $false
$j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.prayer | Add-Member -Force city 31
$j.reminders | Add-Member -Force on $false
$j.features | Add-Member -Force hidden_pages @()
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
function Click($x, $y) { [QI]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 400; [QI]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QI]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 800 }
function Shot($h, $name) {
  $r = New-Object QI+RECT; [QI]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap 520, 520
  $gr = [System.Drawing.Graphics]::FromImage($bmp); $gr.CopyFromScreen($r.L - 60, $r.T - 170, 0, 0, $bmp.Size); $gr.Dispose()
  $bmp.Save("$out\$name.png"); $bmp.Dispose()
}
[QI]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_FPS = "$out\log.txt"; Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue; Start-Process $Exe; Remove-Item Env:HILAL_FPS
Start-Sleep 6
[QI]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1500
[QI]::SetCursorPos([int]($cx + 188), 69) | Out-Null; Start-Sleep 3
for ($i = 0; $i -lt 4; $i++) { Click ($cx + 188) 69 }
Click ($cx - 210 + 44 + 50) 111     # the city pill
$h = [QI]::FindWindow('HilalCities', 'Hilal Cities')
"list open: " + ($h -ne [IntPtr]::Zero -and [QI]::IsWindowVisible($h))
$r = New-Object QI+RECT; [QI]::GetWindowRect($h, [ref]$r) | Out-Null
# Where it opened, against the open notch (its page starts 22 dp in, past the ear; the pill 44 dp into the page).
$n = [QI]::FindWindow('HilalNotch', 'Hilal'); $nr = New-Object QI+RECT; [QI]::GetWindowRect($n, [ref]$nr) | Out-Null; $g = New-Object QI+RECT; [QI]::GetWindowRgnBox($n, [ref]$g) | Out-Null
$pageL = $nr.L + $g.L + 2 + 22
"open notch's page from x $pageL to $($nr.L + $g.R - 2 - 22), bottom y $($nr.T + $g.B); the list from x $($r.L) (that's $($r.L - $pageL) in; the pill is 44 in), top y $($r.T)"
[QI]::SetCursorPos($r.L + 100, $r.T + 150) | Out-Null; Start-Sleep 1
Shot $h 'list'
for ($i = 0; $i -lt 15; $i++) { [QI]::mouse_event(0x800, 0, 0, 120, 0); Start-Sleep -Milliseconds 120 }
Start-Sleep 1; Shot $h 'scrolled-up'
# Maadi: the third line from the top once scrolled all the way up (Egypt, Cairo, Maadi).
Click ($r.L + 100) ($r.T + 8 + 2 * 26 + 12)
"list after the pick: " + [QI]::IsWindowVisible($h)
[QI]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
$after = Get-Content $st -Raw | ConvertFrom-Json
"city after the pick: " + $after.prayer.city + " (1 is Maadi)"
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-city-test" $st -Force; Remove-Item "$st.before-city-test"
Select-String -Path "$out\log.txt" -Pattern '\[action\] Prayer|\[prayer\] city' | ForEach-Object Line
"settings restored"
