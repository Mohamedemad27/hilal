# Watches the notch area and the notification corner until a given time, saving a frame
# (with a timestamp) whenever either changes noticeably. Leaves the cursor parked away.
param([string]$Until = '17:09:30', [string]$Out = 'K:\hilal\research\nusa-water\watch')
New-Item -ItemType Directory -Force $Out | Out-Null
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Runtime.InteropServices;
public static class WT {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  // A cheap signature: average brightness of an 8x4 grid of cells.
  public static int[] Sig(Bitmap b) {
    var s = new int[32]; int cw = b.Width / 8, ch = b.Height / 4;
    for (int i = 0; i < 32; i++) { int x0 = (i % 8) * cw, y0 = (i / 8) * ch; long t = 0; int n = 0;
      for (int y = y0; y < y0 + ch; y += 4) for (int x = x0; x < x0 + cw; x += 4) { var c = b.GetPixel(x, y); t += c.R + c.G + c.B; n++; }
      s[i] = (int)(t / Math.Max(1, n)); }
    return s;
  }
  public static int Diff(int[] a, int[] b) { if (a == null) return 9999; int d = 0; for (int i = 0; i < a.Length; i++) d = Math.Max(d, Math.Abs(a[i] - b[i])); return d; }
}
'@
[WT]::SetProcessDPIAware() | Out-Null
$end = [datetime]::ParseExact($Until, 'HH:mm:ss', $null)
$top = New-Object System.Drawing.Bitmap 560, 200; $gt = [System.Drawing.Graphics]::FromImage($top)
$corner = New-Object System.Drawing.Bitmap 480, 420; $gc = [System.Drawing.Graphics]::FromImage($corner)
$st = $null; $sc = $null; $n = 0
while ((Get-Date) -lt $end) {
  $gt.CopyFromScreen(680, 0, 0, 0, $top.Size); $gc.CopyFromScreen(1440, 620, 0, 0, $corner.Size)
  $a = [WT]::Sig($top); $b = [WT]::Sig($corner); $t = Get-Date -Format 'HHmmss_fff'
  if ([WT]::Diff($st, $a) -gt 25) { $top.Save("$Out\top-$t.png"); $st = $a; $n++; "top changed $t" }
  if ([WT]::Diff($sc, $b) -gt 25) { $corner.Save("$Out\corner-$t.png"); $sc = $b; $n++; "corner changed $t" }
  Start-Sleep -Milliseconds 150
}
"done, $n frames"
