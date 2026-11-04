//! Saves a video, or just its sound, from a link (YouTube and the many other sites yt-dlp
//! knows). yt-dlp does the fetching and ffmpeg the converting; both live in a `tools`
//! folder beside hilal.exe. Files land in Downloads\Hilal.

use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

/// No console window for the tools.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CF_UNICODETEXT: u32 = 13;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Mp3,
    Mp4,
}

/// What to save (chosen before the download): the sound only or the video, in which
/// format, at what quality (a video's height, a sound's bitrate; None: the best there is),
/// and optionally the file's name and the artist and album written into it.
#[derive(Clone, PartialEq, Debug)]
pub struct Options {
    pub audio: bool,
    pub format: &'static str,
    pub quality: Option<u32>,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

pub const VIDEO_FORMATS: [&str; 3] = ["mp4", "mkv", "webm"];
pub const AUDIO_FORMATS: [&str; 4] = ["mp3", "m4a", "opus", "wav"];
/// Video heights to offer (those the video has), and sound bitrates (kbps).
pub const HEIGHTS: [u32; 7] = [2160, 1440, 1080, 720, 480, 360, 240];
pub const BITRATES: [u32; 3] = [320, 192, 128];

impl Options {
    /// The plain choice of the page's MP3 and MP4 buttons: the best there is.
    pub fn of(kind: Kind) -> Self {
        Self { audio: kind == Kind::Mp3, format: if kind == Kind::Mp3 { "mp3" } else { "mp4" }, quality: None, name: None, artist: None, album: None }
    }
}

/// What a link holds, read before downloading: the title, the artist and album (as the site
/// gives them, else the uploader), and the video heights there are.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub heights: Vec<u32>,
}

/// Reads what a link holds, in the background (yt-dlp's JSON, nothing downloaded).
pub fn probe(url: &str) -> std::sync::mpsc::Receiver<Option<Info>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let url = url.to_owned();
    std::thread::spawn(move || {
        let out = Command::new(tools_dir().join("yt-dlp.exe"))
            .args(["-J", "--no-playlist", "--no-warnings", "--js-runtimes", "node", "--", &url])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .env("PYTHONIOENCODING", "utf-8")
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let info = out.ok().filter(|o| o.status.success()).and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()).map(|j| info_of(&j));
        let _ = tx.send(info);
    });
    rx
}

fn info_of(j: &serde_json::Value) -> Info {
    let s = |k: &str| j[k].as_str().unwrap_or("").trim().to_owned();
    let mut heights: Vec<u32> = j["formats"]
        .as_array()
        .map(|f| f.iter().filter(|f| f["vcodec"].as_str().is_some_and(|v| v != "none")).filter_map(|f| f["height"].as_u64().map(|h| h as u32)).collect())
        .unwrap_or_default();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    let artist = [s("artist"), s("creator"), s("uploader"), s("channel")].into_iter().find(|a| !a.is_empty()).unwrap_or_default();
    Info { title: s("title"), artist, album: s("album"), heights }
}

/// A name Windows takes for a file (no \ / : * ? " < > |, no dots or spaces at the end).
pub fn file_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    clean.trim().trim_end_matches(['.', ' ']).chars().take(150).collect()
}

#[derive(Clone, PartialEq, Debug)]
pub enum Phase {
    Starting,
    Downloading,
    /// Turning it into mp3, or joining picture and sound into one mp4.
    Finishing,
    Done(PathBuf),
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Status {
    pub title: String,
    /// 0..1 through the current download (video and sound may come one after the other).
    pub progress: f32,
    pub phase: Phase,
}

/// One download running in the background.
pub struct Job {
    pub status: Arc<Mutex<Status>>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Job {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn running(&self) -> bool {
        matches!(self.status().phase, Phase::Starting | Phase::Downloading | Phase::Finishing)
    }

    pub fn cancel(&self) {
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn tools_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("tools"))).unwrap_or_default()
}

/// yt-dlp and ffmpeg are both there.
pub fn tools_ready() -> bool {
    tools_dir().join("yt-dlp.exe").is_file() && tools_dir().join("ffmpeg.exe").is_file()
}

/// Downloads\Hilal (made if needed).
pub fn out_dir() -> PathBuf {
    let downloads = unsafe { SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, HANDLE::default()) }
        .ok()
        .and_then(|p| unsafe { p.to_string() }.ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join("Downloads"));
    downloads.join("Hilal")
}

