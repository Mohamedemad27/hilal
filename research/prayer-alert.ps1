# Shows a prayer alert right away (HILAL_PRAYER_TEST), records the speakers, and has
# Hilal save the peek (HILAL_DUMP). Glass off so the frame shows what screen capture would.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [int]$Prayer = 4)
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\prayer-alert'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-prayer-test" -Force
$rec = Start-Job { & K:\hilal\research\loopback.ps1 -Seconds 22 -Out K:\hilal\research\prayer-alert\sound.wav }
Start-Sleep 2
$env:HILAL_DUMP = $out; $env:HILAL_PRAYER_TEST = "$Prayer"; $env:HILAL_FPS = "$out\log.txt"
Start-Process $Exe
Remove-Item Env:HILAL_DUMP, Env:HILAL_PRAYER_TEST, Env:HILAL_FPS
Receive-Job $rec -Wait; Remove-Job $rec
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-prayer-test" $st -Force; Remove-Item "$st.before-prayer-test"
Select-String -Path "$out\log.txt" -Pattern 'prayer|chime' | ForEach-Object { $_.Line }
Get-ChildItem $out -Filter *.png | Select-Object -ExpandProperty Name
