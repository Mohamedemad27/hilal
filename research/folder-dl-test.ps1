# Nova mode's browser download toasts: a fake download in the Downloads folder (a
# .crdownload that grows for 3 s, then is renamed to the finished name, as Chrome does).
# Hilal saves the toasts (HILAL_DUMP). The test file is deleted and the settings put back.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QA { [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); }
'@
$dl = (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\folder-dl'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-dl-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force glass $false
$j.settings | Add-Member -Force vertical $false
$j.settings | Add-Member -Force anchor $null
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.reminders | Add-Member -Force on $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
[QA]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; Start-Process $Exe; Remove-Item Env:HILAL_DUMP
Start-Sleep 6
$part = Join-Path $dl 'hilal-test.bin.crdownload'; $final = Join-Path $dl 'hilal-test.bin'
"downloads folder: $dl"
$fs = [IO.File]::Open($part, 'Create')
for ($i = 0; $i -lt 6; $i++) { $fs.Write((New-Object byte[] 65536), 0, 65536); $fs.Flush(); Start-Sleep -Milliseconds 500 }
$fs.Close()
Move-Item $part $final; Start-Sleep 5
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Remove-Item $final -ErrorAction SilentlyContinue; "test file deleted: " + (-not (Test-Path $final))
Copy-Item "$st.before-dl-test" $st -Force; Remove-Item "$st.before-dl-test"
"pictures: " + ((Get-ChildItem $out -Filter 'toast-*.png' | Where-Object Name -notmatch 'mid-').Name -join ', ')
"settings restored"
