# Slides the cursor along the top edge from the right toward the center, then dwells, logging
# the notch's size and saving snapshots when it changes.
param([string]$Out = 'K:\hilal\research\approach-side', [int]$Y = 0, [int]$FromX = 1400, [int]$StepPx = 10, [int]$StepMs = 120, [int]$DwellMs = 2000)
New-Item -ItemType Directory -Force $Out | Out-Null
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Runtime.InteropServices; using System.Diagnostics; using System.Text; using System.Threading;
public static class AS {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  static void Move(int x, int y) { int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1); mouse_event(0x8001, x * 65535 / (sw - 1), y * 65535 / (sh - 1), 0, 0); }
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
  public static string Run(string outDir, int y, int fromX, int stepPx, int stepMs, int dwellMs) {
    SetProcessDPIAware();
    var sb = new StringBuilder("ms,cursorX,width,height\n");
    var bmp = new Bitmap(700, 220); var g = Graphics.FromImage(bmp);
    Move(fromX, 420); Thread.Sleep(1200); Move(fromX, y); Thread.Sleep(600);
    var sw = Stopwatch.StartNew(); int lastW = -1, lastH = -1, shot = 0;
    Action<int> sample = (cx) => {
      g.CopyFromScreen(960 - 350, 0, 0, 0, bmp.Size);
      var m = Measure(bmp);
      sb.AppendFormat("{0},{1},{2},{3}\n", sw.ElapsedMilliseconds, cx, m[0], m[1]);
      if (Math.Abs(m[0] - lastW) > 6 || Math.Abs(m[1] - lastH) > 3) {
        bmp.Save(String.Format("{0}\\{1:00}-x{2}-{3}x{4}.png", outDir, shot++, cx, m[0], m[1]));
        lastW = m[0]; lastH = m[1];
      }
    };
    for (int cx = fromX; cx >= 960; cx -= stepPx) { Move(cx, y); var t0 = sw.ElapsedMilliseconds; while (sw.ElapsedMilliseconds - t0 < stepMs) sample(cx); }
    var d0 = sw.ElapsedMilliseconds; while (sw.ElapsedMilliseconds - d0 < dwellMs) sample(960);
    Move(960, 420); var a0 = sw.ElapsedMilliseconds; while (sw.ElapsedMilliseconds - a0 < 1200) sample(-1);
    return sb.ToString();
  }
}
'@
$csv = [AS]::Run($Out, $Y, $FromX, $StepPx, $StepMs, $DwellMs)
Set-Content "$Out\samples.csv" $csv -Encoding ascii
$rows = $csv.Trim().Split("`n") | Select-Object -Skip 1 | ForEach-Object { $a = $_.Split(','); [pscustomobject]@{ ms = [int]$a[0]; x = [int]$a[1]; w = [int]$a[2]; h = [int]$a[3] } }
$prev = $null
foreach ($r in $rows) { if (-not $prev -or [math]::Abs($r.w - $prev.w) -gt 6 -or [math]::Abs($r.h - $prev.h) -gt 3) { "{0,6} ms  cursor x={1,4}  notch {2}x{3}" -f $r.ms, $r.x, $r.w, $r.h; $prev = $r } }
