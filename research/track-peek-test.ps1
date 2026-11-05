# The new-song card (HILAL_TRACK_PEEK_TEST shows it 7 s after start), standing on the left
# edge and lying along the top: Hilal saves it (HILAL_DUMP), to see that no page shows under
# it. Settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QJ { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); }
'@
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\track-peek'; New-Item -ItemType Directory -Force $out | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-peek-test" -Force
foreach ($vertical in $true, $false) {
  $j = Get-Content "$st.before-peek-test" -Raw | ConvertFrom-Json
  $j.settings | Add-Member -Force nova $true
  $j.settings | Add-Member -Force glass $false
  $j.settings | Add-Member -Force vertical $vertical
  $j.settings | Add-Member -Force side_left $true
  $j.settings | Add-Member -Force anchor $(if ($vertical) { @(0, 500) } else { $null })
  $j.settings | Add-Member -Force docked $true
  $j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
  $j.water | Add-Member -Force enabled $false
  $j.prayer | Add-Member -Force alerts $false
  $j.reminders | Add-Member -Force on $false
  [IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
  $d = "$out\" + $(if ($vertical) { 'side' } else { 'top' }); New-Item -ItemType Directory -Force $d | Out-Null; Get-ChildItem $d -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
  [QJ]::SetCursorPos(1100, 700) | Out-Null
  $env:HILAL_DUMP = $d; $env:HILAL_TRACK_PEEK_TEST = '1'; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_TRACK_PEEK_TEST
  Start-Sleep 12
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
  "$(Split-Path $d -Leaf): " + ((Get-ChildItem $d -Filter *.png).Name -join ', ')
}
Copy-Item "$st.before-peek-test" $st -Force; Remove-Item "$st.before-peek-test"
Start-Process "$env:LOCALAPPDATA\Programs\Hilal\hilal.exe"
"settings restored, Hilal running again"
