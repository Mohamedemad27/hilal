# Drives the Download page with the real mouse: copies a link, opens the notch, goes to
# the page, presses Paste, picks MP3 or MP4, presses Download, and waits for the file.
# With HILAL_DUMP set, Hilal saves a picture of each stage into research\download.
param([string]$Url = 'https://www.youtube.com/watch?v=snhPW3STY3A', [ValidateSet('mp3','mp4')][string]$Kind = 'mp3', [int]$Wait = 120, [string]$Exe = 'K:\hilal\target\release\hilal.exe')
$out = 'K:\hilal\research\download'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class D {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
}
'@
[D]::SetProcessDPIAware() | Out-Null
Set-Clipboard -Value $Url
$dir = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'Downloads\Hilal'
$before = @(Get-ChildItem $dir -File -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName)
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue
$env:HILAL_DUMP = $out; $env:HILAL_FPS = "$out\log.txt"; Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
Start-Sleep 5
$sc = [D]::GetDpiForSystem() / 96.0; $cx = [D]::GetSystemMetrics(0) / 2
function At($x, $y) { [D]::SetCursorPos([int]($cx + ($x - 210) * $sc), [int]($y * $sc)) | Out-Null }
function Click($x, $y, $ms = 700) { At $x $y; Start-Sleep -Milliseconds 250; [D]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 60; [D]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
[D]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
for ($i = 0; $i -lt 4; $i++) { Click 398 69 900 }   # next page x4 -> Download
Click 350 48 900                                     # Paste
if ($Kind -eq 'mp3') { Click 72 86 } else { Click 140 86 }
Click 340 86 500                                     # Download
$t0 = Get-Date; $file = $null
while (((Get-Date) - $t0).TotalSeconds -lt $Wait -and -not $file) {
  At 200 100; Start-Sleep 1
  $file = Get-ChildItem $dir -File -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.FullName -and $_.Extension -eq ".$Kind" } | Select-Object -First 1
}
Start-Sleep 2
"file: " + $(if ($file) { "$($file.Name)  $([math]::Round($file.Length/1MB,1)) MB  after $([int]((Get-Date) - $t0).TotalSeconds) s" } else { 'none' })
[D]::SetCursorPos($cx, 600) | Out-Null; Start-Sleep 1
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Get-ChildItem $out -Filter 'download-*.png' | Select-Object -ExpandProperty Name