/// The clipboard's text, if it holds a web link.
pub fn clipboard_link() -> Option<String> {
    unsafe {
        OpenClipboard(None).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(g);
            Some(s)
        });
        let _ = CloseClipboard();
        let t = text?.trim().to_owned();
        (t.starts_with("http://") || t.starts_with("https://")).then_some(t)
    }
}

/// The yt-dlp arguments for a download.
fn args(o: &Options, url: &str, out: &std::path::Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "--newline",
        "--no-playlist",
        "--no-colors",
        "--no-simulate",
        "--encoding",
        "utf-8",
        "--progress",
        "--js-runtimes",
        "node",
        "--progress-template",
        "download:P %(progress._percent_str)s",
        "--print",
        "before_dl:T %(title)s",
        "--print",
        "after_move:F %(filepath)s",
        "--ffmpeg-location",
    ]
    .map(String::from)
    .to_vec();
    a.push(tools_dir().display().to_string());
    a.push("-o".into());
    let name = o.name.as_deref().map(file_name).filter(|n| !n.is_empty());
    // (A % in a chosen name is meant as itself, not as a field.)
    let pattern = name.map_or_else(|| "%(title).150B".to_owned(), |n| n.replace('%', "%%"));
    a.push(out.join(format!("{pattern}.%(ext)s")).display().to_string());
    if o.audio {
        let q = match o.quality {
            Some(k) => format!("{k}K"),
            None => "0".to_owned(),
        };
        a.extend(["-f", "bestaudio/best", "-x", "--audio-format", o.format, "--audio-quality", &q].map(String::from));
    } else {
        let h = o.quality.map_or(String::new(), |h| format!("[height<={h}]"));
        let f = match o.format {
            "mp4" => format!("bv*{h}[ext=mp4]+ba[ext=m4a]/b{h}[ext=mp4]/bv*{h}+ba/b{h}"),
            "webm" => format!("bv*{h}[ext=webm]+ba[ext=webm]/bv*{h}+ba/b{h}"),
            _ => format!("bv*{h}+ba/b{h}"),
        };
        a.extend(["-f".to_owned(), f, "--merge-output-format".to_owned(), o.format.to_owned()]);
    }
    // The tags: the title, and the artist and album as given (yt-dlp's own guesses when not).
    a.push("--embed-metadata".into());
    for (field, v) in [("meta_artist", &o.artist), ("meta_album", &o.album)] {
        if let Some(v) = v.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            // (A colon would split the instruction; the full-width one looks the same.)
            a.push("--parse-metadata".into());
            a.push(format!("{}:%({field})s", v.replace(':', "\u{ff1a}").replace('%', "%%")));
        }
    }
    a.push("--".into());
    a.push(url.to_owned());
    a
}

/// Reads one line of yt-dlp's output into the status.
fn read_line(line: &str, s: &mut Status) {
    if let Some(t) = line.strip_prefix("T ") {
        s.title = t.trim().to_owned();
    } else if let Some(p) = line.strip_prefix("P ") {
        if let Ok(v) = p.trim().trim_end_matches('%').trim().parse::<f32>() {
            s.progress = (v / 100.0).clamp(0.0, 1.0);
            s.phase = if v >= 100.0 { Phase::Finishing } else { Phase::Downloading };
        }
    } else if let Some(f) = line.strip_prefix("F ") {
        s.phase = Phase::Done(PathBuf::from(f.trim()));
    }
}

/// Starts downloading `url` as `kind` (the best there is).
#[allow(dead_code)]
pub fn start(url: &str, kind: Kind) -> Job {
    start_with(url, &Options::of(kind))
}

