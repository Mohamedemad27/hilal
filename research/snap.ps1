# Usage: snap.ps1 -X 960 -Y 10 -Out file.png [-Left 0 -Top 0 -Width 0 -Height 0] [-Wait 900] [-Click]
# Moves the cursor, waits, and saves a screenshot (full screen, or the given region in physical px).
param([int]$X = -1, [int]$Y = -1, [string]$Out, [int]$Left = 0, [int]$Top = 0, [int]$Width = 0, [int]$Height = 0, [int]$Wait = 900, [switch]$Click, [switch]$Info)

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
public static class W {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  public static List<string> Windows(int pid) {
    var o = new List<string>();
    EnumWindows((h, l) => { int p; GetWindowThreadProcessId(h, out p); if (p == pid) { RECT r; GetWindowRect(h, out r); var c = new StringBuilder(256); GetClassName(h, c, 256);
      o.Add(String.Format("{0} vis={1} rect=({2},{3})-({4},{5}) ex=0x{6:X} class={7}", h, IsWindowVisible(h), r.L, r.T, r.R, r.B, GetWindowLong(h, -20), c)); } return true; }, IntPtr.Zero);
    return o;
  }
}
'@
[W]::SetProcessDPIAware() | Out-Null
if ($Info) {
  "screen=$([W]::GetSystemMetrics(0))x$([W]::GetSystemMetrics(1))"
  foreach ($p in Get-Process nusa -ErrorAction SilentlyContinue) { [W]::Windows($p.Id) }
  return
}
if ($X -ge 0) {
  # A real (injected) mouse move in small steps, so low-level mouse hooks see it.
  $sw = [W]::GetSystemMetrics(0); $sh = [W]::GetSystemMetrics(1)
  foreach ($dy in 12, 6, 0) {
    $ax = [int](($X * 65535) / ($sw - 1)); $ay = [int]((([Math]::Max(0, $Y + $dy)) * 65535) / ($sh - 1))
    [W]::mouse_event(0x8001, $ax, $ay, 0, 0); Start-Sleep -Milliseconds 60
  }
}
if ($Click) { Start-Sleep -Milliseconds 150; [W]::mouse_event(2, 0, 0, 0, 0); [W]::mouse_event(4, 0, 0, 0, 0) }
Start-Sleep -Milliseconds $Wait
if ($Width -eq 0) { $Width = [W]::GetSystemMetrics(0); $Height = [W]::GetSystemMetrics(1) }
$bmp = New-Object System.Drawing.Bitmap $Width, $Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($Left, $Top, 0, 0, $bmp.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"saved $Out"
