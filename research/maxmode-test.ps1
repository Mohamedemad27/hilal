# Each choice for maximized windows (stay / shrink / hide): with a maximized window in
# front, what the notch does; then with that window made normal-sized again. Hilal's
# settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class M1 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
}
'@
[M1]::SetProcessDPIAware() | Out-Null
$sc = [M1]::GetDpiForSystem() / 96.0
$st = Join-Path $env:APPDATA 'Hilal\state.json'
function Pump($ms) { $sw = [Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt $ms) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 15 } }
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-maxmode-test" -Force
[M1]::SetCursorPos(1700, 900) | Out-Null
foreach ($mode in @(@('stay', $false, $false), @('shrink', $true, $false), @('hide', $false, $true))) {
  $j = Get-Content "$st.before-maxmode-test" -Raw | ConvertFrom-Json
  $j.settings | Add-Member -Force shrink_over_maximized $mode[1]
  $j.settings | Add-Member -Force hide_over_maximized $mode[2]
  [IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
  Start-Process $Exe; Pump 5000
  $notch = [M1]::FindWindow('HilalNotch', 'Hilal')
  function State { if (-not [M1]::IsWindowVisible($notch)) { 'hidden' } else { $r = New-Object M1+RECT; [M1]::GetWindowRgnBox($notch, [ref]$r) | Out-Null; 'shown {0}x{1}' -f [int](($r.R - $r.L) / $sc), [int](($r.B - $r.T) / $sc) } }
  $form = New-Object System.Windows.Forms.Form
  $form.Text = 'Maximized test'; $form.WindowState = 'Maximized'; $form.Show(); [M1]::SetForegroundWindow($form.Handle) | Out-Null
  Pump 3000
  $a = State
  $form.WindowState = 'Normal'; $form.Bounds = New-Object System.Drawing.Rectangle 200, 300, 600, 400; [M1]::SetForegroundWindow($form.Handle) | Out-Null
  Pump 3000
  $b = State
  "{0,-7} maximized window: {1,-16} normal window: {2}" -f $mode[0], $a, $b
  $form.Close(); Pump 300
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
}
Copy-Item "$st.before-maxmode-test" $st -Force; Remove-Item "$st.before-maxmode-test"
"settings restored"
