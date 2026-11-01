# Same capture as for Nusa, for Hilal: a reminder due right away (glass off so screen
# capture sees it), the speakers recorded, the icon filmed. Restores the state after.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
$st = Join-Path $env:APPDATA 'Hilal\state.json'
$out = 'K:\hilal\research\hilal-anim'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item $st "$st.before-anim-test" -Force
$j = Get-Content $st -Raw | ConvertFrom-Json
$now = (Get-Date).Hour * 60 + (Get-Date).Minute
$j.settings.glass = $false
foreach ($kv in @{ from_min = $now - 7; to_min = $now + 53; goal_ml = 3000; bottle_ml = 250; in_bottle_ml = 0; handled_slot = -1; snooze_until = 0; enabled = $true; drunk_ml = 0; day = (Get-Date -Format 'yyyy-MM-dd') }.GetEnumerator()) {
  $j.water | Add-Member -Force $kv.Key $kv.Value }
[IO.File]::WriteAllText($st, ($j | ConvertTo-Json -Depth 10), (New-Object System.Text.UTF8Encoding $false))
$rec = Start-Job -ScriptBlock { & K:\hilal\research\loopback.ps1 -Seconds 8 -Out K:\hilal\research\hilal-anim\sound.wav }
Start-Sleep -Milliseconds 1500
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.Diagnostics; using System.Runtime.InteropServices;
public static class Film3 {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public static int Run(string dir, int ms) {
    SetProcessDPIAware(); var b = new Bitmap(320, 64); var g = Graphics.FromImage(b); var sw = Stopwatch.StartNew(); int n = 0;
    while (sw.ElapsedMilliseconds < ms) { g.CopyFromScreen(800, 0, 0, 0, b.Size); if (n % 2 == 0) b.Save(String.Format("{0}\\f{1:00000}.png", dir, sw.ElapsedMilliseconds)); n++; }
    return n;
  }
}
'@
Start-Process $Exe
"frames: " + [Film3]::Run($out, 6000)
Receive-Job $rec -Wait; Remove-Job $rec
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
Copy-Item "$st.before-anim-test" $st -Force; Remove-Item "$st.before-anim-test"
"state restored; glass=" + (Get-Content $st -Raw | ConvertFrom-Json).settings.glass
