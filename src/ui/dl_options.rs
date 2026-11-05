//! The download options (asked for in Hilal's issue #1): before a download starts, a small
//! window under the notch with drop-down menus for the type (video or sound), the format and
//! the quality (only what the video has), and boxes for the file's name and for the artist
//! and album written into it, filled in from what the site says, to fix what it gets wrong.

use std::cell::RefCell;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{App, Kind, PP_W};
use crate::download::{self, Info, Options};
use crate::gfx::{rgb, rgba, white, Align, Rect, WHITE};

pub const DW_W: f32 = 420.0;
pub const DW_H: f32 = 366.0;
const ROW: f32 = 38.0;
const TOP: f32 = 70.0;
const LABEL_W: f32 = 96.0;
const VIOLET: crate::gfx::Color = rgb(0xa8, 0x7b, 0xff);

/// A click in the window.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Do {
    Close,
    /// Opens a drop-down (0 type, 1 format, 2 quality).
    Menu(u8),
    Field(usize),
    Download,
}

/// What can't run while the app is borrowed: a drop-down menu (it runs its own loop).
pub enum After {
    Menu(u8),
}

pub struct DlWin {
    pub hwnd: HWND,
    surface: Option<usize>,
    pub shown: bool,
    pub opts: Options,
    /// What the link holds (while it's read: None and `probing`).
    info: Option<Info>,
    probing: Option<std::sync::mpsc::Receiver<Option<Info>>>,
    probe_failed: bool,
    /// The name, artist and album boxes, which one is being typed in, and which were changed
    /// by hand (those aren't overwritten when the video's info comes).
    fields: [String; 3],
    edited: [bool; 3],
    pub focus: Option<usize>,
    cursor: (f32, f32),
    pressed: Option<Do>,
    regions: RefCell<Vec<(Rect, Do)>>,
    opened_at: Instant,
    /// Drawn again until then after a change (a frame the screen wasn't ready for is
    /// dropped, and nothing else may come to replace it).
    redraw_until: Option<Instant>,
}

impl DlWin {
    pub fn new() -> Self {
        Self {
            hwnd: HWND::default(),
            surface: None,
            shown: false,
            opts: Options::of(Kind::Mp4),
            info: None,
            probing: None,
            probe_failed: false,
            fields: Default::default(),
            edited: [false; 3],
            focus: None,
            cursor: (-1.0, -1.0),
            pressed: None,
            regions: RefCell::new(Vec::new()),
            opened_at: Instant::now(),
            redraw_until: None,
        }
    }

    fn at(&self, x: f32, y: f32) -> Option<Do> {
        self.regions.borrow().iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, d)| *d)
    }
}

/// "1080p" or "320 kbps"; None is the best there is.
fn quality_text(audio: bool, q: Option<u32>) -> String {
    match q {
        None => "Best".to_owned(),
        Some(h) if !audio => format!("{h}p"),
        Some(k) => format!("{k} kbps"),
    }
}

impl App {
    /// Opens the options for the link pasted (MP3 or MP4 as chosen on the page).
    pub(super) fn open_dl_options(&mut self) {
        let Some(url) = self.dl_url.clone() else { return };
        let k = self.gfx.scale;
        if self.dl_win.hwnd.is_invalid() {
            let Some(h) = crate::create_dl_window() else { return };
            self.dl_win.hwnd = h;
        }
        let w = &mut self.dl_win;
        w.opts = Options::of(self.dl_kind);
        w.info = None;
        w.probe_failed = false;
        w.fields = Default::default();
        w.edited = [false; 3];
        w.focus = None;
        w.probing = Some(download::probe(&url));
        w.opened_at = Instant::now();
        // Under the notch, where the Claude popup opens.
        let pos = (self.popup.pos.0 + (((PP_W - DW_W) / 2.0) * k) as i32, self.popup.pos.1);
        let (pw, ph) = ((DW_W * k).round() as i32, (DW_H * k).round() as i32);
        unsafe {
            let _ = SetWindowPos(w.hwnd, HWND_TOPMOST, pos.0, pos.1, pw, ph, SWP_NOACTIVATE);
        }
        let hwnd = w.hwnd;
        match w.surface {
            None => w.surface = self.gfx.add_surface(hwnd, DW_W, DW_H, pos, false).ok(),
            Some(s) => self.gfx.move_surface(s, pos),
        }
        self.dl_win.shown = true;
        self.draw_dl();
        unsafe {
            let _ = ShowWindow(self.dl_win.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.dl_win.hwnd);
        }
    }

