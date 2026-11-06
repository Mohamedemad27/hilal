//! Quran Radio (Nova mode; in Nova Island a plugin): live Qur'an stations, played by Hilal
//! itself so they keep going in the background. The stations are mp3quran.net's (the
//! official Qur'an radio first, then the reciters' round-the-clock recitations), read once a
//! day; a dropped stream (no internet, the station restarted) is tried again, a little later
//! each time.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackItem, MediaPlaybackState, MediaPlayer};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Station {
    pub name: String,
    pub url: String,
    /// An official radio (else a reciter's recitations).
    pub official: bool,
}

/// Egypt's Qur'an radio on radiojar (the stream Cairo's station plays on the web).
const EGYPT_ID: &str = "8s5u5tpdtwzuv";

/// The station list, read in the background.
pub fn fetch() -> Receiver<Vec<Station>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let list = (|| -> Option<Vec<Station>> {
            let j: serde_json::Value = ureq::get("https://www.mp3quran.net/api/v3/radios?language=ar").timeout(Duration::from_secs(15)).call().ok()?.into_json().ok()?;
            let mut out: Vec<Station> = j["radios"]
                .as_array()?
                .iter()
                .filter_map(|r| {
                    let name = r["name"].as_str()?.trim().to_owned();
                    let url = r["url"].as_str()?.trim().to_owned();
                    url.starts_with("https://").then(|| Station { official: name.contains("\u{0625}\u{0630}\u{0627}\u{0639}\u{0629}"), name, url })
                })
                .collect();
            out.sort_by_key(|s| !s.official);
            // Egypt's Qur'an radio (from Cairo) isn't among them: after the other official ones.
            if !out.iter().any(|s| s.url.contains(EGYPT_ID) || s.name.contains("\u{0645}\u{0635}\u{0631}")) {
                let at = out.iter().take_while(|s| s.official).count();
                out.insert(at, Station { name: "\u{0625}\u{0630}\u{0627}\u{0639}\u{0629} \u{0627}\u{0644}\u{0642}\u{0631}\u{0622}\u{0646} \u{0627}\u{0644}\u{0643}\u{0631}\u{064a}\u{0645} - \u{0645}\u{0635}\u{0631}".to_owned(), url: format!("https://stream.radiojar.com/{EGYPT_ID}"), official: true });
            }
            Some(out)
        })();
        let _ = tx.send(list.unwrap_or_default());
    });
    rx
}

/// What the radio is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Off,
    Connecting,
    Live,
    /// The stream dropped; trying again.
    Retrying,
}

/// The player of a station: Windows' own, here in Hilal; or, while the drive the user's
/// folder is on is full, a Hilal of its own playing it (see `run_apart`).
enum Player {
    Here {
        player: MediaPlayer,
        /// What it plays (its name and picture shown to Windows on it).
        item: MediaPlaybackItem,
        /// The station's picture, being found.
        art: Option<Receiver<Vec<u8>>>,
        /// Its stream ended or failed (see `code`).
        ended: Arc<AtomicBool>,
    },
    Apart {
        child: std::process::Child,
        stdin: std::process::ChildStdin,
        /// What it last said it's doing (see `code`); `GONE` once it has ended.
        state: Arc<AtomicU8>,
    },
}

/// The states a separate player reports, one per line.
const OPENING: u8 = 0;
const PLAYING: u8 = 1;
const BUFFERING: u8 = 2;
const GONE: u8 = 3;
/// Paused by hand (Hilal's music page, a media key, Windows' own controls).
const PAUSED: u8 = 4;

/// `ended`: the stream ended or failed (a pause after that is no one's doing).
fn code(s: Option<MediaPlaybackState>, ended: bool) -> u8 {
    match s {
        Some(MediaPlaybackState::Opening) => OPENING,
        Some(MediaPlaybackState::Playing) => PLAYING,
        Some(MediaPlaybackState::Buffering) => BUFFERING,
        Some(MediaPlaybackState::Paused) if !ended => PAUSED,
        _ => GONE,
    }
}

