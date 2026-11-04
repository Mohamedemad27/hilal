# A reminder card shown for 10 s (HILAL_REMINDER_TEST): resting the mouse on it for 6 s
# holds its countdown, so it stays about 16 s; it doesn't open the notch, and when it
# leaves from under the mouse the notch doesn't spring open. Settings are put back.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q3 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
}
'@
[Q3]::SetProcessDPIAware() | Out-Null
$sc = [Q3]::GetDpiForSystem() / 96.0
$st = Join-Path $env:APPDATA 'Hilal\state.json'
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-hover-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.reminders | Add-Member -Force seconds 10
$j.reminders | Add-Member -Force minutes 1
$j.settings | Add-Member -Force shrink_over_maximized $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
[Q3]::SetCursorPos(1100, 700) | Out-Null
$log = 'K:\hilal\research\reminder\hover-log.txt'
$env:HILAL_REMINDER_TEST = '2'; $env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_REMINDER_TEST, Env:HILAL_FPS
$h = [IntPtr]::Zero; while ($h -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 200; $h = [Q3]::FindWindow('HilalNotch', 'Hilal') }
function Size { $g = New-Object Q3+RECT; [Q3]::GetWindowRgnBox($h, [ref]$g) | Out-Null; '{0}x{1}' -f [int](($g.R - $g.L) / $sc), [int](($g.B - $g.T) / $sc) }
$t0 = $null
for ($i = 0; $i -lt 70; $i++) {
  $s = Size
  if (-not $t0 -and [int]($s.Split('x')[0]) -gt 400) { $t0 = Get-Date; "card up: $s" }
  if ($t0) {
    $el = ((Get-Date) - $t0).TotalSeconds
    if ($el -ge 2 -and $el -lt 2.6) { [Q3]::SetCursorPos(960, 40) | Out-Null; "{0:n1} s: mouse on the card" -f $el }
    if ($el -ge 8 -and $el -lt 8.6) { "{0:n1} s: still {1} under the mouse (not opened)" -f $el, $s; [Q3]::SetCursorPos(1100, 700) | Out-Null; "          mouse away" }
    if ([int]($s.Split('x')[0]) -le 400) { "{0:n1} s: card gone ({1})" -f $el, $s; break }
  }
  Start-Sleep -Milliseconds 500
}
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-hover-test" $st -Force; Remove-Item "$st.before-hover-test"
Select-String -Path $log -Pattern '\[reminder\]|\[open\]' | ForEach-Object Line
"settings restored"
