//! Quran Radio (Nova mode; in Nova Island a plugin): live Qur'an stations, played by Hilal
//! itself so they keep going in the background. The stations are mp3quran.net's (the
//! official Qur'an radio first, then the reciters' round-the-clock recitations), read once a
//! day; a dropped stream (no internet, the station restarted) is tried again, a little later
//! each time.

use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Foundation::Uri;
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackState, MediaPlayer};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Station {
    pub name: String,
    pub url: String,
    /// An official radio (else a reciter's recitations).
    pub official: bool,
}

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

pub struct Radio {
    player: Option<MediaPlayer>,
    pub want: bool,
    pub volume: f64,
    url: String,
    retry_at: Option<Instant>,
    retries: u32,
    started: Instant,
}

impl Radio {
    pub fn new(volume: f64) -> Self {
        Self { player: None, want: false, volume, url: String::new(), retry_at: None, retries: 0, started: Instant::now() }
    }

    fn connect(&mut self) {
        self.stop_player();
        let made = (|| -> windows::core::Result<MediaPlayer> {
            let p = MediaPlayer::new()?;
            p.SetSource(&MediaSource::CreateFromUri(&Uri::CreateUri(&HSTRING::from(self.url.as_str()))?)?)?;
            p.SetVolume(self.volume)?;
            p.Play()?;
            Ok(p)
        })();
        self.player = made.ok();
        self.started = Instant::now();
    }

    fn stop_player(&mut self) {
        if let Some(p) = self.player.take() {
            let _ = p.Pause();
            let _ = p.Close();
        }
    }

    pub fn play(&mut self, url: &str) {
        self.url = url.to_owned();
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
        if let Some(p) = &self.player {
            let _ = p.SetVolume(self.volume);
        }
    }

    pub fn state(&self) -> State {
        if !self.want {
            return State::Off;
        }
        if self.retry_at.is_some() {
            return State::Retrying;
        }
        let playing = self.player.as_ref().and_then(|p| p.PlaybackSession().ok()).and_then(|s| s.PlaybackState().ok()) == Some(MediaPlaybackState::Playing);
        if playing { State::Live } else { State::Connecting }
    }

    /// Keeps a wanted stream going: one that hasn't started in 15 s, or stopped, is tried
    /// again after 3, 6, 12 ... s (at most a minute).
    pub fn tick(&mut self) {
        if !self.want {
            return;
        }
        if let Some(at) = self.retry_at {
            if Instant::now() >= at {
                self.retry_at = None;
                self.connect();
            }
            return;
        }
        let state = self.player.as_ref().and_then(|p| p.PlaybackSession().ok()).and_then(|s| s.PlaybackState().ok());
        let stuck = match state {
            Some(MediaPlaybackState::Playing) => {
                self.retries = 0;
                false
            }
            Some(MediaPlaybackState::Opening) | Some(MediaPlaybackState::Buffering) => self.started.elapsed() > Duration::from_secs(15),
            _ => self.started.elapsed() > Duration::from_secs(3),
        };
        if stuck {
            self.stop_player();
            let wait = (3u64 << self.retries.min(4)).min(60);
            self.retries += 1;
            self.retry_at = Some(Instant::now() + Duration::from_secs(wait));
        }
    }
}