    pub fn close_dl(&mut self) {
        self.dl_win.shown = false;
        self.dl_win.probing = None;
        unsafe {
            let _ = ShowWindow(self.dl_win.hwnd, SW_HIDE);
        }
    }

    /// The video's info as it comes (filling the boxes not typed in), and the caret's blink.
    pub(super) fn tick_dl(&mut self) {
        if !self.dl_win.shown {
            return;
        }
        let mut changed = false;
        if let Some(rx) = &self.dl_win.probing {
            if let Ok(got) = rx.try_recv() {
                super::trace(&format!("[download] details: {:?}", got.as_ref().map(|i| (&i.title, &i.heights))));
                let w = &mut self.dl_win;
                w.probing = None;
                match got {
                    Some(info) => {
                        for (k, v) in [info.title.clone(), info.artist.clone(), info.album.clone()].into_iter().enumerate() {
                            if !w.edited[k] {
                                w.fields[k] = v;
                            }
                        }
                        // A quality the video doesn't have goes back to the best.
                        if !w.opts.audio && w.opts.quality.is_some_and(|q| !info.heights.contains(&q)) {
                            w.opts.quality = None;
                        }
                        w.info = Some(info);
                    }
                    None => w.probe_failed = true,
                }
                changed = true;
            }
        }
        let now = Instant::now();
        if changed {
            self.dl_win.redraw_until = Some(now + std::time::Duration::from_millis(600));
        }
        if changed || self.dl_win.focus.is_some() || self.dl_win.redraw_until.is_some_and(|t| now < t) {
            self.draw_dl();
        }
    }

    /// The choices a drop-down offers, and which is chosen.
    pub fn dl_menu(&self, which: u8) -> Vec<(String, bool)> {
        let o = &self.dl_win.opts;
        match which {
            0 => vec![("Video".to_owned(), !o.audio), ("Audio only".to_owned(), o.audio)],
            1 => {
                let list: &[&str] = if o.audio { &download::AUDIO_FORMATS } else { &download::VIDEO_FORMATS };
                list.iter().map(|f| (f.to_uppercase(), *f == o.format)).collect()
            }
            _ => self.dl_qualities().into_iter().map(|q| (quality_text(o.audio, q), q == o.quality)).collect(),
        }
    }

    /// The qualities to offer: the best, then the video's own heights (or the bitrates).
    fn dl_qualities(&self) -> Vec<Option<u32>> {
        let o = &self.dl_win.opts;
        let mut out = vec![None];
        if o.audio {
            if o.format != "wav" {
                out.extend(download::BITRATES.iter().map(|&k| Some(k)));
            }
        } else {
            let have = self.dl_win.info.as_ref().map(|i| i.heights.clone()).unwrap_or_default();
            out.extend(download::HEIGHTS.iter().filter(|h| have.is_empty() || have.contains(h)).map(|&h| Some(h)));
        }
        out
    }

