# Measures Hilal's own GPU (3D and copy engines) and CPU use in each state: closed, notch
# open, Claude popup open. Optionally starts a given exe first. Prints averages.
param([string]$Exe = '', [int]$Seconds = 6, [switch]$NoPopup)
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class G1 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
}
'@
[G1]::SetProcessDPIAware() | Out-Null
if ($Exe) { Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1; Start-Process $Exe; Start-Sleep 6 }
$p = Get-Process hilal | Select-Object -First 1
$cores = (Get-CimInstance Win32_Processor | Measure-Object NumberOfLogicalProcessors -Sum).Sum
$sc = [G1]::GetDpiForSystem() / 96.0; $cx = [G1]::GetSystemMetrics(0) / 2
function Take-Sample($name) {
  $c0 = (Get-Process -Id $p.Id).TotalProcessorTime.TotalSeconds
  $s = Get-Counter "\GPU Engine(pid_$($p.Id)_*)\Utilization Percentage" -SampleInterval 1 -MaxSamples $Seconds -ErrorAction SilentlyContinue
  $c1 = (Get-Process -Id $p.Id).TotalProcessorTime.TotalSeconds
  $d3 = 0; $cp = 0
  foreach ($set in $s) { foreach ($x in $set.CounterSamples) { if ($x.InstanceName -match 'engtype_3d') { $d3 += $x.CookedValue } elseif ($x.InstanceName -match 'engtype_copy') { $cp += $x.CookedValue } } }
  "{0,-22} GPU 3D {1,5:N1}%   GPU copy {2,5:N1}%   CPU {3,5:N1}% of one core" -f $name, ($d3 / $Seconds), ($cp / $Seconds), (($c1 - $c0) / $Seconds * 100)
}
function Click($x, $y, $ms) { [G1]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 120; [G1]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 50; [G1]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
[G1]::SetCursorPos([int]($cx + 700 * $sc), [int](900 * $sc)) | Out-Null; Start-Sleep 2
Take-Sample 'closed'
[G1]::SetCursorPos([int]$cx, 5) | Out-Null; Start-Sleep 1
Take-Sample 'notch open'
if (-not $NoPopup) {
  for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188 * $sc) (69 * $sc) 600 }
  Click ($cx - 10 * $sc) (46 * $sc) 400
  [G1]::SetCursorPos([int]$cx, [int](300 * $sc)) | Out-Null; Start-Sleep 1
  Take-Sample 'Claude popup open'
}
[G1]::SetCursorPos([int]($cx + 700 * $sc), [int](900 * $sc)) | Out-Null
