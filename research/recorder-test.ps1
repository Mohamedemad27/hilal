# A "screen recorder" (a copy of ping named obs64.exe) starts and stops: Hilal should show
# up in an ordinary screenshot only while it runs. Compares the top of the screen behind
# a plain dark window, so a black notch is easy to spot.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class R1 { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y); [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i); }'
[R1]::SetProcessDPIAware() | Out-Null
$cx = [R1]::GetSystemMetrics(0) / 2
$dir = 'K:\hilal\research\recorder'; New-Item -ItemType Directory -Force $dir | Out-Null
function Pump($ms) { $sw = [Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt $ms) { [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 15 } }
# A light window behind the notch, so the notch shows as a dark shape in captures.
$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = "None"; $form.StartPosition = "Manual"; $form.TopMost = $true; $form.BackColor = [System.Drawing.Color]::FromArgb(230, 230, 230)
$form.Bounds = New-Object System.Drawing.Rectangle ([int]($cx - 200)), 0, 400, 80; $form.Show(); Pump 300
function Notch($name) {
  $b = New-Object System.Drawing.Bitmap 400, 60; $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen([int]($cx - 200), 0, 0, 0, $b.Size); $b.Save("$dir\$name.png")
  $p = $b.GetPixel(200, 12); $g.Dispose(); $b.Dispose()
  "{0,-26} pixel in the notch's spot: ({1},{2},{3}) -> {4}" -f $name, $p.R, $p.G, $p.B, $(if ($p.R -lt 100) { 'notch VISIBLE' } else { 'notch hidden' })
}
Get-Process hilal, obs64 -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
[R1]::SetCursorPos(1700, 900) | Out-Null
Start-Process $Exe; Pump 5000
Notch 'before (glass on)'
Copy-Item C:\Windows\System32\PING.EXE K:\tmp\obs64.exe -Force
$rec = Start-Process K:\tmp\obs64.exe -ArgumentList '-n', '30', '127.0.0.1' -WindowStyle Hidden -PassThru
Pump 4500
Notch 'recorder running'
Stop-Process -Id $rec.Id -Confirm:$false; Pump 4500
Notch 'recorder closed'
$form.Close()
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false
Remove-Item K:\tmp\obs64.exe -ErrorAction SilentlyContinue