/// Notes on `ended` when the player's stream ends or fails.
fn watch_end(p: &MediaPlayer, ended: &Arc<AtomicBool>) -> windows::core::Result<()> {
    type Args = Option<windows::core::IInspectable>;
    let e = ended.clone();
    p.MediaEnded(&windows::Foundation::TypedEventHandler::new(move |_, _: &Args| {
        e.store(true, Ordering::Relaxed);
        Ok(())
    }))?;
    let e = ended.clone();
    p.MediaFailed(&windows::Foundation::TypedEventHandler::new(move |_, _: &Option<windows::Media::Playback::MediaPlayerFailedEventArgs>| {
        e.store(true, Ordering::Relaxed);
        Ok(())
    }))?;
    Ok(())
}

/// Plays a station, named for Windows (its media controls, and so Hilal's own music views)
/// as the station under "Quran Radio"; its picture follows (see `show`).
fn new_player(url: &str, volume: f64, name: &str) -> windows::core::Result<(MediaPlayer, MediaPlaybackItem)> {
    let p = MediaPlayer::new()?;
    // A live stream, not a file: it lags less (fewer pauses to buffer).
    p.SetRealTimePlayback(true)?;
    let item = MediaPlaybackItem::Create(&MediaSource::CreateFromUri(&Uri::CreateUri(&HSTRING::from(url))?)?)?;
    let _ = show(&item, name, None);
    p.SetSource(&item)?;
    p.SetVolume(volume)?;
    p.Play()?;
    Ok((p, item))
}

/// Shows the station to Windows: its name, and its picture once there is one.
fn show(item: &MediaPlaybackItem, name: &str, art: Option<&[u8]>) -> windows::core::Result<()> {
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference};
    let props = item.GetDisplayProperties()?;
    props.SetType(windows::Media::MediaPlaybackType::Music)?;
    let music = props.MusicProperties()?;
    music.SetTitle(&HSTRING::from(name))?;
    music.SetArtist(&HSTRING::from("Quran Radio"))?;
    if let Some(bytes) = art {
        let stream = InMemoryRandomAccessStream::new()?;
        let w = DataWriter::CreateDataWriter(&stream)?;
        w.WriteBytes(bytes)?;
        w.StoreAsync()?.get()?;
        w.DetachStream()?;
        stream.Seek(0)?;
        props.SetThumbnail(&RandomAccessStreamReference::CreateFromStream(&stream)?)?;
    }
    item.ApplyDisplayProperties(&props)
}

/// The station's picture, found (or made) in the background.
fn find_art(name: &str, official: bool) -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    let name = name.to_owned();
    std::thread::spawn(move || {
        let _ = tx.send(crate::radio_art::picture(&name, official));
    });
    rx
}

