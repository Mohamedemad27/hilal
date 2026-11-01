//! Live levels of whatever the PC is playing, for the closed notch's music bars: the
//! default output is captured in loopback (WASAPI), and every ~20 ms a 1024-point FFT is
//! split into a few bands from bass to treble. Each band is normalized against its own
//! recent peak, so the bars move with the song at any volume.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

pub const BANDS: usize = 5;
const N: usize = 1024;
/// Band edges in Hz: bass, low mids, mids, presence, air.
const EDGES: [f32; BANDS + 1] = [40.0, 160.0, 500.0, 1500.0, 4000.0, 12000.0];

pub struct Meter {
    levels: Arc<[AtomicU32; BANDS]>,
    active: Arc<AtomicBool>,
    /// Last time a sample arrived (silence when no one plays sound).
    alive: Arc<AtomicU32>,
    started: Instant,
}

impl Meter {
    pub fn new() -> Self {
        let levels: Arc<[AtomicU32; BANDS]> = Arc::new(Default::default());
        let active = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicU32::new(0));
        let started = Instant::now();
        let (l, a, al) = (levels.clone(), active.clone(), alive.clone());
        std::thread::spawn(move || run(l, a, al, started));
        Self { levels, active, alive, started }
    }

    /// Only listens while music plays.
    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
    }

    /// Each band 0..1, or None if no sound has come through lately.
    pub fn levels(&self) -> Option<[f32; BANDS]> {
        let ms = self.started.elapsed().as_millis() as u32;
        if ms.saturating_sub(self.alive.load(Ordering::Relaxed)) > 400 {
            return None;
        }
        Some(std::array::from_fn(|i| f32::from_bits(self.levels[i].load(Ordering::Relaxed))))
    }
}

fn run(levels: Arc<[AtomicU32; BANDS]>, active: Arc<AtomicBool>, alive: Arc<AtomicU32>, started: Instant) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    loop {
        if !active.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        // Capture until stopped or the device goes away (then start over on the new one).
        let _ = unsafe { capture(&levels, &active, &alive, started) };
        std::thread::sleep(Duration::from_millis(500));
    }
}

unsafe fn capture(levels: &[AtomicU32; BANDS], active: &AtomicBool, alive: &AtomicU32, started: Instant) -> windows::core::Result<()> {
    let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
    let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
    let fmt = client.GetMixFormat()?;
    let wf: WAVEFORMATEX = *fmt;
    // Shared-mode mix formats are 32-bit float; anything else we skip.
    let is_float = wf.wBitsPerSample == 32
        && (wf.wFormatTag == 3 || (wf.wFormatTag == 0xFFFE && {
            let ext = *(fmt as *const WAVEFORMATEXTENSIBLE);
            ext.SubFormat.data1 == 3
        }));
    let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 200_000, 0, fmt, None);
    CoTaskMemFree(Some(fmt as *const _));
    init?;
    if !is_float {
        return Ok(());
    }
    let rate = wf.nSamplesPerSec as f32;
    let channels = wf.nChannels.max(1) as usize;
    let cap: IAudioCaptureClient = client.GetService()?;
    client.Start()?;

    let window: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()).collect();
    let bins: Vec<(usize, usize)> = (0..BANDS)
        .map(|b| {
            let lo = ((EDGES[b] / rate * N as f32) as usize).max(1);
            let hi = ((EDGES[b + 1] / rate * N as f32) as usize).clamp(lo + 1, N / 2);
            (lo, hi)
        })
        .collect();
    let mut ring = vec![0.0f32; N];
    let mut pos = 0usize;
    let mut fresh = 0usize;
    // Per band, in dB: the recent loudest and the running average.
    let mut peak = [-40.0f32; BANDS];
    let mut avg = [-60.0f32; BANDS];
    let mut re = vec![0.0f32; N];
    let mut im = vec![0.0f32; N];

    while active.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
        loop {
            let n = cap.GetNextPacketSize()?;
            if n == 0 {
                break;
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
            let samples = std::slice::from_raw_parts(data as *const f32, frames as usize * channels);
            for f in 0..frames as usize {
                let v = if silent { 0.0 } else { samples[f * channels..(f + 1) * channels].iter().sum::<f32>() / channels as f32 };
                ring[pos] = v;
                pos = (pos + 1) % N;
            }
            fresh += frames as usize;
            cap.ReleaseBuffer(frames)?;
            alive.store(started.elapsed().as_millis() as u32, Ordering::Relaxed);
        }
        // About every 20 ms of new sound, look at the latest N samples.
        if fresh < (rate * 0.02) as usize {
            continue;
        }
        fresh = 0;
        for i in 0..N {
            re[i] = ring[(pos + i) % N] * window[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        for (b, &(lo, hi)) in bins.iter().enumerate() {
            let e = (lo..hi).map(|k| re[k] * re[k] + im[k] * im[k]).sum::<f32>() / (hi - lo) as f32;
            let db = 10.0 * e.max(1e-12).log10();
            // How loud the band is overall: against its own recent loudest, falling slowly
            // (about 2 dB a second), with a floor so near-silence stays flat.
            let floor = -70.0;
            let p = (peak[b] - 0.04).max(db).max(floor + 25.0);
            peak[b] = p;
            let overall = ((db - (p - 36.0)) / 36.0).clamp(0.0, 1.0);
            // And how it moves: against its average over the last half second or so, which
            // makes beats jump out even in a song that's loud all the way through.
            avg[b] += (db - avg[b]) * 0.04;
            let lift = ((db - avg[b] + 6.0) / 14.0).clamp(0.0, 1.0);
            let v = (0.35 * overall + 0.65 * lift) * overall.min(1.0).sqrt();
            levels[b].store(v.to_bits(), Ordering::Relaxed);
        }
    }
    let _ = client.Stop();
    Ok(())
}

/// In-place radix-2 FFT.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f32).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fft_finds_a_tone_in_its_bin() {
        let n = 1024;
        let mut re: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * 64.0 * i as f32 / n as f32).sin()).collect();
        let mut im = vec![0.0; n];
        super::fft(&mut re, &mut im);
        let mag: Vec<f32> = (0..n / 2).map(|k| re[k].hypot(im[k])).collect();
        let top = mag.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(top, 64);
    }
}

#[cfg(test)]
mod live {
    /// Listens to whatever is playing for a few seconds and prints the levels (run with
    /// --ignored --nocapture while music plays).
    #[test]
    #[ignore]
    fn prints_live_levels() {
        let m = super::Meter::new();
        m.set_active(true);
        std::thread::sleep(std::time::Duration::from_millis(800));
        for _ in 0..12 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            match m.levels() {
                Some(l) => println!("{}", l.map(|v| format!("{v:.2}")).join(" ")),
                None => println!("no sound"),
            }
        }
    }
}
