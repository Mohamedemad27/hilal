# Makes Nusa's water reminder due right away, starts recording the speakers, launches
# Nusa, and films its reminder icon fast. Nusa's data is backed up first (restore after).
$n = Join-Path $env:APPDATA 'Nusa'
$out = 'K:\hilal\research\nusa-anim'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Copy-Item "$n\water.txt" "$out\water.txt.orig" -Force
$now = (Get-Date).Hour * 60 + (Get-Date).Minute
$lines = Get-Content "$n\water.txt" | ForEach-Object {
  switch -regex ($_) {
    '^bottle=' { 'bottle=250' } '^goal=' { 'goal=3000' } '^start=' { "start=$($now - 7)" } '^end=' { "end=$($now + 53)" }
    '^day=' { 'day=' + (Get-Date -Format 'yyyyMMdd') } '^taken=' { 'taken=0' } '^inb=' { 'inb=0' } '^fired=' { 'fired=0' } '^on=' { 'on=1' }
    default { $_ } } }
[IO.File]::WriteAllLines("$n\water.txt", $lines)
$rec = Start-Job -ScriptBlock { & K:\hilal\research\loopback.ps1 -Seconds 9 -Out K:\hilal\research\nusa-anim\sound.wav }
Start-Sleep -Milliseconds 1500
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Diagnostics; using System.Runtime.InteropServices;
public static class Film {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public static int Run(string dir, int ms) {
    SetProcessDPIAware(); var b = new Bitmap(320, 64); var g = Graphics.FromImage(b); var sw = Stopwatch.StartNew(); int n = 0;
    while (sw.ElapsedMilliseconds < ms) { g.CopyFromScreen(800, 0, 0, 0, b.Size); b.Save(String.Format("{0}\\f{1:00000}.png", dir, sw.ElapsedMilliseconds)); n++; }
    return n;
  }
}
'@
$t0 = Get-Date
Start-Process "$env:LOCALAPPDATA\Programs\Nusa\nusa.exe"
"frames: " + [Film]::Run($out, 6000)
Receive-Job $rec -Wait; Remove-Job $rec
"nusa started at " + $t0.ToString('HH:mm:ss.fff')
Get-Content "$n\water.txt" | Select-String 'fired|inb|taken'
