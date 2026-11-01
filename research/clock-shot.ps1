# PrintScreen the notch closed and open; save 2x crops of the clock and the header.
param([string]$Tag = 'clock')
$s = 'K:\hilal\research\snap.ps1'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class KC { [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, int flags, int extra); }'
function Grab {
  [System.Windows.Forms.Clipboard]::Clear()
  [KC]::keybd_event(0x2C, 0, 1, 0); Start-Sleep -Milliseconds 60; [KC]::keybd_event(0x2C, 0, 3, 0)
  Start-Sleep -Milliseconds 900
  [System.Windows.Forms.Clipboard]::GetImage()
}
function Crop($img, $x, $y, $w, $h, $z, $name) {
  $out = New-Object System.Drawing.Bitmap ($w * $z), ($h * $z)
  $g = [System.Drawing.Graphics]::FromImage($out); $g.InterpolationMode = 'HighQualityBicubic'
  $g.DrawImage($img, (New-Object System.Drawing.Rectangle 0, 0, ($w * $z), ($h * $z)), (New-Object System.Drawing.Rectangle $x, $y, $w, $h), 'Pixel')
  $out.Save("K:\hilal\research\$Tag-$name.png")
}
& $s -X 960 -Y 700 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 1200 | Out-Null
$closed = Grab; Crop $closed 860 0 200 36 3 'closed'
& $s -X 960 -Y 3 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 1200 | Out-Null
$open = Grab; Crop $open 735 0 450 150 2 'open'
& $s -X 960 -Y 700 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 10 | Out-Null
"saved $Tag-*.png"
