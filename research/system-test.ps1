# Opens Hilal's System page and holds it open, with frame logging and dumps.
param([string]$Dump)
$s = 'K:\hilal\research\snap.ps1'
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Remove-Item K:\hilal\research\fps.txt -ErrorAction SilentlyContinue
$env:HILAL_FPS = 'K:\hilal\research\fps.txt'; $env:HILAL_DUMP = $Dump
Start-Process K:\hilal\target\release\hilal.exe
Remove-Item Env:HILAL_FPS, Env:HILAL_DUMP
Start-Sleep 5
& $s -X 960 -Y 3 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 900 | Out-Null
& $s -X 772 -Y 69 -Click -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 6000 | Out-Null
& $s -X 960 -Y 700 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 1000 | Out-Null
Get-Content K:\hilal\research\fps.txt
