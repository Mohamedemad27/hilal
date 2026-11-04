# Shows chosen reminders right away (HILAL_REMINDER_TEST=<index>) and has Hilal save each
# card (HILAL_DUMP), glass off so the frame shows the card itself. Settings are put back.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [int[]]$Items = @(2, 7, 77, 117, 59))
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\reminder'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-reminder-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force glass $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
foreach ($i in $Items) {
  $d = "$out\dump-$i"; New-Item -ItemType Directory -Force $d | Out-Null
  Get-ChildItem $d -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
  $env:HILAL_DUMP = $d; $env:HILAL_REMINDER_TEST = "$i"; $env:HILAL_FPS = "$d\log.txt"
  Start-Process $Exe
  Remove-Item Env:HILAL_DUMP, Env:HILAL_REMINDER_TEST, Env:HILAL_FPS
  Start-Sleep 9
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
  if (Test-Path "$d\reminder-peek.png") { Copy-Item "$d\reminder-peek.png" "$out\card-$i.png" -Force; "card $i saved" } else { "card ${i}: no frame (" + ((Get-ChildItem $d -Filter *.png).Name -join ', ') + ")" }
  Select-String -Path "$d\log.txt" -Pattern '\[reminder\]' | ForEach-Object { $_.Line }
}
Copy-Item "$st.before-reminder-test" $st -Force; Remove-Item "$st.before-reminder-test"
"settings restored"
