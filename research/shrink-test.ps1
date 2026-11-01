# A maximized window in front: the notch should shrink to its thin line, let clicks just
# under it through to the window, ignore a quick pass of the mouse, open when the mouse
# rests on it, and grow back once the window is gone. Reads the notch's size from its
# window region; Hilal's log goes to research\shrink.log.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class H1 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
}
'@
[H1]::SetProcessDPIAware() | Out-Null
$sc = [H1]::GetDpiForSystem() / 96.0; $cx = [H1]::GetSystemMetrics(0) / 2
$log = 'K:\hilal\research\shrink.log'
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Remove-Item $log -ErrorAction SilentlyContinue
$env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_FPS; Start-Sleep 5
$notch = [H1]::FindWindow('HilalNotch', 'Hilal')
if ($notch -eq [IntPtr]::Zero) { $notch = (Get-Process hilal).MainWindowHandle }
function Size { $r = New-Object H1+RECT; [H1]::GetWindowRgnBox($notch, [ref]$r) | Out-Null; "{0}x{1}" -f [int](($r.R - $r.L) / $sc), [int](($r.B - $r.T) / $sc) }
function Pump($ms) { $sw = [Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt $ms) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 15 } }
[H1]::SetCursorPos([int]($cx + 600 * $sc), [int](700 * $sc)) | Out-Null; Pump 1500
"before, notch size: " + (Size)
$form = New-Object System.Windows.Forms.Form
$form.Text = 'Maximized test'; $form.BackColor = [System.Drawing.Color]::FromArgb(40, 44, 52)
$form.WindowState = 'Maximized'; $form.Show(); [H1]::SetForegroundWindow($form.Handle) | Out-Null
Pump 3000
"maximized window in front, notch size: " + (Size)
# Whose window gets a click just under the line, and on the line itself?
function Owner($y) { $pt = New-Object H1+POINT; $pt.X = [int]$cx; $pt.Y = [int]($y * $sc); $w = [H1]::GetAncestor([H1]::WindowFromPoint($pt), 2); if ($w -eq $notch) { 'Hilal' } else { 'the window under it' } }
"a click 14 dp down goes to: " + (Owner 14) + ";  on the line (2 dp): " + (Owner 2)
# A quick pass over the line.
[H1]::SetCursorPos([int]$cx, [int](2 * $sc)) | Out-Null; Pump 120
[H1]::SetCursorPos([int]($cx + 600 * $sc), [int](700 * $sc)) | Out-Null; Pump 600
"after a quick pass, notch size: " + (Size)
# Resting on it.
[H1]::SetCursorPos([int]$cx, [int](2 * $sc)) | Out-Null; Pump 1200
"after resting on it, notch size: " + (Size)
[H1]::SetCursorPos([int]($cx + 600 * $sc), [int](700 * $sc)) | Out-Null; Pump 1200
$form.Close(); Pump 3000
"window closed, notch size: " + (Size)
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Get-Content $log -ErrorAction SilentlyContinue | Where-Object { $_ -match '\[open\]' }
