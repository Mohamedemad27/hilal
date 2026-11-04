# Nova mode's address bar slot: a new blank Edge window in front; the notch should move into
# the right third of its address bar (a slim pill), and become a thin line while the address
# is being typed (Ctrl+L). Keys are pressed only while that Edge window is in front, and only
# it is closed; settings are put back afterwards.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
public static class QD {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, int d, UIntPtr e);
  public static string Title(IntPtr h) { var sb = new StringBuilder(256); GetWindowText(h, sb, 256); return sb.ToString(); }
  public static IntPtr Find(string part) { IntPtr found = IntPtr.Zero; EnumWindows((h, l) => { if (IsWindowVisible(h) && Title(h).Contains(part)) { found = h; return false; } return true; }, IntPtr.Zero); return found; }
}
'@
[QD]::SetProcessDPIAware() | Out-Null
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$log = 'K:\hilal\research\addr-log.txt'
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-addr-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$j.settings | Add-Member -Force nova $true
$j.settings | Add-Member -Force vertical $false
$j.settings | Add-Member -Force anchor $null
$j.settings | Add-Member -Force docked $true
$j.settings | Add-Member -Force shrink_over_maximized $false
$j | Add-Member -Force brief_day ([int](Get-Date -Format 'yyyyMMdd'))
$j.water | Add-Member -Force enabled $false
$j.prayer | Add-Member -Force alerts $false
$j.reminders | Add-Member -Force on $false
$j.features | Add-Member -Force addr_island $true
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
[QD]::SetCursorPos(1100, 900) | Out-Null
Remove-Item $log -ErrorAction SilentlyContinue
$env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_FPS; Start-Sleep 5
$h = [QD]::FindWindow('HilalNotch', 'Hilal')
function NotchAt { $r = New-Object QD+RECT; [QD]::GetWindowRect($h, [ref]$r) | Out-Null; $g = New-Object QD+RECT; [QD]::GetWindowRgnBox($h, [ref]$g) | Out-Null; "notch at x {0}..{1}, y {2}..{3}" -f ($r.L + $g.L), ($r.L + $g.R), ($r.T + $g.T), ($r.T + $g.B) }
"before the browser: " + (NotchAt)
Start-Process msedge -ArgumentList '--new-window', 'about:blank'
$edge = [IntPtr]::Zero
for ($i = 0; $i -lt 20 -and $edge -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 500; $edge = [QD]::Find('about:blank') }
if ($edge -eq [IntPtr]::Zero) { "no Edge test window appeared" } else {
  # In front for sure (a tap of Alt lets this script hand over the foreground).
  [QD]::ShowWindow($edge, 9) | Out-Null
  [QD]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [QD]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero); [QD]::SetForegroundWindow($edge) | Out-Null; Start-Sleep 2
  $br = New-Object QD+RECT; [QD]::GetWindowRect($edge, [ref]$br) | Out-Null
  "Edge test window '$([QD]::Title($edge))' at x {0}..{1}, y {2}; in front: {3}" -f $br.L, $br.R, $br.T, ([QD]::GetForegroundWindow() -eq $edge)
  "with the browser in front (typing in its new tab's address bar): " + (NotchAt)
  if ([QD]::GetForegroundWindow() -eq $edge) {
    # A click on the blank page takes the typing out of the address bar.
    [QD]::SetCursorPos([int](($br.L + $br.R) / 2), [int](($br.T + $br.B) / 2)) | Out-Null; Start-Sleep -Milliseconds 300
    [QD]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [QD]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep 1.5
    [QD]::SetCursorPos(1100, 900) | Out-Null; Start-Sleep 1
    "after a click on the page: " + (NotchAt)
    foreach ($v in 0x11, 0x4C) { [QD]::keybd_event($v, 0, 0, [UIntPtr]::Zero) }; Start-Sleep -Milliseconds 50; foreach ($v in 0x4C, 0x11) { [QD]::keybd_event($v, 0, 2, [UIntPtr]::Zero) }
    Start-Sleep 1.5
    "typing in the address bar: " + (NotchAt)
  } else { "not in front: no keys pressed" }
  [QD]::PostMessage($edge, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; "closed the Edge test window"
  Start-Sleep 2
  "browser gone: " + (NotchAt)
}
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-addr-test" $st -Force; Remove-Item "$st.before-addr-test"
if (Test-Path $log) { Select-String -Path $log -Pattern '\[addr\]' | ForEach-Object Line }
"settings restored"
