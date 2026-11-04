# Nova mode's meeting toasts, from a made-up calendar (HILAL_CALENDAR_TEST): one meeting a
# minute and a few seconds from now (its "in 1 min" toast, then at its start the Join one)
# and one ten minutes away (its "in 10 min" toast). Hilal saves the toasts (HILAL_DUMP).
# Settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q9 { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); }
'@
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\meetings'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
$fmt = 'yyyyMMddTHHmmssZ'
$now = [DateTime]::UtcNow
$a = $now.AddSeconds(70); $b = $now.AddSeconds(10 * 60 + 20)
$ics = "BEGIN:VCALENDAR`r`nBEGIN:VEVENT`r`nUID:t1`r`nDTSTART:$($a.ToString($fmt))`r`nDTEND:$($a.AddMinutes(30).ToString($fmt))`r`nSUMMARY:Design review`r`nLOCATION:https://meet.google.com/abc-defg-hij`r`nEND:VEVENT`r`nBEGIN:VEVENT`r`nUID:t2`r`nDTSTART:$($b.ToString($fmt))`r`nDTEND:$($b.AddMinutes(15).ToString($fmt))`r`nSUMMARY:Weekly sync`r`nEND:VEVENT`r`nEND:VCALENDAR`r`n"
[IO.File]::WriteAllText("$out\test.ics", $ics)
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-meetings-test" -Force
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
[Q9]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; $env:HILAL_CALENDAR_TEST = "$out\test.ics"; $env:HILAL_FPS = "$out\log.txt"
Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_CALENDAR_TEST, Env:HILAL_FPS
Start-Sleep 85
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-meetings-test" $st -Force; Remove-Item "$st.before-meetings-test"
"pictures: " + ((Get-ChildItem $out -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
"settings restored"
