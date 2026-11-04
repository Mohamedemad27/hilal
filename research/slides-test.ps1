# Nova mode's closed notch for 20 s (each slide shows 6 s), with Hilal saving a picture of
# each slide (HILAL_DUMP), and the weather it found. Settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q7 { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); }
'@
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\slides'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-slides-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force glass $false
$j.settings | Add-Member -Force vertical $false
$j.settings | Add-Member -Force anchor $null
$j.settings | Add-Member -Force docked $true
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.reminders | Add-Member -Force on $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
[Q7]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; Start-Process $Exe; Remove-Item Env:HILAL_DUMP
Start-Sleep 22
$after = Get-Content $st -Raw | ConvertFrom-Json
"weather: " + ($after.weather | ConvertTo-Json -Compress -Depth 4)
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-slides-test" $st -Force; Remove-Item "$st.before-slides-test"
"pictures: " + ((Get-ChildItem $out -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
"settings restored"
