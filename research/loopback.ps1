# Records what the speakers play (WASAPI loopback; no microphone) for a few seconds into a
# 32-bit float WAV. Usage: loopback.ps1 -Seconds 8 -Out file.wav
param([double]$Seconds = 8, [string]$Out = 'K:\hilal\research\loopback.wav')
Add-Type -TypeDefinition @'
using System; using System.IO; using System.Runtime.InteropServices; using System.Threading;
[ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator { int EnumAudioEndpoints(int a, int b, out IntPtr c); int GetDefaultAudioEndpoint(int flow, int role, out IMMDevice dev); }
[ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice { int Activate(ref Guid iid, int ctx, IntPtr p, [MarshalAs(UnmanagedType.IUnknown)] out object o); }
[ComImport, Guid("1CB9AD4C-DBFA-4c32-B178-C2F568A703B2"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioClient {
  int Initialize(int share, int flags, long dur, long period, IntPtr fmt, IntPtr session);
  int GetBufferSize(out uint n); int GetStreamLatency(out long l); int GetCurrentPadding(out uint p);
  int IsFormatSupported(int s, IntPtr f, out IntPtr c); int GetMixFormat(out IntPtr f);
  int GetDevicePeriod(out long a, out long b); int Start(); int Stop(); int Reset(); int SetEventHandle(IntPtr h);
  int GetService(ref Guid iid, [MarshalAs(UnmanagedType.IUnknown)] out object o);
}
[ComImport, Guid("C8ADBD64-E71E-48a0-A4DE-185C395CD317"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IAudioCaptureClient {
  int GetBuffer(out IntPtr data, out uint frames, out uint flags, out ulong pos, out ulong qpc);
  int ReleaseBuffer(uint frames); int GetNextPacketSize(out uint n);
}
public static class Loop {
  public static string Record(double seconds, string path) {
    var en = (IMMDeviceEnumerator)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")));
    IMMDevice dev; Marshal.ThrowExceptionForHR(en.GetDefaultAudioEndpoint(0, 0, out dev));
    var iidClient = new Guid("1CB9AD4C-DBFA-4c32-B178-C2F568A703B2"); object o;
    Marshal.ThrowExceptionForHR(dev.Activate(ref iidClient, 23, IntPtr.Zero, out o));
    var client = (IAudioClient)o; IntPtr fmt; Marshal.ThrowExceptionForHR(client.GetMixFormat(out fmt));
    int channels = Marshal.ReadInt16(fmt, 2), rate = Marshal.ReadInt32(fmt, 4), bits = Marshal.ReadInt16(fmt, 14);
    Marshal.ThrowExceptionForHR(client.Initialize(0, 0x20000, 10000000, 0, fmt, IntPtr.Zero));
    var iidCap = new Guid("C8ADBD64-E71E-48a0-A4DE-185C395CD317");
    Marshal.ThrowExceptionForHR(client.GetService(ref iidCap, out o)); var cap = (IAudioCaptureClient)o;
    var ms = new MemoryStream(); int frameBytes = channels * bits / 8;
    // Keep real time: fill gaps (when nothing plays, no packets arrive) with silence.
    var sw = System.Diagnostics.Stopwatch.StartNew(); long written = 0;
    client.Start();
    while (sw.Elapsed.TotalSeconds < seconds) {
      uint n; cap.GetNextPacketSize(out n);
      if (n == 0) {
        long due = (long)(sw.Elapsed.TotalSeconds * rate) - 4800;
        if (due > written) { ms.Write(new byte[(due - written) * frameBytes], 0, (int)((due - written) * frameBytes)); written = due; }
        Thread.Sleep(5); continue;
      }
      IntPtr data; uint frames, flags; ulong p1, p2; cap.GetBuffer(out data, out frames, out flags, out p1, out p2);
      var buf = new byte[frames * frameBytes];
      if ((flags & 2) == 0) Marshal.Copy(data, buf, 0, buf.Length);
      ms.Write(buf, 0, buf.Length); written += frames; cap.ReleaseBuffer(frames);
    }
    client.Stop();
    var pcm = ms.ToArray();
    using (var f = new BinaryWriter(File.Create(path))) {
      f.Write(new char[] {'R','I','F','F'}); f.Write(36 + pcm.Length); f.Write(new char[] {'W','A','V','E','f','m','t',' '});
      f.Write(16); f.Write((short)(bits == 32 ? 3 : 1)); f.Write((short)channels); f.Write(rate); f.Write(rate * frameBytes);
      f.Write((short)frameBytes); f.Write((short)bits); f.Write(new char[] {'d','a','t','a'}); f.Write(pcm.Length); f.Write(pcm);
    }
    return String.Format("{0} Hz, {1} ch, {2}-bit, {3:F2} s", rate, channels, bits, (double)pcm.Length / (rate * frameBytes));
  }
}
'@
[Loop]::Record($Seconds, $Out)
