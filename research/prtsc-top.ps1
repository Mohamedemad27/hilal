# PrintScreen (which Hilal makes itself capturable for) and save the notch area.
param([string]$Name)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class KP { [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, int flags, int extra); [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'
[KP]::SetProcessDPIAware() | Out-Null
[System.Windows.Forms.Clipboard]::Clear()
[KP]::keybd_event(0x2C, 0, 1, 0); Start-Sleep -Milliseconds 60; [KP]::keybd_event(0x2C, 0, 3, 0)
Start-Sleep -Milliseconds 900
$img = [System.Windows.Forms.Clipboard]::GetImage()
if (-not $img) { "no image"; return }
$crop = New-Object System.Drawing.Bitmap 560, 200
[System.Drawing.Graphics]::FromImage($crop).DrawImage($img, (New-Object System.Drawing.Rectangle 0, 0, 560, 200), (New-Object System.Drawing.Rectangle 680, 0, 560, 200), 'Pixel')
$crop.Save("K:\hilal\research\hilal-water\$Name.png"); "saved $Name"
