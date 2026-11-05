//! Hilal's own alert sounds, synthesized once into in-memory WAVs and played with
//! PlaySound. The water chime: two water drops (quick rising "plip"s) and then a soft bell
//! (C6 with a faint octave and twelfth), about nine tenths of a second. The prayer chime:
//! three calm bells stepping down a major chord (G5, E5, C5), ringing out over two seconds,
//! unless an adhan recording has been chosen (kept as %APPDATA%\Hilal\adhan.<ext>), which
//! then plays instead.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use windows::core::{HSTRING, PCWSTR};
use windows::Foundation::Uri;
use windows::Media::Core::MediaSource;
use windows::Media::Playback::MediaPlayer;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

/// Audio types an adhan recording may be.
pub const ADHAN_TYPES: [&str; 6] = ["mp3", "m4a", "aac", "wav", "wma", "ogg"];

/// The adhan starts this quiet (of the device's volume), swells over `ADHAN_FADE_S`
/// seconds, and settles at `ADHAN_MAX`: present, never loud.
const ADHAN_START: f64 = 0.03;
const ADHAN_MAX: f64 = 0.5;
const ADHAN_FADE_S: f64 = 10.0;

thread_local! {
    /// The adhan while it plays (dropping the player would cut it off), and when it began.
    static PLAYER: RefCell<Option<(MediaPlayer, Instant)>> = const { RefCell::new(None) };
}

/// The adhan's volume `t` seconds in: an ease-in-out swell from quiet up to the cap.
fn adhan_volume(t: f64) -> f64 {
    let p = (t / ADHAN_FADE_S).clamp(0.0, 1.0);
    let ease = p * p * (3.0 - 2.0 * p);
    ADHAN_START + (ADHAN_MAX - ADHAN_START) * ease
}

/// Moves the adhan's volume along its swell; call it every frame while it may be playing.
pub fn tick() {
    PLAYER.with(|p| {
        if let Some((player, start)) = p.borrow().as_ref() {
            let t = start.elapsed().as_secs_f64();
            if t <= ADHAN_FADE_S + 0.5 {
                let _ = player.SetVolume(adhan_volume(t));
            }
        }
    });
}

const RATE: u32 = 44_100;
const LEN_S: f32 = 0.9;

/// A voice: start s, start Hz, end Hz, glide s, fade time constant s, peak level.
/// A drop glides up fast; a bell holds its pitch.
type Voice = (f32, f32, f32, f32, f32, f32);

const WATER: [Voice; 5] = [
    (0.00, 520.0, 1040.0, 0.045, 0.035, 0.42),  // first drop
    (0.12, 680.0, 1360.0, 0.045, 0.040, 0.38),  // second drop, a little higher
    (0.26, 1046.5, 1046.5, 0.0, 0.26, 0.34),    // bell
    (0.26, 2093.0, 2093.0, 0.0, 0.12, 0.08),    // its octave
    (0.26, 3139.5, 3139.5, 0.0, 0.07, 0.035),   // its twelfth
];

const PRAYER_LEN_S: f32 = 2.2;
const PRAYER: [Voice; 6] = [
    (0.00, 783.99, 783.99, 0.0, 0.55, 0.30),   // G5
    (0.00, 1567.98, 1567.98, 0.0, 0.20, 0.05), // its octave
    (0.38, 659.25, 659.25, 0.0, 0.60, 0.30),   // E5
    (0.38, 1318.51, 1318.51, 0.0, 0.22, 0.05),
    (0.76, 523.25, 523.25, 0.0, 0.75, 0.32),   // C5, left to ring
    (0.76, 1046.50, 1046.50, 0.0, 0.26, 0.06),
];

/// The microphone turned on: two quick soft tones stepping up (A5, E6). Muted: the same
/// stepping down. A quarter of a second, quieter than the chimes.
const MIC_LEN_S: f32 = 0.28;
const MIC_ON: [Voice; 2] = [
    (0.00, 880.0, 880.0, 0.0, 0.045, 0.26),
    (0.08, 1318.51, 1318.51, 0.0, 0.07, 0.26),
];
const MIC_OFF: [Voice; 2] = [
    (0.00, 1318.51, 1318.51, 0.0, 0.045, 0.26),
    (0.08, 880.0, 880.0, 0.0, 0.07, 0.26),
];

fn chime() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| synth(&WATER, LEN_S))
}

fn prayer_chime() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| synth(&PRAYER, PRAYER_LEN_S))
}

