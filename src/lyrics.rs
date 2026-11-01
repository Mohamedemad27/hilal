//! Synced lyrics from LRCLIB (https://lrclib.net), a free community lyrics library.

use std::sync::{Arc, Mutex};

use serde_json::Value;

const USER_AGENT: &str = "Hilal/0.1 (Windows)";

pub struct Line {
    pub ms: i64,
    pub text: String,
}

#[derive(Default)]
pub enum State {
    #[default]
    Idle,
    Loading,
    Found(Vec<Line>),
    Missing,
}

/// Fetches lyrics in the background whenever the track changes.
#[derive(Default)]
pub struct Lyrics {
    key: String,
    pub state: Arc<Mutex<State>>,
}

impl Lyrics {
    pub fn follow(&mut self, title: &str, artist: &str, duration_ms: i64) {
        let key = format!("{title}\u{1}{artist}");
        if key == self.key {
            return;
        }
        self.key = key;
        // A fresh Arc per track, so a slow lookup for the previous song can't overwrite this one.
        self.state = Arc::new(Mutex::new(State::Loading));
        let state = self.state.clone();
        let (title, artist) = (title.to_owned(), artist.to_owned());
        std::thread::spawn(move || {
            let found = fetch(&title, &artist, duration_ms);
            *state.lock().unwrap() = match found {
                Some(lines) if !lines.is_empty() => State::Found(lines),
                _ => State::Missing,
            };
        });
    }
}

fn fetch(title: &str, artist: &str, duration_ms: i64) -> Option<Vec<Line>> {
    let mut req = ureq::get("https://lrclib.net/api/get")
        .set("User-Agent", USER_AGENT)
        .query("track_name", title)
        .query("artist_name", artist);
    if duration_ms > 0 {
        req = req.query("duration", &(duration_ms / 1000).to_string());
    }
    if let Ok(resp) = req.call() {
        if let Ok(v) = resp.into_json::<Value>() {
            if let Some(lrc) = v["syncedLyrics"].as_str() {
                return Some(parse_lrc(lrc));
            }
        }
    }
    // The exact lookup is strict about duration and spelling; fall back to a search.
    let results: Value = ureq::get("https://lrclib.net/api/search")
        .set("User-Agent", USER_AGENT)
        .query("track_name", title)
        .query("artist_name", artist)
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let lrc = results
        .as_array()?
        .iter()
        .find_map(|r| r["syncedLyrics"].as_str())?;
    Some(parse_lrc(lrc))
}

/// Parses "[mm:ss.xx] text" lines. A line may carry several timestamps.
fn parse_lrc(lrc: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(after) = rest.strip_prefix('[') {
            let Some(close) = after.find(']') else { break };
            if let Some(ms) = parse_stamp(&after[..close]) {
                stamps.push(ms);
            }
            rest = &after[close + 1..];
        }
        let text = rest.trim().to_owned();
        for ms in stamps {
            lines.push(Line { ms, text: text.clone() });
        }
    }
    lines.sort_by_key(|l| l.ms);
    lines
}

fn parse_stamp(s: &str) -> Option<i64> {
    let (m, sec) = s.split_once(':')?;
    let m: i64 = m.trim().parse().ok()?;
    let sec: f64 = sec.trim().parse().ok()?;
    Some(m * 60_000 + (sec * 1000.0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc() {
        let l = parse_lrc("[00:12.50] hello\n[01:02.00][01:30.00] again\n[ar:Someone]");
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].ms, 12_500);
        assert_eq!(l[1].text, "again");
        assert_eq!(l[2].ms, 90_000);
    }
}
