# Small helpers for driving Nusa: real mouse moves/clicks and screenshots of the notch area.
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Runtime.InteropServices; using System.Threading;
public static class NU {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  public static void Move(int x, int y) { int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1); mouse_event(0x8001, x * 65535 / (sw - 1), y * 65535 / (sh - 1), 0, 0); }
  public static void Click(int x, int y) { Move(x, y); Thread.Sleep(120); mouse_event(2, 0, 0, 0, 0); Thread.Sleep(40); mouse_event(4, 0, 0, 0, 0); }
  public static void Shot(string path) { var b = new Bitmap(560, 200); Graphics.FromImage(b).CopyFromScreen(680, 0, 0, 0, b.Size); b.Save(path); }
}
'@
[NU]::SetProcessDPIAware() | Out-Null
function Hover { [NU]::Move(960, 40); Start-Sleep -Milliseconds 100; [NU]::Move(960, 3); Start-Sleep -Milliseconds 900 }
function Away { [NU]::Move(960, 600); Start-Sleep -Milliseconds 800 }
function Click($x, $y, $wait = 700) { [NU]::Click($x, $y); Start-Sleep -Milliseconds $wait }
function Shot($name) { [NU]::Shot("K:\hilal\research\nusa-water\$name.png") }
New-Item -ItemType Directory -Force K:\hilal\research\nusa-water | Out-Null
