# Nova mode's Quran Radio: opens the Radio page (five pages after Music), waits for the
# stations, picks the first (the official radio), lets it play a few seconds at a low
# volume, then stops it. Hilal saves the page in each state (HILAL_DUMP). Settings put back.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QF {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
'@
$cx = [QF]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\radio'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-radio-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force glass $false
$j.settings | Add-Member -Force vertical $false
$j.settings | Add-Member -Force anchor $null
$j.settings | Add-Member -Force docked $true
$j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
$j | Add-Member -Force radio_volume 0.15
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.reminders | Add-Member -Force on $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
function Click($x, $y) { [QF]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 400; [QF]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QF]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 700 }
[QF]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; $env:HILAL_FPS = "$out\log.txt"; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
Start-Sleep 5
[QF]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1500
for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188) 69 }
[QF]::SetCursorPos($cx, 60) | Out-Null; Start-Sleep 4
Click ($cx - 210 + 300) 44
Start-Sleep 8
Click ($cx - 210 + 68) 74
Start-Sleep 2
[QF]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
$after = Get-Content $st -Raw | ConvertFrom-Json
"stations: " + @($after.radio_stations).Count + "; chosen: " + $after.radio_station
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-radio-test" $st -Force; Remove-Item "$st.before-radio-test"
"pictures: " + ((Get-ChildItem $out -Filter 'radio-*.png').Name -join ', ')
Select-String -Path "$out\log.txt" -Pattern '\[action\] Radio' | ForEach-Object Line
"settings restored"