/// Starts downloading `url` with the options chosen.
pub fn start_with(url: &str, opts: &Options) -> Job {
    let status = Arc::new(Mutex::new(Status { title: String::new(), progress: 0.0, phase: Phase::Starting }));
    let child_slot = Arc::new(Mutex::new(None));
    let job = Job { status: status.clone(), child: child_slot.clone() };
    let out = out_dir();
    let fail = |msg: String| status.lock().unwrap().phase = Phase::Failed(msg);
    if !tools_ready() {
        fail("Download tools are missing".into());
        return job;
    }
    if std::fs::create_dir_all(&out).is_err() {
        fail("Can't make the Downloads\\Hilal folder".into());
        return job;
    }
    let spawned = Command::new(tools_dir().join("yt-dlp.exe"))
        .args(args(opts, url, &out))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONIOENCODING", "utf-8")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            fail(format!("Couldn't start yt-dlp: {e}"));
            return job;
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    *child_slot.lock().unwrap() = Some(child);

    // The last error yt-dlp printed, to show if it fails.
    let error = Arc::new(Mutex::new(String::new()));
    if let Some(err) = stderr {
        let error = error.clone();
        std::thread::spawn(move || {
            for line in lines(err) {
                // With HILAL_FPS=<file> set, keep yt-dlp's messages there (for debugging).
                if let Some(path) = std::env::var_os("HILAL_FPS") {
                    use std::io::Write;
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                        let _ = writeln!(f, "  [yt-dlp] {line}");
                    }
                }
                if let Some(e) = line.strip_prefix("ERROR: ") {
                    *error.lock().unwrap() = e.to_owned();
                }
            }
        });
    }
    std::thread::spawn(move || {
        if let Some(out) = stdout {
            for line in lines(out) {
                read_line(&line, &mut status.lock().unwrap());
            }
        }
        let ok = child_slot.lock().unwrap().as_mut().and_then(|c| c.wait().ok()).is_some_and(|s| s.success());
        let mut s = status.lock().unwrap();
        if !matches!(s.phase, Phase::Done(_)) || !ok {
            let e = error.lock().unwrap().clone();
            s.phase = Phase::Failed(if e.is_empty() { "Download stopped".into() } else { short_error(&e) });
        }
    });
    job
}

/// The lines of a pipe, read to its end even where one isn't valid UTF-8 (stopping early
/// would close the pipe under yt-dlp and make it fail).
fn lines(r: impl std::io::Read) -> impl Iterator<Item = String> {
    let mut r = BufReader::new(r);
    std::iter::from_fn(move || {
        let mut buf = Vec::new();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_owned()),
        }
    })
}

/// yt-dlp's errors start with the extractor and video id; keep the readable part.
fn short_error(e: &str) -> String {
    let e = if e.starts_with('[') { e.split_once(": ").map_or(e, |(_, rest)| rest) } else { e }.trim();
    let e = e.split(". ").next().unwrap_or(e);
    e.chars().take(80).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_progress_title_and_file() {
        let mut s = Status { title: String::new(), progress: 0.0, phase: Phase::Starting };
        read_line("T Some video", &mut s);
        read_line("P  42.5%", &mut s);
        assert_eq!(s.title, "Some video");
        assert!((s.progress - 0.425).abs() < 1e-6);
        assert_eq!(s.phase, Phase::Downloading);
        read_line("P 100.0%", &mut s);
        assert_eq!(s.phase, Phase::Finishing);
        read_line(r"F C:\x\Some video.mp3", &mut s);
        assert_eq!(s.phase, Phase::Done(PathBuf::from(r"C:\x\Some video.mp3")));
    }

    #[test]
    fn errors_are_shortened() {
        assert_eq!(short_error("[youtube] abc: Video unavailable. This video is private"), "Video unavailable");
    }

    #[test]
    fn mp3_asks_for_audio_and_mp4_for_merged_video() {
        let a = args(&Options::of(Kind::Mp3), "https://x", std::path::Path::new("D"));
        assert!(a.windows(2).any(|w| w == ["--audio-format", "mp3"]));
        let v = args(&Options::of(Kind::Mp4), "https://x", std::path::Path::new("D"));
        assert!(v.windows(2).any(|w| w == ["--merge-output-format", "mp4"]));
        assert_eq!(v.last().unwrap(), "https://x");
    }

    #[test]
    fn options_set_quality_format_name_and_tags() {
        let o = Options { audio: false, format: "webm", quality: Some(720), name: Some("My: clip?".into()), artist: Some("A: B".into()), album: None };
        let a = args(&o, "https://x", std::path::Path::new("D"));
        assert!(a.windows(2).any(|w| w[0] == "-f" && w[1].starts_with("bv*[height<=720][ext=webm]")), "{a:?}");
        assert!(a.windows(2).any(|w| w == ["--merge-output-format", "webm"]));
        assert!(a.iter().any(|x| x.ends_with("My_ clip_.%(ext)s")), "{a:?}");
        assert!(a.windows(2).any(|w| w[0] == "--parse-metadata" && w[1] == "A\u{ff1a} B:%(meta_artist)s"), "{a:?}");
        let s = Options { audio: true, format: "opus", quality: Some(128), name: None, artist: None, album: None };
        let a = args(&s, "https://x", std::path::Path::new("D"));
        assert!(a.windows(2).any(|w| w == ["--audio-quality", "128K"]));
        assert!(a.iter().any(|x| x.ends_with("%(title).150B.%(ext)s")));
    }
}
