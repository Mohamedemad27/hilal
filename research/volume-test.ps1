# The music page's volume slider and the wheel on it. Changes the real volume for a few
# seconds: the level and mute are read first and put back at the end. Pictures and the
# levels read after each step.
param([string]$Exe = 'K:\hilal\target\release\hilal.exe')
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class QV {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
}
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class VEnum {}
[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IVEnum { int EnumAudioEndpoints(); [PreserveSig] int GetDefaultAudioEndpoint(int flow, int role, out IVDev dev); }
[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IVDev { [PreserveSig] int Activate(ref Guid iid, int ctx, IntPtr p, [MarshalAs(UnmanagedType.IUnknown)] out object o); }
[Guid("5CDF2C82-841E-4546-9722-0CF74078229A"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IVVol {
  int RegisterControlChangeNotify(); int UnregisterControlChangeNotify(); int GetChannelCount();
  int SetMasterVolumeLevel();
  [PreserveSig] int SetMasterVolumeLevelScalar(float l, IntPtr ctx);
  int GetMasterVolumeLevel();
  [PreserveSig] int GetMasterVolumeLevelScalar(out float l);
  int SetChannelVolumeLevel(); int SetChannelVolumeLevelScalar(); int GetChannelVolumeLevel(); int GetChannelVolumeLevelScalar();
  [PreserveSig] int SetMute(int m, IntPtr ctx);
  [PreserveSig] int GetMute(out int m);
}
public static class Speakers {
  static IVVol V() {
    var e = (IVEnum)(new VEnum()); IVDev d; e.GetDefaultAudioEndpoint(0, 0, out d);
    var iid = typeof(IVVol).GUID; object o; d.Activate(ref iid, 23, IntPtr.Zero, out o); return (IVVol)o;
  }
  public static float Level() { float l; V().GetMasterVolumeLevelScalar(out l); return l; }
  public static bool Muted() { int m; V().GetMute(out m); return m != 0; }
  public static void Put(float l, bool m) { var v = V(); v.SetMasterVolumeLevelScalar(l, IntPtr.Zero); v.SetMute(m ? 1 : 0, IntPtr.Zero); }
}
'@
$level0 = [Speakers]::Level(); $muted0 = [Speakers]::Muted()
"volume before: {0:P0}, muted {1}" -f $level0, $muted0
$cx = [QV]::GetSystemMetrics(0) / 2
$out = 'K:\hilal\research\volume'; New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item
Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
[QV]::SetCursorPos(1100, 700) | Out-Null
try {
  $env:HILAL_DUMP = $out; $env:HILAL_FPS = "$out\log.txt"; Remove-Item "$out\log.txt" -ErrorAction SilentlyContinue
  Start-Process $Exe; Remove-Item Env:HILAL_DUMP, Env:HILAL_FPS
  Start-Sleep 9
  [QV]::SetCursorPos($cx, 16) | Out-Null; Start-Sleep 1; [QV]::SetCursorPos($cx, 70) | Out-Null; Start-Sleep 2
  # The slider runs from page x 318 to 378 (the page's left edge at cx - 210), at y 117.
  [QV]::SetCursorPos($cx + 108 + 24, 117) | Out-Null; Start-Sleep -Milliseconds 600
  [QV]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 60; [QV]::mouse_event(4, 0, 0, 0, 0); Start-Sleep 1
  "after the click at 40%: {0:P0}" -f [Speakers]::Level()
  foreach ($i in 1..3) { [QV]::mouse_event(0x800, 0, 0, 120, 0); Start-Sleep -Milliseconds 250 }
  Start-Sleep 1
  "after 3 notches up: {0:P0}" -f [Speakers]::Level()
  [QV]::SetCursorPos(1100, 700) | Out-Null; Start-Sleep 2
} finally {
  Get-Process hilal -ErrorAction SilentlyContinue | Stop-Process -Confirm:$false; Start-Sleep 1
  [Speakers]::Put($level0, $muted0)
  "volume put back: {0:P0}, muted {1}" -f [Speakers]::Level(), [Speakers]::Muted()
  Start-Process "$env:LOCALAPPDATA\Programs\Hilal\hilal.exe"
}
"pictures: " + ((Get-ChildItem $out -Filter *.png | Where-Object Name -notmatch 'mid-').Name -join ', ')