/// Windows' player keeps a station's sound, as it comes, in the browser cache under the
/// user's folder: with that drive full it fails at once ("not enough space on the disk").
/// Then a user folder for the radio alone, on the fixed drive with the most room (if one has
/// some); None while the real one's drive has room.
fn spare_profile() -> Option<PathBuf> {
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
    const DRIVE_FIXED: u32 = 3;
    let free = |root: &str| -> u64 {
        let mut avail = 0u64;
        let ok = unsafe { GetDiskFreeSpaceExW(&HSTRING::from(root), Some(&mut avail), None, None) };
        if ok.is_ok() { avail } else { 0 }
    };
    let home = std::env::var("USERPROFILE").ok()?;
    let home_root: String = home.chars().take(3).collect();
    if free(&home_root) >= 300 << 20 {
        return None;
    }
    let best = ('A'..='Z')
        .map(|d| format!("{d}:\\"))
        .filter(|r| !r.eq_ignore_ascii_case(&home_root) && unsafe { GetDriveTypeW(&HSTRING::from(r.as_str())) } == DRIVE_FIXED)
        .max_by_key(|r| free(r))?;
    if free(&best) < 1 << 30 {
        return None;
    }
    let dir = PathBuf::from(best).join(".hilal-radio");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Starts a Hilal of its own playing a station, with its user folder in `profile`.
fn spawn_apart(s: &Station, volume: f64, profile: &std::path::Path) -> Option<Player> {
    use std::io::BufRead;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // (HILAL_RADIO_EXE: the Hilal to run, for the tests, which aren't one.)
    let exe = std::env::var_os("HILAL_RADIO_EXE").map(PathBuf::from).or_else(|| std::env::current_exe().ok())?;
    let mut child = std::process::Command::new(exe)
        .args(["--radio-player", &s.url, &volume.to_string(), &s.name, if s.official { "1" } else { "0" }])
        .env("USERPROFILE", profile)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let state = Arc::new(AtomicU8::new(OPENING));
    let st = state.clone();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Ok(c) = line.trim().parse::<u8>() {
                st.store(c, Ordering::Relaxed);
            }
        }
        st.store(GONE, Ordering::Relaxed);
    });
    Some(Player::Apart { child, stdin, state })
}

