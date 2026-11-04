# Nova mode's themes: for each, the closed notch (its clock slide) and the open Music page,
# saved by Hilal (HILAL_DUMP), glass on. Settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [string[]]$Themes = @('galaxy', 'nody', 'retro', 'bakka', 'snow', 'outline'))
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QC {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
}
'@
$cx = [QC]::GetSystemMetrics(0) / 2
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\themes'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-themes-test" -Force
foreach ($t in $Themes) {
  $j = Get-Content "$st.before-themes-test" -Raw | ConvertFrom-Json
  $j.settings | Add-Member -Force nova $true
  $j.settings | Add-Member -Force glass $true
  $j.settings | Add-Member -Force vertical $false
  $j.settings | Add-Member -Force anchor $null
  $j.settings | Add-Member -Force docked $true
  $j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
  $j.water | Add-Member -Force enabled $false
  $j.prayer | Add-Member -Force alerts $false
  $j.reminders | Add-Member -Force on $false
  $j.features | Add-Member -Force theme $t
  [IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
  $d = "$out\$t"; New-Item -ItemType Directory -Force $d | Out-Null; Get-ChildItem $d -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
  [QC]::SetCursorPos(1100, 700) | Out-Null
  $env:HILAL_DUMP = $d; Start-Process $Exe; Remove-Item Env:HILAL_DUMP
  Start-Sleep 6
  [QC]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep 2
  [QC]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 1
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
  "${t}: " + ((Get-ChildItem $d -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
}
Copy-Item "$st.before-themes-test" $st -Force; Remove-Item "$st.before-themes-test"
"settings restored"
