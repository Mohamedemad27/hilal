# Nova mode's toasts for real: Caps Lock (pressed twice, so it ends as it was), the volume
# (one step up and back down), and copies: a sum, a color, a link and an image (saved to
# Pictures\Hilal and put on the Shelf; the test deletes it). Hilal saves each toast
# (HILAL_DUMP). The clipboard's text and the settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Q6 {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
}
'@
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\toasts'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-toasts-test" -Force
$clipBefore = Get-Clipboard -Raw -ErrorAction SilentlyContinue
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force glass $false
$j.water | Add-Member -Force enabled $false
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
[Q6]::SetCursorPos(1100, 700) | Out-Null
$env:HILAL_DUMP = $out; $env:HILAL_FPS = "$out\log.txt"; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
Start-Sleep 7
function Key($vk) { [Q6]::keybd_event($vk, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [Q6]::keybd_event($vk, 0, 2, [UIntPtr]::Zero) }
Key 0x14; Start-Sleep 2.5; Key 0x14; Start-Sleep 2.5          # Caps Lock on, then off
Key 0xAF; Start-Sleep 2.5; Key 0xAE; Start-Sleep 2.5          # volume up, then down
Set-Clipboard -Value '12*3+4'; Start-Sleep 3
Set-Clipboard -Value '#ff8800'; Start-Sleep 3
Set-Clipboard -Value 'https://github.com/Mohamedemad27/hilal'; Start-Sleep 3
$bmp = New-Object System.Drawing.Bitmap 320, 200
$gr = [System.Drawing.Graphics]::FromImage($bmp); $gr.Clear([System.Drawing.Color]::SteelBlue); $gr.FillEllipse([System.Drawing.Brushes]::Gold, 110, 50, 100, 100); $gr.Dispose()
[System.Windows.Forms.Clipboard]::SetImage($bmp); $bmp.Dispose(); Start-Sleep 4
$after = Get-Content $st -Raw | ConvertFrom-Json
"shelf now: " + ($after.shelf -join ' | ')
"leaves the shelf at: " + (($after.shelf_until | ForEach-Object { $_ -join ' @ ' }) -join ' | ')
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
foreach ($p in $after.shelf) { if ($p -like '*\Pictures\Hilal\Screenshot *') { Remove-Item $p -ErrorAction SilentlyContinue; "deleted test file $p" } }
Copy-Item "$st.before-toasts-test" $st -Force; Remove-Item "$st.before-toasts-test"
if ($clipBefore) { Set-Clipboard -Value $clipBefore } else { Set-Clipboard -Value $null }
"toast pictures: " + ((Get-ChildItem $out -Filter 'toast-*.png' | Where-Object Name -notmatch 'mid-').Name -join ', ')
"settings and clipboard restored"
