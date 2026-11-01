# Opens and closes the Claude popup many times from different rows, the way a person
# would (hover the notch, click a session, look, move away), checking each time that it
# opened, drew, and closed, with Hilal logging its frame timing to research\stress.log.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe', [int]$Rounds = 15)
$log = 'K:\hilal\research\stress.log'; Remove-Item $log -ErrorAction SilentlyContinue
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class S1 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint f, uint t, out IntPtr r);
}
'@
[S1]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
$env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_FPS
Start-Sleep 5
$sc = [S1]::GetDpiForSystem() / 96.0; $cx = [S1]::GetSystemMetrics(0) / 2
$pop = [S1]::FindWindow('HilalPopup', 'Hilal Claude')
function Click($x, $y, $ms) { [S1]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 120; [S1]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 50; [S1]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
# Does Hilal's window answer within 500 ms? (A hung message loop doesn't.)
function Alive { $r = [IntPtr]::Zero; [S1]::SendMessageTimeout($pop, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 500, [ref]$r) -ne [IntPtr]::Zero }
[S1]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188 * $sc) (69 * $sc) 700 }
$fails = 0
for ($r = 1; $r -le $Rounds; $r++) {
  $row = ($r - 1) % 3
  [S1]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 700      # hover: notch opens
  "--- round $r row $row click" | Out-File -Append -Encoding utf8 $log
  $t = [Diagnostics.Stopwatch]::StartNew()
  Click ($cx - 10 * $sc) ((46 + 31 * $row) * $sc) 0
  $shownAt = $null
  while ($t.ElapsedMilliseconds -lt 1500) { if ([S1]::IsWindowVisible($pop)) { $shownAt = $t.ElapsedMilliseconds; break }; Start-Sleep -Milliseconds 10 }
  [S1]::SetCursorPos([int]$cx, [int](300 * $sc)) | Out-Null; Start-Sleep -Milliseconds 600   # look at it
  $alive = Alive
  [S1]::SetCursorPos([int]($cx + 700 * $sc), [int](900 * $sc)) | Out-Null                  # move away
  $closed = $null; $t2 = [Diagnostics.Stopwatch]::StartNew()
  while ($t2.ElapsedMilliseconds -lt 2500) { if (-not [S1]::IsWindowVisible($pop)) { $closed = $t2.ElapsedMilliseconds; break }; Start-Sleep -Milliseconds 20 }
  $ok = ($shownAt -ne $null) -and $alive -and ($closed -ne $null)
  if (-not $ok) { $fails++ }
  "round {0,2} row {1}: shown after {2} ms, responding {3}, closed after {4} ms {5}" -f $r, $row, $shownAt, $alive, $closed, $(if ($ok) { '' } else { '<-- PROBLEM' })
  Start-Sleep -Milliseconds 400
}
"problems: $fails of $Rounds"
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
