//! Now-playing info and controls through Windows' global media session (SMTC).
//! Works with Spotify, browsers, the Media Player app, and anything else that shows
//! up in the Windows volume flyout.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::win;

#[derive(Clone, Default, PartialEq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// All times are in 100 ns ticks.
    pub position: i64,
    pub end: i64,
    pub updated_at: i64,
    /// Encoded cover image (PNG/JPEG/BMP), when the player provides one.
    pub art: Option<Arc<Vec<u8>>>,
}

impl Track {
    /// Playback position right now, in milliseconds, extrapolated from the last update.
    pub fn position_ms(&self) -> i64 {
        let mut pos = self.position;
        if self.playing && self.updated_at > 0 {
            pos += (win::filetime_now() - self.updated_at).max(0);
        }
        if self.end > 0 {
            pos = pos.min(self.end);
        }
        pos / 10_000
    }

    pub fn end_ms(&self) -> i64 {
        self.end / 10_000
    }
}

pub enum Command {
    PlayPause,
    Next,
    Previous,
    /// Jump to a position, in milliseconds.
    Seek(i64),
}

pub struct Media {
    pub track: Arc<Mutex<Option<Track>>>,
    tx: Sender<Command>,
}

impl Media {
    pub fn start() -> Self {
        let track = Arc::new(Mutex::new(None));
        let (tx, rx) = channel();
        let shared = track.clone();
        std::thread::spawn(move || run(shared, rx));
        Self { track, tx }
    }

    pub fn current(&self) -> Option<Track> {
        self.track.lock().unwrap().clone()
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }
}

fn run(shared: Arc<Mutex<Option<Track>>>, rx: Receiver<Command>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let manager = loop {
        match Manager::RequestAsync().and_then(|op| op.get()) {
            Ok(m) => break m,
            Err(_) => std::thread::sleep(Duration::from_secs(5)),
        }
    };
    // Players often publish the cover a moment after the title, so retry a few times per track.
    let (mut art_key, mut art, mut art_tries) = (String::new(), None, 0);
    loop {
        let session = manager.GetCurrentSession().ok();
        let mut current = None;
        if let Some((mut track, props)) = session.as_ref().and_then(|s| read(s).ok()) {
            let key = format!("{}\u{1}{}", track.title, track.artist);
            if key != art_key {
                (art_key, art, art_tries) = (key, None, 0);
            }
            if art.is_none() && art_tries < 6 {
                art_tries += 1;
                art = thumbnail(&props).ok().filter(|b| !b.is_empty()).map(Arc::new);
            }
            track.art = art.clone();
            current = Some(track);
        }
        *shared.lock().unwrap() = current;
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(cmd) => {
                if let Some(s) = &session {
                    let _ = match cmd {
                        Command::PlayPause => s.TryTogglePlayPauseAsync().and_then(|op| op.get()),
                        Command::Next => s.TrySkipNextAsync().and_then(|op| op.get()),
                        Command::Previous => s.TrySkipPreviousAsync().and_then(|op| op.get()),
                        Command::Seek(ms) => s.TryChangePlaybackPositionAsync(ms * 10_000).and_then(|op| op.get()),
                    };
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn read(s: &Session) -> windows::core::Result<(Track, Properties)> {
    let props = s.TryGetMediaPropertiesAsync()?.get()?;
    let timeline = s.GetTimelineProperties()?;
    let playback = s.GetPlaybackInfo()?;
    let track = Track {
        title: props.Title()?.to_string_lossy(),
        artist: props.Artist()?.to_string_lossy(),
        playing: playback.PlaybackStatus()? == Status::Playing,
        position: timeline.Position()?.Duration,
        end: timeline.EndTime()?.Duration,
        updated_at: timeline.LastUpdatedTime()?.UniversalTime,
        art: None,
    };
    Ok((track, props))
}

fn thumbnail(props: &Properties) -> windows::core::Result<Vec<u8>> {
    let stream = props.Thumbnail()?.OpenReadAsync()?.get()?;
    let size = stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size)?.get()?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}
