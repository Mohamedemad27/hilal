# Nova mode's Apps page: opens the notch on it (one page after Music) and waits for the
# Start menu to be read and the icons to load; Hilal saves the page (HILAL_DUMP). Prints the
# pinned apps it chose. Settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q8 {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
'@
$cx = [Q8]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\apps'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-apps-test" -Force
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
[Q8]::SetCursorPos(1100, 700) | Out-Null
Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue
$env:HILAL_DUMP = $out; $env:HILAL_FPS = "$out\log.txt"; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
Start-Sleep 5
[Q8]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
[Q8]::SetCursorPos([int]($cx + 188), 69) | Out-Null; Start-Sleep -Milliseconds 3000
[Q8]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [Q8]::mouse_event(4, 0, 0, 0, 0)
[Q8]::SetCursorPos([int]($cx), 60) | Out-Null
Start-Sleep 8
$after = Get-Content $st -Raw | ConvertFrom-Json
"Start menu apps read: " + @($after.apps_catalog).Count
"pinned: " + (($after.apps_pinned | ForEach-Object name) -join ', ')
[Q8]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-apps-test" $st -Force; Remove-Item "$st.before-apps-test"
"pictures: " + ((Get-ChildItem $out -Filter 'apps*.png').Name -join ', ')
Select-String -Path "$out\log.txt" -Pattern '\[action\]|\[open\]|\[slow\]' | ForEach-Object Line
"settings restored"
