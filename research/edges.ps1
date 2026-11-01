# PrintScreen with the notch closed and open, then save 6x zooms of its corners.
param([string]$Tag = 'before')
$s = 'K:\hilal\research\snap.ps1'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class KE { [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, int flags, int extra); }'
function Grab {
  [System.Windows.Forms.Clipboard]::Clear()
  [KE]::keybd_event(0x2C, 0, 1, 0); Start-Sleep -Milliseconds 60; [KE]::keybd_event(0x2C, 0, 3, 0)
  Start-Sleep -Milliseconds 900
  [System.Windows.Forms.Clipboard]::GetImage()
}
function Zoom($img, $x, $y, $w, $h, $name) {
  $z = 6; $out = New-Object System.Drawing.Bitmap ($w * $z), ($h * $z)
  $g = [System.Drawing.Graphics]::FromImage($out); $g.InterpolationMode = 'NearestNeighbor'; $g.PixelOffsetMode = 'Half'
  $g.DrawImage($img, (New-Object System.Drawing.Rectangle 0, 0, ($w * $z), ($h * $z)), (New-Object System.Drawing.Rectangle $x, $y, $w, $h), 'Pixel')
  $out.Save("K:\hilal\research\edge-$Tag-$name.png")
}
& $s -X 960 -Y 700 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 1200 | Out-Null
$closed = Grab
Zoom $closed 865 0 60 34 'closed-left'
& $s -X 960 -Y 3 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 1200 | Out-Null
$open = Grab
Zoom $open 725 0 70 34 'open-ear'
Zoom $open 735 100 60 45 'open-corner'
& $s -X 960 -Y 700 -Out "$env:TEMP\x.png" -Width 4 -Height 4 -Wait 10 | Out-Null
"saved edge-$Tag-*.png"
