# The mic badge standing on the left side (an app using the mic, as Discord in a call), with
# HILAL_MIC_DRY: clicks only flip a pretend mute (the real microphone is never touched).
# A click on the closed column's mic (it must not open), then the page opened and a click on
# the mic in its wave. Pictures and the log's opens and clicks. Settings put back.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QS {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
'@
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\mic-side'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-mic-side-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force glass $false
$j.settings | Add-Member -Force vertical $true
$j.settings | Add-Member -Force side_left $true
$j.settings | Add-Member -Force anchor @(0, 500)
$j.settings | Add-Member -Force shrink_over_maximized $false
$j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.reminders | Add-Member -Force on $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
function Click($x, $y) { [QS]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 1200; [QS]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QS]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 1500 }
[QS]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; $env:HILAL_MIC_DRY = '1'; $env:HILAL_FPS = "$out\log.txt"; Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue
Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_MIC_DRY, Env:HILAL_FPS
Start-Sleep 9
"--- the closed column's mic"
# The column is shorter with music playing: its height from its picture mid-way into closing.
# (The window's middle is at 500, the mic 12 below the column's top.)
$col = Get-ChildItem $out -Filter 'mid-44x*.png' | Where-Object Name -ne 'mid-44x168.png' | Sort-Object LastWriteTime | Select-Object -Last 1   # (168: a toast)
$h = if ($col) { [int]($col.BaseName -replace 'mid-44x', '') } else { 244 }
$micY = 500 - $h / 2 + 12
"column $h tall, mic at y $micY"
Click 22 $micY
[QS]::SetCursorPos(22, $micY + 50) | Out-Null; Start-Sleep 2   # the column itself: it opens
Copy-Item "$out\music.png" "$out\page-muted.png" -ErrorAction SilentlyContinue   # (before the click puts it back)
"--- the page's wave"
Click 210 418
[QS]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 2
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-mic-side-test" $st -Force; Remove-Item "$st.before-mic-side-test"
"pictures: " + ((Get-ChildItem $out -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
Select-String -Path "$out\log.txt" -Pattern '\[mic\]|\[action\]|\[open\]' | ForEach-Object Line
Start-Process "$env:LOCALAPPDATA\Programs\Hilal\hilal.exe"
"settings restored, Hilal running again"