    /// A choice made in a drop-down.
    pub fn dl_pick(&mut self, which: u8, i: usize) {
        let qualities = self.dl_qualities();
        let o = &mut self.dl_win.opts;
        match which {
            0 => {
                let audio = i == 1;
                if audio != o.audio {
                    o.audio = audio;
                    o.format = if audio { "mp3" } else { "mp4" };
                    o.quality = None;
                }
            }
            1 => {
                let list: &[&'static str] = if o.audio { &download::AUDIO_FORMATS } else { &download::VIDEO_FORMATS };
                if let Some(f) = list.get(i) {
                    o.format = f;
                    if *f == "wav" {
                        o.quality = None;
                    }
                }
            }
            _ => {
                if let Some(q) = qualities.get(i) {
                    o.quality = *q;
                }
            }
        }
        self.draw_dl();
    }

    pub fn dl_hover(&mut self, x: f32, y: f32) {
        if self.dl_win.cursor != (x, y) {
            self.dl_win.cursor = (x, y);
            self.draw_dl();
        }
    }

    pub fn dl_down(&mut self, x: f32, y: f32) {
        self.dl_win.pressed = self.dl_win.at(x, y);
        self.draw_dl();
    }

    pub fn dl_up(&mut self, x: f32, y: f32) -> Option<After> {
        let pressed = self.dl_win.pressed.take();
        let d = pressed.filter(|d| self.dl_win.at(x, y) == Some(*d));
        let after = match d {
            Some(Do::Close) => {
                self.close_dl();
                None
            }
            Some(Do::Menu(k)) => {
                super::trace(&format!("[download] menu {k}"));
                Some(After::Menu(k))
            }
            Some(Do::Field(k)) => {
                self.dl_win.focus = Some(k);
                None
            }
            Some(Do::Download) => {
                self.dl_go();
                None
            }
            None => {
                self.dl_win.focus = None;
                None
            }
        };
        if self.dl_win.shown {
            self.draw_dl();
        }
        after
    }

    /// Typing: into the box in focus; Tab moves on, Enter downloads, Esc closes.
    pub fn dl_char(&mut self, c: char) {
        let w = &mut self.dl_win;
        match c {
            '\u{1b}' => {
                if w.focus.is_some() {
                    w.focus = None;
                } else {
                    self.close_dl();
                    return;
                }
            }
            '\r' => {
                self.dl_go();
                return;
            }
            '\t' => w.focus = Some(w.focus.map_or(0, |k| (k + 1) % 3)),
            _ => {
                let Some(k) = w.focus else { return };
                match c {
                    '\u{8}' => {
                        w.fields[k].pop();
                    }
                    '\u{16}' => {
                        if let Some(t) = super::toasts::clipboard_text(w.hwnd) {
                            w.fields[k].push_str(t.trim());
                        }
                    }
                    '\u{1}' => w.fields[k].clear(),
                    c if !c.is_control() && w.fields[k].chars().count() < 200 => w.fields[k].push(c),
                    _ => return,
                }
                w.edited[k] = true;
            }
        }
        self.draw_dl();
    }

    /// Starts the download with what's chosen, and closes the window.
    fn dl_go(&mut self) {
        let Some(url) = self.dl_url.clone() else { return };
        let w = &self.dl_win;
        let some = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
        let mut opts = w.opts.clone();
        // The name only when it's not just the title (yt-dlp names it after the title anyway).
        let title = w.info.as_ref().map_or("", |i| i.title.as_str());
        opts.name = some(&w.fields[0]).filter(|n| n != title.trim());
        opts.artist = some(&w.fields[1]);
        opts.album = some(&w.fields[2]);
        self.dl_kind = if opts.audio { Kind::Mp3 } else { Kind::Mp4 };
        self.dl_format = opts.format.to_uppercase();
        super::trace(&format!("[download] {} {} {:?}", opts.format, quality_text(opts.audio, opts.quality), opts.name));
        self.dl_job = Some(download::start_with(&url, &opts));
        self.close_dl();
    }

    pub fn dl_moved(&mut self) {
        let mut r = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = GetWindowRect(self.dl_win.hwnd, &mut r);
        }
        if let Some(s) = self.dl_win.surface {
            self.gfx.move_surface(s, (r.left, r.top));
        }
    }

    /// The title bar drags the window.
    pub fn dl_drags(&self, x: f32, y: f32) -> bool {
        y < 56.0 && self.dl_win.at(x, y).is_none()
    }

