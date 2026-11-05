//! Lyrics from LRCLIB (https://lrclib.net), a free community lyrics library: synced ones (a
//! time for each line) when it has them, else just the words.

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
    /// Only the words, with no times: shown to read, not following the song.
    Plain(Vec<String>),
    Missing,
}

/// What a lookup found.
enum Got {
    Synced(Vec<Line>),
    Plain(Vec<String>),
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
                Some(Got::Synced(lines)) if !lines.is_empty() => State::Found(lines),
                Some(Got::Plain(lines)) if !lines.is_empty() => State::Plain(lines),
                _ => State::Missing,
            };
        });
    }
}

/// The words of plain lyrics, line by line (blank lines between verses dropped).
fn plain_lines(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

fn fetch(title: &str, artist: &str, duration_ms: i64) -> Option<Got> {
    // Synced lyrics if any entry has them; else the words of the closest one that has those.
    let mut plain: Option<String> = None;
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
                return Some(Got::Synced(parse_lrc(lrc)));
            }
            plain = v["plainLyrics"].as_str().filter(|t| !t.trim().is_empty()).map(str::to_owned);
        }
    }
    // The exact lookup is strict about duration and spelling; fall back to a search.
    let results: Option<Value> = ureq::get("https://lrclib.net/api/search")
        .set("User-Agent", USER_AGENT)
        .query("track_name", title)
        .query("artist_name", artist)
        .call()
        .ok()
        .and_then(|r| r.into_json().ok());
    let list = results.as_ref().and_then(|r| r.as_array()).cloned().unwrap_or_default();
    if let Some(lrc) = list.iter().find_map(|r| r["syncedLyrics"].as_str()) {
        return Some(Got::Synced(parse_lrc(lrc)));
    }
    if plain.is_none() {
        // (The entry nearest the song's length: shorter ones are often previews.)
        let secs = duration_ms as f64 / 1000.0;
        plain = list
            .iter()
            .filter(|r| r["plainLyrics"].as_str().is_some_and(|t| !t.trim().is_empty()))
            .min_by(|a, b| {
                let d = |r: &Value| (r["duration"].as_f64().unwrap_or(0.0) - secs).abs();
                d(a).total_cmp(&d(b))
            })
            .and_then(|r| r["plainLyrics"].as_str())
            .map(str::to_owned);
    }
    plain.map(|t| Got::Plain(plain_lines(&t)))
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
    fn plain_lyrics_lose_their_blank_lines() {
        assert_eq!(plain_lines("one\n\n  two  \r\nthree\n"), vec!["one", "two", "three"]);
    }

    /// A song LRCLIB has only the words of (run with --ignored: it asks LRCLIB).
    #[test]
    #[ignore]
    fn falls_back_to_the_plain_words() {
        match fetch("Seebk Enta", "Angham", 241_000) {
            Some(Got::Plain(lines)) => {
                println!("{} lines; first: {:?}", lines.len(), &lines[..3.min(lines.len())]);
                assert!(lines.len() > 5);
            }
            Some(Got::Synced(_)) => println!("it has synced lyrics now"),
            None => panic!("nothing found"),
        }
    }

    #[test]
    fn parses_lrc() {
        let l = parse_lrc("[00:12.50] hello\n[01:02.00][01:30.00] again\n[ar:Someone]");
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].ms, 12_500);
        assert_eq!(l[1].text, "again");
        assert_eq!(l[2].ms, 90_000);
    }
}
