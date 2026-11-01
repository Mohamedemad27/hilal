# Moves the cursor slowly up toward the top-center, then dwells, recording the notch's size
# (black pixels around the top center) at every step, with timestamps. Saves snapshots
# whenever the size changes noticeably.
param([string]$Out = 'K:\hilal\research\approach', [int]$StepPx = 10, [int]$StepMs = 150, [int]$DwellMs = 2500, [int]$X = 960)
New-Item -ItemType Directory -Force $Out | Out-Null
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Runtime.InteropServices; using System.Diagnostics; using System.Text; using System.Threading;
public static class AP {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  static void Move(int x, int y) { int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1); mouse_event(0x8001, x * 65535 / (sw - 1), y * 65535 / (sh - 1), 0, 0); }
  static bool Dark(Color c) { return c.R + c.G + c.B < 14; }
  static int[] Measure(Bitmap b) {
    // width at y=2 and height down the middle column, counting black from the top center
    int cx = b.Width / 2, l = cx, r = cx, h = 0;
    if (!Dark(b.GetPixel(cx, 2))) return new int[] { 0, 0 };
    while (l > 0 && Dark(b.GetPixel(l - 1, 2))) l--;
    while (r < b.Width - 1 && Dark(b.GetPixel(r + 1, 2))) r++;
    // height: lowest black pixel in a column a bit off center (avoids text)
    int col = Math.Max(l + 6, cx - (r - l) / 2 + 8);
    for (int y = 0; y < b.Height; y++) if (Dark(b.GetPixel(col, y))) h = y + 1;
    return new int[] { r - l + 1, h };
  }
  public static string Run(string outDir, int x, int stepPx, int stepMs, int dwellMs) {
    SetProcessDPIAware();
    var sb = new StringBuilder("ms,cursorY,width,height\n");
    var bmp = new Bitmap(700, 220); var g = Graphics.FromImage(bmp);
    Move(x, 420); Thread.Sleep(1500);
    var sw = Stopwatch.StartNew(); int lastW = -1, lastH = -1, shot = 0;
    Action<int> sample = (cy) => {
      g.CopyFromScreen(x - 350, 0, 0, 0, bmp.Size);
      var m = Measure(bmp);
      sb.AppendFormat("{0},{1},{2},{3}\n", sw.ElapsedMilliseconds, cy, m[0], m[1]);
      if (Math.Abs(m[0] - lastW) > 6 || Math.Abs(m[1] - lastH) > 4) {
        bmp.Save(String.Format("{0}\\{1:00}-y{2}-{3}x{4}.png", outDir, shot++, cy, m[0], m[1]));
        lastW = m[0]; lastH = m[1];
      }
    };
    for (int cy = 400; cy >= 0; cy -= stepPx) { Move(x, cy); var t0 = sw.ElapsedMilliseconds; while (sw.ElapsedMilliseconds - t0 < stepMs) { sample(cy); } }
    var d0 = sw.ElapsedMilliseconds; while (sw.ElapsedMilliseconds - d0 < dwellMs) sample(0);
    // and away again
    Move(x, 420); var a0 = sw.ElapsedMilliseconds; while (sw.ElapsedMilliseconds - a0 < 1500) sample(420);
    return sb.ToString();
  }
}
'@
$csv = [AP]::Run($Out, $X, $StepPx, $StepMs, $DwellMs)
Set-Content "$Out\samples.csv" $csv -Encoding ascii
# Summarize: one line per distinct state, with when it began.
$rows = $csv.Trim().Split("`n") | Select-Object -Skip 1 | ForEach-Object { $a = $_.Split(','); [pscustomobject]@{ ms = [int]$a[0]; y = [int]$a[1]; w = [int]$a[2]; h = [int]$a[3] } }
$prev = $null
foreach ($r in $rows) { if (-not $prev -or [math]::Abs($r.w - $prev.w) -gt 6 -or [math]::Abs($r.h - $prev.h) -gt 4) { "{0,6} ms  cursor y={1,3}  notch {2}x{3}" -f $r.ms, $r.y, $r.w, $r.h; $prev = $r } }