    pub fn draw_dl(&mut self) {
        let Some(surface) = self.dl_win.surface else { return };
        if !self.dl_win.shown || self.gfx.begin(surface).is_err() {
            return;
        }
        self.dl_win.regions.borrow_mut().clear();
        let w = &self.dl_win;
        let g = &self.gfx;
        let hit = |r: Rect, d: Do| w.regions.borrow_mut().push((r, d));
        let hov = |r: Rect| r.contains(w.cursor.0, w.cursor.1);
        let down = |d: Do| w.pressed == Some(d);
        let pane = Rect::new(1.0, 1.0, DW_W - 2.0, DW_H - 2.0);
        g.fill_rrect(pane, 18.0, rgb(0x12, 0x12, 0x18));
        g.stroke_rrect(pane, 18.0, 1.0, white(0.1), false);
        g.radial(DW_W - 40.0, 0.0, 220.0, 120.0, rgba(0xa8, 0x7b, 0xff, 0.14));
        // Title, the video's name, and close.
        g.icon('\u{E896}', 26.0, 26.0, 14.0, VIOLET);
        g.text("Download options", 44.0, 26.0, 240.0, 14.0, 700, WHITE, Align::Start);
        let close = Rect::centered(DW_W - 28.0, 26.0, 26.0, 26.0);
        if hov(close) {
            g.fill_circle(close.x + 13.0, 26.0, 13.0, white(0.12));
        }
        g.icon('\u{E711}', close.x + 13.0, 26.5, 9.0, white(if hov(close) { 1.0 } else { 0.6 }));
        hit(close, Do::Close);
        let sub = match (&w.info, w.probing.is_some(), w.probe_failed) {
            (Some(i), _, _) => i.title.clone(),
            (None, true, _) => "Reading the video\u{2026}".to_owned(),
            _ => "Couldn't read the video's details; you can still choose".to_owned(),
        };
        g.text(&sub, 20.0, 48.0, DW_W - 40.0, 11.0, 500, white(0.55), Align::Start);
        // The rows.
        let o = &w.opts;
        let formats = o.format.to_uppercase();
        let quality = if o.audio && o.format == "wav" { "Lossless".to_owned() } else { quality_text(o.audio, o.quality) };
        let rows: [(&str, Option<String>, Option<usize>); 6] = [
            ("Type", Some(if o.audio { "Audio only".to_owned() } else { "Video".to_owned() }), None),
            ("Format", Some(formats), None),
            ("Quality", Some(quality), None),
            ("File name", None, Some(0)),
            ("Artist", None, Some(1)),
            ("Album", None, Some(2)),
        ];
        for (i, (label, value, field)) in rows.iter().enumerate() {
            let cy = TOP + i as f32 * ROW + ROW / 2.0;
            g.text(label, 20.0, cy, LABEL_W - 8.0, 12.0, 500, white(0.75), Align::Start);
            let b = Rect::new(20.0 + LABEL_W, cy - 14.0, DW_W - 40.0 - LABEL_W, 28.0);
            match (value, field) {
                (Some(v), _) => {
                    // A drop-down: the choice and a chevron.
                    let d = Do::Menu(i as u8);
                    let locked = i == 2 && o.audio && o.format == "wav";
                    g.fill_rrect(b, 8.0, white(if down(d) { 0.16 } else if hov(b) && !locked { 0.12 } else { 0.07 }));
                    g.text(v, b.x + 12.0, cy, b.w - 40.0, 12.0, 600, white(if locked { 0.45 } else { 0.95 }), Align::Start);
                    if !locked {
                        g.icon('\u{E70D}', b.x + b.w - 16.0, cy + 0.5, 9.0, white(0.6));
                        hit(b, d);
                    }
                }
                (None, Some(k)) => {
                    let focused = w.focus == Some(*k);
                    g.fill_rrect(b, 8.0, white(if focused { 0.1 } else { 0.06 }));
                    if focused {
                        g.stroke_rrect(b, 8.0, 1.2, rgba(0xa8, 0x7b, 0xff, 0.8), false);
                    }
                    let text = &w.fields[*k];
                    let hint = ["Named after the video", "Not set", "Not set"][*k];
                    // (The end of a long text shows, where the typing is.)
                    let mut s = if text.is_empty() { hint.to_owned() } else { text.clone() };
                    let full = s.chars().count();
                    while g.measure(&s, 12.0, 500) > b.w - 26.0 && s.chars().count() > 1 {
                        s.remove(0);
                    }
                    if s.chars().count() < full {
                        s.insert(0, '\u{2026}');
                    }
                    g.text(&s, b.x + 12.0, cy, b.w - 24.0, 12.0, 500, white(if text.is_empty() { 0.35 } else { 0.95 }), Align::Start);
                    if focused && (w.opened_at.elapsed().as_millis() / 500) % 2 == 0 {
                        let cx = b.x + 12.0 + if text.is_empty() { 0.0 } else { g.measure(&s, 12.0, 500) };
                        g.fill_rrect(Rect::new(cx + 1.0, cy - 8.0, 1.5, 16.0), 0.5, VIOLET);
                    }
                    hit(b, Do::Field(*k));
                }
                _ => {}
            }
        }
        // Cancel and Download.
        let by = DW_H - 44.0;
        let go = Rect::new(DW_W - 20.0 - 120.0, by, 120.0, 30.0);
        let cancel = Rect::new(go.x - 10.0 - 90.0, by, 90.0, 30.0);
        g.fill_rrect(cancel, 9.0, white(if hov(cancel) { 0.14 } else { 0.08 }));
        g.text("Cancel", cancel.x, by + 15.0, cancel.w, 12.5, 600, WHITE, Align::Center);
        hit(cancel, Do::Close);
        g.fill_rrect(go, 9.0, if down(Do::Download) { rgb(0x15, 0x5c, 0xb0) } else if hov(go) { rgb(0x26, 0x84, 0xe8) } else { rgb(0x1d, 0x74, 0xd8) });
        g.icon('\u{E896}', go.x + 22.0, by + 15.0, 12.0, WHITE);
        g.text("Download", go.x + 32.0, by + 15.0, go.w - 40.0, 12.5, 600, WHITE, Align::Center);
        hit(go, Do::Download);
        g.text("Enter downloads", 20.0, by + 15.0, cancel.x - 30.0, 10.0, 400, white(0.35), Align::Start);
        // A skipped frame (the screen wasn't ready) is drawn again next time round.
        if self.gfx.present().is_err() {
            self.dl_win.redraw_until = Some(Instant::now() + std::time::Duration::from_millis(100));
        }
    }
}
