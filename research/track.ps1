# Hovers the notch and records, as fast as the screen can be read, the notch's on-screen
# height and width over time. Frozen frames show up as repeats; hitches as jumps.
param([int]$Ms = 900, [string]$Out = 'K:\hilal\research\track.csv', [switch]$Close)
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Runtime.InteropServices; using System.Diagnostics; using System.Text;
public static class T {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  static void Move(int x, int y) { int sw = GetSystemMetrics(0), sh = GetSystemMetrics(1); mouse_event(0x8001, x * 65535 / (sw - 1), y * 65535 / (sh - 1), 0, 0); }
  static bool Dark(Color c) { return c.R + c.G + c.B < 40; }
  public static string Run(int ms, bool close) {
    SetProcessDPIAware();
    var sb = new StringBuilder("ms,height,width\n");
    var bmp = new Bitmap(640, 190); var g = Graphics.FromImage(bmp);
    Move(960, close ? 3 : 700); System.Threading.Thread.Sleep(close ? 1500 : 800);
    var sw = Stopwatch.StartNew();
    Move(960, close ? 700 : 3);
    while (sw.ElapsedMilliseconds < ms) {
      double t = sw.Elapsed.TotalMilliseconds;
      g.CopyFromScreen(640, 0, 0, 0, bmp.Size);
      int h = 0; for (int y = 0; y < 190; y++) if (Dark(bmp.GetPixel(320, y))) h = y + 1; else if (y > 4 && h > 0 && y > h + 8) break;
      int l = 320; while (l > 0 && Dark(bmp.GetPixel(l - 1, 4))) l--;
      int r = 320; while (r < 639 && Dark(bmp.GetPixel(r + 1, 4))) r++;
      sb.AppendFormat("{0:F1},{1},{2}\n", t, h, r - l + 1);
    }
    Move(960, 700);
    return sb.ToString();
  }
}
'@
$csv = [T]::Run($Ms, $Close.IsPresent)
Set-Content -Path $Out -Value $csv -Encoding ascii
$rows = $csv.Trim().Split("`n") | Select-Object -Skip 1 | ForEach-Object { $a = $_.Split(','); [pscustomobject]@{ t = [double]$a[0]; h = [int]$a[1]; w = [int]$a[2] } }
$gaps = for ($i = 1; $i -lt $rows.Count; $i++) { $rows[$i].t - $rows[$i - 1].t }
"samples: $($rows.Count), avg sample gap: {0:F1} ms, max: {1:F1} ms" -f ($gaps | Measure-Object -Average).Average, ($gaps | Measure-Object -Maximum).Maximum
$rows | Format-Table -AutoSize | Out-String -Width 120
