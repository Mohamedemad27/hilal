# Restarts Hilal with frame logging, opens/closes the notch a few times, prints the stats.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [string]$Log = 'K:\hilal\research\fps.txt', [int]$Rounds = 4)
$s = 'K:\hilal\research\snap.ps1'
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Remove-Item $Log -ErrorAction SilentlyContinue
& $s -X 960 -Y 700 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 10 | Out-Null
$env:HILAL_FPS = $Log
Start-Process $Exe
Remove-Item Env:HILAL_FPS
Start-Sleep 5   # let the welcome finish
for ($i = 0; $i -lt $Rounds; $i++) {
  & $s -X 960 -Y 3 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 1200 | Out-Null
  & $s -X 960 -Y 700 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 1200 | Out-Null
}
Start-Sleep 1
Get-Content $Log
