# Parks the cursor at given heights under the notch for a while each, logging the notch's
# size over time and saving a snapshot of each new state.
param([string]$Out = 'K:\hilal\research\dwell', [int[]]$Ys = @(40, 25, 15, 8, 3), [int]$X = 960, [int]$HoldMs = 2000)
New-Item -ItemType Directory -Force $Out | Out-Null
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Runtime.InteropServices; using System.Diagnostics; using System.Text; using System.Threading;
public static class DW {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  public static void Move(int x, int y) { int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1); mouse_event(0x8001, x * 65535 / (sw - 1), y * 65535 / (sh - 1), 0, 0); }
  static bool Dark(Color c) { return c.R + c.G + c.B < 14; }
  static int[] Measure(Bitmap b) {
    int cx = b.Width / 2, l = cx, r = cx, h = 0;
    if (!Dark(b.GetPixel(cx, 2))) return new int[] { 0, 0 };
    while (l > 0 && Dark(b.GetPixel(l - 1, 2))) l--;
    while (r < b.Width - 1 && Dark(b.GetPixel(r + 1, 2))) r++;
    int col = l + Math.Min(12, (r - l) / 4);
    for (int y = 0; y < b.Height; y++) if (Dark(b.GetPixel(col, y))) h = y + 1;
    return new int[] { r - l + 1, h };
  }
  public static string Hold(string outDir, string tag, int x, int y, int holdMs) {
    SetProcessDPIAware();
    var sb = new StringBuilder();
    var bmp = new Bitmap(700, 220); var g = Graphics.FromImage(bmp);
    Move(x, y);
    var sw = Stopwatch.StartNew(); int lastW = -1, lastH = -1, shot = 0;
    while (sw.ElapsedMilliseconds < holdMs) {
      g.CopyFromScreen(x - 350, 0, 0, 0, bmp.Size);
      var m = Measure(bmp);
      if (Math.Abs(m[0] - lastW) > 6 || Math.Abs(m[1] - lastH) > 3) {
        sb.AppendFormat("  {0,5} ms  notch {1}x{2}\n", sw.ElapsedMilliseconds, m[0], m[1]);
        bmp.Save(String.Format("{0}\\{1}-{2:00}-{3}x{4}.png", outDir, tag, shot++, m[0], m[1]));
        lastW = m[0]; lastH = m[1];
      }
    }
    return sb.ToString();
  }
}
'@
[DW]::SetProcessDPIAware() | Out-Null
foreach ($y in $Ys) {
  [DW]::Move($X, 500); Start-Sleep -Milliseconds 1500   # far away, let it close
  "cursor parked at y=$y :"
  [DW]::Hold($Out, "y$y", $X, $y, $HoldMs)
}
[DW]::Move($X, 500)
