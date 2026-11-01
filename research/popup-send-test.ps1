# Uses the Claude popup end to end: opens it, types a message, presses Enter (Hilal
# pastes it into the Claude app's message box; nothing is sent there), then takes a
# screenshot so the result can be checked. Uses the installed Hilal.
param([string]$Exe = "$env:LOCALAPPDATA\Programs\Hilal\hilal.exe", [string]$Text = 'Hello from the Hilal popup')
$out = 'K:\hilal\research\claude'; New-Item -ItemType Directory -Force $out | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class P4 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
}
'@
[P4]::SetProcessDPIAware() | Out-Null
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Start-Process $Exe; Start-Sleep 5
$sc = [P4]::GetDpiForSystem() / 96.0; $cx = [P4]::GetSystemMetrics(0) / 2
$h = [P4]::FindWindow('HilalPopup', 'Hilal Claude')
function Click($x, $y, $ms = 900) { [P4]::SetCursorPos([int]$x, [int]$y) | Out-Null; Start-Sleep -Milliseconds 250; [P4]::mouse_event(2,0,0,0,0); Start-Sleep -Milliseconds 60; [P4]::mouse_event(4,0,0,0,0); Start-Sleep -Milliseconds $ms }
[P4]::SetCursorPos($cx, 5) | Out-Null; Start-Sleep -Milliseconds 1200
for ($i = 0; $i -lt 5; $i++) { Click ($cx + 188 * $sc) (69 * $sc) }
Start-Sleep 1
Click ($cx - 10 * $sc) (46 * $sc) 1200                          # first session -> popup
"popup open: " + [P4]::IsWindowVisible($h)
Click ($cx - 220 * $sc + 150 * $sc) (150 * $sc + 368 * $sc) 600 # the message box
[System.Windows.Forms.SendKeys]::SendWait($Text)
Start-Sleep -Milliseconds 500
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Start-Sleep 2
$fg = [P4]::GetForegroundWindow(); $sb = New-Object System.Text.StringBuilder 256; [P4]::GetWindowText($fg, $sb, 256) | Out-Null
"foreground now: " + $sb.ToString()
$w = [P4]::GetSystemMetrics(0); $hh = [P4]::GetSystemMetrics(1)
$bmp = New-Object System.Drawing.Bitmap $w, $hh; $g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0, 0, 0, 0, $bmp.Size); $bmp.Save("$out\after-send.png"); $g.Dispose(); $bmp.Dispose()
"screenshot: $out\after-send.png"
