# Ctrl+Alt+H with the mouse away: the notch opens, closes by itself after a few seconds,
# and a second press closes it at once. Reads the notch's size from its window region.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class K1 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetDpiForSystem();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern int GetWindowRgnBox(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
'@
[K1]::SetProcessDPIAware() | Out-Null
$sc = [K1]::GetDpiForSystem() / 96.0
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
[K1]::SetCursorPos(1700, 900) | Out-Null
$log = 'K:\hilal\research\hotkey.log'; Remove-Item $log -ErrorAction SilentlyContinue
$env:HILAL_FPS = $log; Start-Process $Exe; Remove-Item Env:HILAL_FPS; Start-Sleep 6
$h = [K1]::FindWindow('HilalNotch', 'Hilal')
# Real key presses (SendKeys doesn't reach global hotkeys).
function Press { foreach ($v in 0x11, 0x12, 0x48) { [K1]::keybd_event($v, 0, 0, [UIntPtr]::Zero) }; Start-Sleep -Milliseconds 50; foreach ($v in 0x48, 0x12, 0x11) { [K1]::keybd_event($v, 0, 2, [UIntPtr]::Zero) } }
function Size { $r = New-Object K1+RECT; [K1]::GetWindowRgnBox($h, [ref]$r) | Out-Null; '{0}x{1}' -f [int](($r.R - $r.L) / $sc), [int](($r.B - $r.T) / $sc) }
"before:                  " + (Size)
Press; Start-Sleep -Milliseconds 1200
"after Ctrl+Alt+H:        " + (Size)
Start-Sleep 9
"9 s later (still open):  " + (Size)
Press; Start-Sleep -Milliseconds 1200
"pressed again (closes):  " + (Size)
Press; Start-Sleep -Milliseconds 1500
"and again (opens):       " + (Size)
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Select-String -Path $log -Pattern '\[hotkey\]' | ForEach-Object Line
