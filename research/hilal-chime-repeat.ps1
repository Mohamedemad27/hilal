# Like nusa-chime-repeat.ps1, for Hilal: a reminder due right away, left unanswered, the
# speakers recorded. Hilal's state is restored afterwards.
param([int]$Seconds = 40, [string]$Exe = 'K:\hilal\target\release\hilal.exe')
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\hilal-repeat'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-repeat-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$now = (Get-Date).Hour * 60 + (Get-Date).Minute
foreach ($kv in @{ from_min = $now - 7; to_min = $now + 53; goal_ml = 3000; bottle_ml = 250; in_bottle_ml = 0; handled_slot = -1; snooze_until = 0; enabled = $true; drunk_ml = 0; day = (Get-Date -Format 'yyyy-MM-dd') }.GetEnumerator()) {
  $j.water | Add-Member -Force $kv.Key $kv.Value }
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
$log = "$out\chime.log"; Remove-Item $log -ErrorAction SilentlyContinue
$rec = Start-Job -ArgumentList $Seconds -ScriptBlock { param($s) & K:\hilal\research\loopback.ps1 -Seconds $s -Out K:\hilal\research\hilal-repeat\long.wav }
Start-Sleep 2
$env:HILAL_FPS = $log
Start-Process $Exe
Remove-Item Env:HILAL_FPS
"hilal started 2 s into the recording"
Receive-Job $rec -Wait; Remove-Job $rec
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-repeat-test" $st -Force; Remove-Item "$st.before-repeat-test"
"chime log: " + ((Select-String -Path $log -Pattern 'chime' -ErrorAction SilentlyContinue).Count)
"state restored"
