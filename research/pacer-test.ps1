# The frame loop's pacing: Hilal is run normally, with its vsync wait skipped (as some
# drivers do) but the pacer on, and with both off. Each time the notch is held open for a
# few seconds and Hilal's frame rate (HILAL_FPS) and GPU use are read.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class F1 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
}
'@
[F1]::SetProcessDPIAware() | Out-Null
$cx = [F1]::GetSystemMetrics(0) / 2
foreach ($case in @(@('normal', @{}), @('no vsync, pacer on', @{ HILAL_NO_VSYNC = '1' }), @('no vsync, pacer off', @{ HILAL_NO_VSYNC = '1'; HILAL_NO_PACER = '1' }))) {
  $log = "K:\hilal\research\pacer-$($case[0] -replace '[^a-z]', '').log"; Remove-Item $log -ErrorAction SilentlyContinue
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
  $env:HILAL_FPS = $log; foreach ($k in $case[1].Keys) { Set-Item "Env:$k" $case[1][$k] }
  $p = Start-Process $Exe -PassThru
  Remove-Item Env:HILAL_FPS, Env:HILAL_NO_VSYNC, Env:HILAL_NO_PACER -ErrorAction SilentlyContinue
  Start-Sleep 5
  [F1]::SetCursorPos([int]$cx, 5) | Out-Null; Start-Sleep 1
  $s = Get-Counter "\GPU Engine(pid_$($p.Id)_*engtype_3d)\Utilization Percentage" -SampleInterval 1 -MaxSamples 4 -ErrorAction SilentlyContinue
  $gpu = ($s | ForEach-Object { ($_.CounterSamples | Measure-Object CookedValue -Sum).Sum } | Measure-Object -Average).Average
  [F1]::SetCursorPos([int]($cx + 700), 900) | Out-Null; Start-Sleep 1
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
  $fps = Get-Content $log | Where-Object { $_ -match 'open=true' } | ForEach-Object { if ($_ -match '^\s*(\d+) fps') { [int]$matches[1] } } | Measure-Object -Maximum
  $held = Get-Content $log | Where-Object { $_ -match '\[pacer\]' } | Select-Object -Last 1
  "{0,-20} notch open: up to {1,5} fps   GPU 3D {2,5:N1}%   {3}" -f $case[0], $fps.Maximum, $gpu, $held
}