fn synth(voices: &[Voice], len_s: f32) -> Vec<u8> {
    {
        let len = (RATE as f32 * len_s) as usize;
        let dt = 1.0 / RATE as f32;
        let mut phase = vec![0.0f32; voices.len()];
        let mut pcm = Vec::with_capacity(len * 2);
        for i in 0..len {
            let t = i as f32 * dt;
            let mut v = 0.0f32;
            for (k, &(start, f0, f1, glide, tau, peak)) in voices.iter().enumerate() {
                let lt = t - start;
                if lt < 0.0 {
                    continue;
                }
                // Exponential glide from f0 to f1, then steady.
                let f = if glide > 0.0 && lt < glide { f0 * (f1 / f0).powf(lt / glide) } else { f1 };
                phase[k] = (phase[k] + std::f32::consts::TAU * f * dt) % std::f32::consts::TAU;
                let attack = (lt / 0.003).min(1.0);
                v += peak * attack * (-lt / tau).exp() * phase[k].sin();
            }
            // Fade the last 30 ms so it ends without a click.
            let tail = ((len - i) as f32 / (RATE as f32 * 0.03)).min(1.0);
            let s = ((v * tail).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        wav
    }
}

/// Plays the water chime without waiting for it.
pub fn play_chime() {
    play(chime(), "chime");
}

/// Plays the microphone's sound: muted (stepping down) or back on (stepping up).
pub fn play_mic(muted: bool) {
    static ON: OnceLock<Vec<u8>> = OnceLock::new();
    static OFF: OnceLock<Vec<u8>> = OnceLock::new();
    if muted {
        play(OFF.get_or_init(|| synth(&MIC_OFF, MIC_LEN_S)), "mic off");
    } else {
        play(ON.get_or_init(|| synth(&MIC_ON, MIC_LEN_S)), "mic on");
    }
}

/// Plays the chosen adhan, or else the prayer chime, without waiting for it.
pub fn play_prayer() {
    if let Some(path) = adhan() {
        match play_file(&path) {
            Ok(()) => return log("adhan", true),
            Err(_) => log("adhan", false),
        }
    }
    play(prayer_chime(), "prayer");
}

/// Stops the adhan if it's playing.
pub fn stop() {
    PLAYER.with(|p| {
        if let Some((player, _)) = p.borrow_mut().take() {
            let _ = player.Pause();
            let _ = player.Close();
        }
    });
}

fn dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("Hilal")
}

/// The chosen adhan recording, if there is one.
pub fn adhan() -> Option<PathBuf> {
    ADHAN_TYPES.iter().map(|e| dir().join(format!("adhan.{e}"))).find(|p| p.is_file())
}

/// Keeps a copy of `src` as the adhan (in place of any earlier one), or with `None` goes
/// back to the bells.
pub fn set_adhan(src: Option<&Path>) -> std::io::Result<()> {
    if let Some(old) = adhan() {
        std::fs::remove_file(old)?;
    }
    let Some(src) = src else { return Ok(()) };
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("mp3").to_ascii_lowercase();
    std::fs::create_dir_all(dir())?;
    std::fs::copy(src, dir().join(format!("adhan.{ext}"))).map(|_| ())
}

fn play_file(path: &Path) -> windows::core::Result<()> {
    stop();
    let uri = Uri::CreateUri(&HSTRING::from(format!("file:///{}", path.display().to_string().replace('\\', "/"))))?;
    let player = MediaPlayer::new()?;
    player.SetSource(&MediaSource::CreateFromUri(&uri)?)?;
    player.SetVolume(ADHAN_START)?;
    player.Play()?;
    PLAYER.with(|p| *p.borrow_mut() = Some((player, Instant::now())));
    Ok(())
}

fn play(wav: &[u8], what: &str) {
    let ok = unsafe { PlaySoundW(PCWSTR(wav.as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
    log(what, ok.as_bool());
}

/// With HILAL_FPS=<file> set, notes each sound played (for checking it plays).
fn log(what: &str, ok: bool) {
    if let Some(path) = std::env::var_os("HILAL_FPS") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "  [{what}] played ok={ok}");
        }
    }
}

#[cfg(test)]
mod tests {
    /// Plays it for real (run with --ignored, to listen or record).
    #[test]
    #[ignore]
    fn plays_the_chime() {
        let wav = super::chime();
        let ok = unsafe {
            super::PlaySoundW(super::PCWSTR(wav.as_ptr() as *const u16), None, super::SND_MEMORY | super::SND_NODEFAULT)
        };
        assert!(ok.as_bool(), "PlaySound failed");
    }

    #[test]
    #[ignore]
    fn plays_the_prayer_chime() {
        let wav = super::prayer_chime();
        let ok = unsafe {
            super::PlaySoundW(super::PCWSTR(wav.as_ptr() as *const u16), None, super::SND_MEMORY | super::SND_NODEFAULT)
        };
        assert!(ok.as_bool(), "PlaySound failed");
    }

    #[test]
    fn adhan_swells_then_stays_capped() {
        use super::{adhan_volume, ADHAN_MAX, ADHAN_START};
        assert!((adhan_volume(0.0) - ADHAN_START).abs() < 1e-9);
        let mut last = 0.0;
        for k in 0..=120 {
            let v = adhan_volume(k as f64 / 10.0);
            assert!(v >= last && v <= ADHAN_MAX + 1e-9);
            last = v;
        }
        assert!((adhan_volume(60.0) - ADHAN_MAX).abs() < 1e-9);
    }

    #[test]
    fn chime_is_a_valid_wav_of_the_right_length() {
        let w = super::chime();
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        let samples = (w.len() - 44) / 2;
        assert_eq!(samples, (super::RATE as f32 * super::LEN_S) as usize);
        // Loud at the start, silent at the very end.
        let s = |i: usize| i16::from_le_bytes([w[44 + 2 * i], w[45 + 2 * i]]).unsigned_abs();
        assert!((0..2000).map(s).max().unwrap() > 5000);
        assert!(s(samples - 1) < 50);
    }
}
