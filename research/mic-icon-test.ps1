# The mic icon on the notch (an app using the mic, as Discord in a call), with HILAL_MIC_DRY:
# clicks only flip a pretend mute (the real microphone is never touched). Pictures of the
# closed notch and the open page, before and after a click on the icon. Settings put back.
# -Minimal: the thin line instead (as over a maximized window), pictures only.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [switch]$Minimal)
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QK {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
'@
$cx = [QK]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\mic-icon'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-mic-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force glass $false
$j.settings | Add-Member -Force vertical $false
$j.settings | Add-Member -Force anchor $null
$j.settings | Add-Member -Force docked $true
$j.settings | Add-Member -Force shrink_over_maximized $false
$j.settings | Add-Member -Force minimal ([bool]$Minimal)
$j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.reminders | Add-Member -Force on $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
function Click($x, $y) { [QK]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 300; [QK]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QK]::mouse_event(4, 0, 0, 0, 0); Start-Sleep -Milliseconds 900 }
[QK]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; $env:HILAL_MIC_DRY = '1'; $env:HILAL_FPS = "$out\log.txt"; Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue
Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_MIC_DRY, Env:HILAL_FPS
Start-Sleep 9
Click ($cx + 101) 16                # the mic on the closed notch (it stays closed under it)
[QK]::SetCursorPos($cx, 16) | Out-Null; Start-Sleep 1; [QK]::SetCursorPos($cx, 70) | Out-Null; Start-Sleep 2
Click ($cx + 182) 19                # the mic on the open page
[QK]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 2
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-mic-test" $st -Force; Remove-Item "$st.before-mic-test"
"pictures: " + ((Get-ChildItem $out -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
Select-String -Path "$out\log.txt" -Pattern '\[mic\]|\[action\] Mic' | ForEach-Object Line
Start-Process "$env:LOCALAPPDATA\Programs\Hilal\hilal.exe"
"settings restored, Hilal running again"
