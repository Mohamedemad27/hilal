# Lyrics for the song playing now: opens the notch on the Music page, presses the lyrics
# bubble, and Hilal saves the lyrics view (HILAL_DUMP). For a song LRCLIB has only the words
# of, they show unsynced. Settings are put back afterwards.
param([string]$Exe = "$env:LOCALAPPDATA\Programs\Hilal\hilal.exe")
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QH {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
'@
$cx = [QH]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\plain-lyrics'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-lyrics-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force glass $false
$j.settings | Add-Member -Force shrink_over_maximized $false
$j.settings | Add-Member -Force hide_over_maximized $false
$j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.reminders | Add-Member -Force on $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
[QH]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; $env:HILAL_FPS = "$out\log.txt"; Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue
Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
Start-Sleep 6
[QH]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep 2
[QH]::SetCursorPos([int]($cx - 210 + 75), 116) | Out-Null; Start-Sleep 3
[QH]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QH]::mouse_event(4, 0, 0, 0, 0)
[QH]::SetCursorPos([int]($cx), 60) | Out-Null; Start-Sleep 5
# Back to how it was: the bubble again (lyrics off), then away.
[QH]::SetCursorPos([int]($cx - 260 + 53), 116) | Out-Null; Start-Sleep 1
[QH]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QH]::mouse_event(4, 0, 0, 0, 0); Start-Sleep 1
[QH]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-lyrics-test" $st -Force; Remove-Item "$st.before-lyrics-test"
"pictures: " + ((Get-ChildItem $out -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
Select-String -Path "$out\log.txt" -Pattern '\[action\] Lyrics' | ForEach-Object Line
Start-Process $Exe
"settings restored, Hilal running again"
