# The Claude popup the messy ways: switching sessions while it's open, double clicks,
# clicking a row while it's closing, and clicking inside it then moving away. After each,
# checks Hilal still answers and whether the popup closes once the mouse leaves.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class S2 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint f, uint t, out IntPtr r);
}
'@
[S2]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Start-Process $Exe; Start-Sleep 5
$sc = [S2]::GetDpiForSystem() / 96.0; $cx = [S2]::GetSystemMetrics(0) / 2
$pop = [S2]::FindWindow('HilalPopup', 'Hilal Claude')
function Click($x, $y, $ms) { [S2]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 80; [S2]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 40; [S2]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
function Row($r, $ms = 300) { Click ($cx - 10 * $sc) ((46 + 31 * $r) * $sc) $ms }
function Alive { $r = [IntPtr]::Zero; [S2]::SendMessageTimeout($pop, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 500, [ref]$r) -ne [IntPtr]::Zero }
function Away { [S2]::SetCursorPos([int]($cx + 700 * $sc), [int](900 * $sc)) | Out-Null; Start-Sleep -Milliseconds 1800 }
function Report($name) { $a = Alive; $v1 = [S2]::IsWindowVisible($pop); Away; $v2 = [S2]::IsWindowVisible($pop); "{0,-34} responding {1}  open {2}  after leaving still open {3}{4}" -f $name, $a, $v1, $v2, $(if (-not $a -or $v2) { '  <-- PROBLEM' } else { '' }) }
function Notch { [S2]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 700 }
Notch; for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188 * $sc) (69 * $sc) 700 }

Notch; Row 0; Row 1; Row 2; Row 0;                            Report 'switch sessions while open'
Notch; Row 1 30; Row 1 300;                                  Report 'double click a row'
Notch; Row 2 400; Away; Notch; Row 0 300;                    Report 'reopen right after it closed'
Notch; Row 0 400; [S2]::SetCursorPos([int]($cx + 700 * $sc), [int](900 * $sc)) | Out-Null; Start-Sleep -Milliseconds 520; Notch; Row 1 300
                                                             Report 'click a row while it is closing'
Notch; Row 0 500; Click ($cx) (250 * $sc) 300;               Report 'click inside the popup body'
Notch; Row 0 500; Click ($cx - 70 * $sc) (518 * $sc) 300;    Report 'click its message box'
"foreground is the popup: " + ([S2]::GetForegroundWindow() -eq $pop)
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
