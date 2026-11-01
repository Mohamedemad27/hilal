# Triggers Nusa's water reminder, leaves it unanswered, and records the speakers for a
# while to see whether the chime repeats. Nusa's water file is restored afterwards.
param([int]$Seconds = 190)
$n = Join-Path $env:APPDATA 'Nusa'
$out = 'K:\hilal\research\nusa-repeat'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal, nusa -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$n\water.txt" "$out\water.txt.orig" -Force
$now = (Get-Date).Hour * 60 + (Get-Date).Minute
$lines = Get-Content "$n\water.txt" | ForEach-Object {
  switch -regex ($_) {
    '^bottle=' { 'bottle=250' } '^goal=' { 'goal=3000' } '^start=' { "start=$($now - 7)" } '^end=' { "end=$($now + 53)" }
    '^day=' { 'day=' + (Get-Date -Format 'yyyyMMdd') } '^taken=' { 'taken=0' } '^inb=' { 'inb=0' } '^fired=' { 'fired=0' } '^on=' { 'on=1' }
    default { $_ } } }
[IO.File]::WriteAllLines("$n\water.txt", $lines)
$rec = Start-Job -ArgumentList $Seconds -ScriptBlock { param($s) & K:\hilal\research\loopback.ps1 -Seconds $s -Out K:\hilal\research\nusa-repeat\long.wav }
Start-Sleep 2
$t0 = Get-Date
Start-Process "$env:LOCALAPPDATA\Programs\Nusa\nusa.exe"
"nusa started at " + $t0.ToString('HH:mm:ss') + " (2 s into the recording)"
Receive-Job $rec -Wait; Remove-Job $rec
"fired after: " + ((Get-Content "$n\water.txt" | Select-String 'fired') -join ' ')
Get-Process nusa -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$out\water.txt.orig" "$n\water.txt" -Force
"Nusa water restored"
