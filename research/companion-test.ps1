# Nova mode's companion: the morning card (9 s after start, the first time today), the
# adhan scene (HILAL_PRAYER_TEST, sound muted by the test: alerts' adhan plays), and the eyes
# (the mouse resting beside the notch). Hilal saves each (HILAL_DUMP). Settings put back.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QB {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
}
'@
$cx = [QB]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\companion'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-companion-test" -Force
function Setup($alerts, $brief = 0) {
  $j = Get-Content "$st.before-companion-test" -Raw | ConvertFrom-Json
  $j.settings | Add-Member -Force nova $true
  $j.settings | Add-Member -Force glass $false
  $j.settings | Add-Member -Force vertical $false
  $j.settings | Add-Member -Force anchor $null
  $j.settings | Add-Member -Force docked $true
  $j | Add-Member -Force brief_day $brief
  $j.water | Add-Member -Force enabled $false
  $j.prayer | Add-Member -Force alerts $alerts
  $j.reminders | Add-Member -Force on $false
  [IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
}
function Run($name, $secs, $envs, $move) {
  $d = "$out\$name"; New-Item -ItemType Directory -Force $d | Out-Null; Get-ChildItem $d -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
  [QB]::SetCursorPos(1100, 700) | Out-Null
  $env:HILAL_DUMP = $d; foreach ($k in $envs.Keys) { Set-Item "Env:$k" $envs[$k] }
  Start-Process $Exe
  Remove-Item Env:HILAL_DUMP; foreach ($k in $envs.Keys) { Remove-Item "Env:$k" }
  if ($move) { Start-Sleep 5; [QB]::SetCursorPos([int]($cx + 160), 20) | Out-Null }
  Start-Sleep $secs
  [QB]::SetCursorPos(1100, 700) | Out-Null
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
  "${name}: " + ((Get-ChildItem $d -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
}
Setup $false; Run 'morning' 14 @{} $false
Setup $true; Run 'adhan' 8 @{ HILAL_PRAYER_TEST = '4' } $false
$today = [int](Get-Date -Format 'yyyyMMdd')
Setup $false $today; Run 'eyes' 6 @{} $true
Copy-Item "$st.before-companion-test" $st -Force; Remove-Item "$st.before-companion-test"
"settings restored"