/// Hilal started as `hilal --radio-player <url> <volume> <name> <official 0/1>`: plays
/// the station (named, with its picture), takes "volume <v>" and "stop" lines, says what
/// it's doing (a code a line), and ends when told to or when Hilal goes (its input closes).
pub fn run_apart() -> windows::core::Result<()> {
    use std::io::Write;
    use std::sync::mpsc::RecvTimeoutError;
    let mut args = std::env::args().skip(2);
    let url = args.next().unwrap_or_default();
    let volume = args.next().and_then(|v| v.parse().ok()).unwrap_or(0.5);
    let name = args.next().unwrap_or_default();
    let official = args.next().as_deref() == Some("1");
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let (p, item) = new_player(&url, volume, &name)?;
    let ended = Arc::new(AtomicBool::new(false));
    let _ = watch_end(&p, &ended);
    let mut art = Some(find_art(&name, official));
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut out = std::io::stdout();
    let mut said = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                if let Some(v) = line.trim().strip_prefix("volume ").and_then(|v| v.parse().ok()) {
                    let _ = p.SetVolume(v);
                } else if line.trim() == "stop" {
                    break;
                } else if line.trim() == "pause" {
                    let _ = p.Pause();
                } else if line.trim() == "play" {
                    let _ = p.Play();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Some(bytes) = art.as_ref().and_then(|a| a.try_recv().ok()) {
            art = None;
            let _ = show(&item, &name, Some(&bytes));
        }
        let now = code(p.PlaybackSession().ok().and_then(|s| s.PlaybackState().ok()), ended.load(Ordering::Relaxed));
        if said != Some(now) {
            said = Some(now);
            if writeln!(out, "{now}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    let _ = p.Pause();
    let _ = p.Close();
    Ok(())
}

pub struct Radio {
    player: Option<Player>,
    pub want: bool,
    pub volume: f64,
    station: Option<Station>,
    retry_at: Option<Instant>,
    retries: u32,
    started: Instant,
    /// When it was last heard playing (since it was last started).
    alive_at: Option<Instant>,
}

/// How long a stream may stay opening or buffering (since it last played, or started)
/// before it's started again.
const PATIENCE: Duration = Duration::from_secs(15);

impl Radio {
    pub fn new(volume: f64) -> Self {
        Self { player: None, want: false, volume, station: None, retry_at: None, retries: 0, started: Instant::now(), alive_at: None }
    }

    fn connect(&mut self) {
        self.stop_player();
        let Some(s) = self.station.clone() else { return };
        self.player = match spare_profile() {
            Some(profile) => {
                crate::ui::trace(&format!("[radio] the user folder's drive is full: playing apart, its folder {}", profile.display()));
                spawn_apart(&s, self.volume, &profile)
            }
            None => new_player(&s.url, self.volume, &s.name).ok().map(|(player, item)| {
                let ended = Arc::new(AtomicBool::new(false));
                let _ = watch_end(&player, &ended);
                Player::Here { player, item, art: Some(find_art(&s.name, s.official)), ended }
            }),
        };
        self.started = Instant::now();
        self.alive_at = None;
    }

    fn stop_player(&mut self) {
        match self.player.take() {
            Some(Player::Here { player: p, .. }) => {
                let _ = p.Pause();
                let _ = p.Close();
            }
            Some(Player::Apart { mut child, mut stdin, .. }) => {
                use std::io::Write;
                let _ = writeln!(stdin, "stop");
                drop(stdin);
                // (It ends by itself in a moment; reaped meanwhile so it leaves no trace.)
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(2));
                    let _ = child.kill();
                    let _ = child.wait();
                });
            }
            None => {}
        }
    }

    /// What the player is doing.
    fn playback(&self) -> Option<MediaPlaybackState> {
        match self.player.as_ref()? {
            Player::Here { player: p, ended, .. } => match code(p.PlaybackSession().ok().and_then(|s| s.PlaybackState().ok()), ended.load(Ordering::Relaxed)) {
                OPENING => Some(MediaPlaybackState::Opening),
                PLAYING => Some(MediaPlaybackState::Playing),
                BUFFERING => Some(MediaPlaybackState::Buffering),
                PAUSED => Some(MediaPlaybackState::Paused),
                _ => None,
            },
            Player::Apart { state, .. } => match state.load(Ordering::Relaxed) {
                OPENING => Some(MediaPlaybackState::Opening),
                PLAYING => Some(MediaPlaybackState::Playing),
                BUFFERING => Some(MediaPlaybackState::Buffering),
                PAUSED => Some(MediaPlaybackState::Paused),
                _ => None,
            },
        }
    }

    pub fn play(&mut self, s: &Station) {
        self.station = Some(s.clone());
        self.want = true;
        self.retries = 0;
        self.retry_at = None;
        self.connect();
    }

    pub fn stop(&mut self) {
        self.want = false;
        self.retry_at = None;
        self.stop_player();
    }

    pub fn set_volume(&mut self, v: f64) {
        self.volume = v.clamp(0.0, 1.0);
        match &mut self.player {
            Some(Player::Here { player: p, .. }) => {
                let _ = p.SetVolume(self.volume);
            }
            Some(Player::Apart { stdin, .. }) => {
                use std::io::Write;
                let _ = writeln!(stdin, "volume {}", self.volume);
            }
            None => {}
        }
    }

    pub fn state(&self) -> State {
        if !self.want {
            return State::Off;
        }
        if self.retry_at.is_some() {
            return State::Retrying;
        }
        // (A moment's buffering while it plays is still live.)
        match self.playback() {
            Some(MediaPlaybackState::Playing) => State::Live,
            Some(MediaPlaybackState::Buffering) if self.alive_at.is_some_and(|t| t.elapsed() < PATIENCE) => State::Live,
            _ => State::Connecting,
        }
    }

    /// Keeps a wanted stream going. Opening or buffering, it gets 15 s from when it last
    /// played (or started); stopped, a moment. Then it's started again: at once if it had
    /// been playing, else after 3, 6, 12 ... s (at most a minute). Paused by hand, the radio
    /// is off (its player kept: played again the same way, it's on again).
    pub fn tick(&mut self) {
        if !self.want {
            if self.player.is_some() && self.playback() == Some(MediaPlaybackState::Playing) {
                crate::ui::trace("[radio] played again by hand");
                self.want = true;
                self.alive_at = Some(Instant::now());
            }
            return;
        }
        if self.retry_at.is_none() && self.playback() == Some(MediaPlaybackState::Paused) {
            crate::ui::trace("[radio] paused by hand: off");
            self.want = false;
            return;
        }
        if let Some(at) = self.retry_at {
            if Instant::now() >= at {
                self.retry_at = None;
                self.connect();
            }
            return;
        }
        // The station's picture, once found.
        if let (Some(Player::Here { item, art, .. }), Some(s)) = (&mut self.player, &self.station) {
            if let Some(bytes) = art.as_ref().and_then(|a| a.try_recv().ok()) {
                *art = None;
                let _ = show(item, &s.name, Some(&bytes));
            }
        }
        let since = self.alive_at.unwrap_or(self.started);
        let stuck = match self.playback() {
            Some(MediaPlaybackState::Playing) => {
                self.retries = 0;
                self.alive_at = Some(Instant::now());
                false
            }
            Some(MediaPlaybackState::Opening) | Some(MediaPlaybackState::Buffering) => since.elapsed() > PATIENCE,
            _ => since.elapsed() > Duration::from_secs(2),
        };
        if stuck {
            let played = self.alive_at.is_some();
            crate::ui::trace(&format!("[radio] stuck ({:?}, {} s since it last played or started): again", self.playback(), since.elapsed().as_secs()));
            self.stop_player();
            let wait = if played {
                1
            } else {
                self.retries += 1;
                (3u64 << (self.retries - 1).min(4)).min(60)
            };
            self.retry_at = Some(Instant::now() + Duration::from_secs(wait));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays a station (HILAL_RADIO_URL, else Egypt's) very quietly for HILAL_RADIO_SECS (120)
    /// seconds as Hilal does, and says how much of the time it was live. Run with --ignored
    /// --nocapture (network, sound); HILAL_FPS=<file> logs each start again.
    #[test]
    #[ignore]
    fn stays_on() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let url = std::env::var("HILAL_RADIO_URL").unwrap_or_else(|_| format!("https://stream.radiojar.com/{EGYPT_ID}"));
        let secs: u64 = std::env::var("HILAL_RADIO_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(120);
        let mut r = Radio::new(0.02);
        r.play(&Station { name: "test".to_owned(), url, official: true });
        let t0 = Instant::now();
        let (mut live, mut all) = (0u32, 0u32);
        while t0.elapsed() < Duration::from_secs(secs) {
            r.tick();
            all += 1;
            if r.state() == State::Live {
                live += 1;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        r.stop();
        println!("live {}% of {secs} s", live * 100 / all.max(1));
    }

    /// Plays Egypt's station very quietly, pauses it as Windows' controls would, and plays it
    /// again: paused, it must stay off (not be started again); played, on. Run with
    /// --ignored --nocapture; HILAL_RADIO_EXE=<a Hilal> when it plays apart.
    #[test]
    #[ignore]
    fn pause_stays_paused() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let mut r = Radio::new(0.02);
        r.play(&Station { name: "test".to_owned(), url: format!("https://stream.radiojar.com/{EGYPT_ID}"), official: true });
        let run = |r: &mut Radio, secs: u64| {
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(secs) {
                r.tick();
                std::thread::sleep(Duration::from_millis(100));
            }
        };
        let hand = |r: &mut Radio, play: bool| match &mut r.player {
            Some(Player::Here { player, .. }) => {
                let _ = if play { player.Play() } else { player.Pause() };
            }
            Some(Player::Apart { stdin, .. }) => {
                use std::io::Write;
                let _ = writeln!(stdin, "{}", if play { "play" } else { "pause" });
            }
            None => {}
        };
        run(&mut r, 8);
        println!("playing: {:?}", r.state());
        assert_eq!(r.state(), State::Live);
        hand(&mut r, false);
        run(&mut r, 10);
        println!("10 s after a pause: {:?} (wanted {})", r.state(), r.want);
        assert_eq!(r.state(), State::Off);
        hand(&mut r, true);
        run(&mut r, 6);
        println!("played again: {:?}", r.state());
        assert_eq!(r.state(), State::Live);
        r.stop();
    }
}