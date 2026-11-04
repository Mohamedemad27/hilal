//! Toasts, as Nova Island has them: a small pill for a few seconds with an icon, what
//! happened and a value (a bar for the volume, the brightness and the battery), some with
//! one click to act on. What they tell: a keyboard language or Caps Lock change, the volume
//! and brightness, the battery, the internet, an app starting to use the microphone or
//! camera (which also lights a dot on the notch), a copied screenshot or image (saved and
//! put on the Shelf for a while), and what the smart clipboard makes of copied text.
//! All of it is Nova mode only.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{App, Shape, AMBER, GREEN, RED};
use crate::gfx::{rgb, white, Align, Color, Image, Rect, WHITE};
use crate::watch::{self, Event, Link};

pub const TOAST: Shape = Shape { w: 340.0, h: 34.0, r: 17.0, e: 12.0 };
pub const V_TOAST: Shape = Shape { w: 44.0, h: 168.0, r: 22.0, e: 0.0 };

const MIC: Color = rgb(0xff, 0x9f, 0x0a);
const CAM: Color = rgb(0x30, 0xd1, 0x58);

/// What a click on a toast does.
#[derive(Clone)]
pub enum ToastAct {
    OpenShelf,
    SaveShot,
    OpenUrl(String),
    Copy(String),
    /// Shows a file in Explorer.
    Reveal(PathBuf),
    /// Installs a downloaded update.
    Update(PathBuf),
}

pub struct Toast {
    pub icon: char,
    pub color: Color,
    pub label: String,
    pub value: String,
    /// A bar (0..1), for the volume, brightness and battery.
    pub bar: Option<f32>,
    /// A swatch (a copied color) or a thumbnail (a copied image).
    pub swatch: Option<Color>,
    pub thumb: Option<Image>,
    pub at: Instant,
    pub until: Instant,
    pub action: Option<ToastAct>,
}

/// What the toasts watch, between frames.
pub struct Watched {
    pub watch: watch::Watch,
    pub mic: Vec<String>,
    pub cam: Vec<String>,
    pub link: Link,
    pub internet: Option<bool>,
    down_at: Option<Instant>,
    keyboard: Option<(u32, u32)>,
    caps: Option<bool>,
    low_shown: bool,
    /// The clipboard's sequence number when last read (None until the first, which is
    /// what was there before; not news).
    clip_seq: Option<u32>,
    /// Hilal's own copy (the smart clipboard's result): not news either.
    own_copy: Option<u32>,
    /// The last copied image, until it's saved or replaced.
    shot: Option<(Vec<u8>, u32, u32)>,
    shot_hash: Option<u64>,
}

impl Watched {
    pub fn new(on: bool) -> Self {
        Self {
            watch: watch::Watch::start(on),
            mic: Vec::new(),
            cam: Vec::new(),
            link: Link::None,
            internet: None,
            down_at: None,
            keyboard: None,
            caps: None,
            low_shown: false,
            clip_seq: None,
            own_copy: None,
            shot: None,
            shot_hash: None,
        }
    }
}

impl App {
    /// Shows a toast for `ms` (replacing any other).
    pub(super) fn toast(&mut self, icon: char, color: Color, label: impl Into<String>, value: impl Into<String>, bar: Option<f32>, ms: u64) {
        let now = Instant::now();
        // The same kind again (the volume while it's being turned) keeps its place.
        let at = self.toast.as_ref().filter(|t| t.icon == icon && t.until > now).map_or(now, |t| t.at);
        self.toast = Some(Toast {
            icon,
            color,
            label: label.into(),
            value: value.into(),
            bar,
            swatch: None,
            thumb: None,
            at,
            until: now + Duration::from_millis(ms),
            action: None,
        });
    }

    /// A toast is up (they show on the closed notch only).
    pub(super) fn toast_shown(&self) -> bool {
        !self.open && self.toast.as_ref().is_some_and(|t| t.until > Instant::now())
    }

    /// Reads what's being watched, and drops a toast whose time is up.
    pub(super) fn tick_toasts(&mut self) {
        let nova = self.nova();
        self.watched.watch.set_on(nova);
        if self.toast.as_ref().is_some_and(|t| t.until <= Instant::now()) || (!nova && self.toast.is_some()) {
            self.toast = None;
        }
        while let Ok(e) = self.watched.watch.events.try_recv() {
            if nova {
                self.on_watch(e);
            }
        }
        if !nova {
            self.watched.mic.clear();
            self.watched.cam.clear();
            return;
        }
        // The keyboard's language and Caps Lock (with per-app layouts, a switch of windows
        // changes the layout too: only a change within the same window counts).
        let f = &self.store.features;
        let (kbd_on, caps_on) = (f.kbd_toast, f.caps_toast);
        let (kl, pid) = watch::keyboard_layout();
        if kl > 0 {
            if let Some((old, old_pid)) = self.watched.keyboard {
                if kl != old && pid == old_pid && kbd_on {
                    let (name, badge) = watch::language(kl);
                    self.toast('\u{E765}', WHITE, name, badge, None, 1400);
                }
            }
            self.watched.keyboard = Some((kl, pid));
        }
        let caps = watch::caps_lock();
        if self.watched.caps.is_some_and(|c| c != caps) && caps_on {
            self.toast('\u{E84A}', if caps { AMBER } else { WHITE }, "Caps Lock", if caps { "On" } else { "Off" }, None, 1400);
        }
        self.watched.caps = Some(caps);
        // Screenshots left on the Shelf for a while go when their time is up.
        let now = crate::store::unix_now();
        if self.store.shelf_until.iter().any(|(_, t)| *t <= now) {
            let gone: Vec<PathBuf> = self.store.shelf_until.iter().filter(|(_, t)| *t <= now).map(|(p, _)| p.clone()).collect();
            self.store.shelf.retain(|p| !gone.contains(p));
            self.store.shelf_until.retain(|(_, t)| *t > now);
            self.store.save();
        }
    }

    fn on_watch(&mut self, e: Event) {
        let f = &self.store.features;
        let (privacy, net, battery, vol, bright) = (f.privacy_dots, f.net_toasts, f.battery, f.hud_volume, f.hud_brightness);
        match e {
            Event::Privacy { mic, cam } => {
                // A toast only for apps that just started.
                let new_cam: Vec<String> = cam.iter().filter(|a| !self.watched.cam.contains(a)).cloned().collect();
                let new_mic: Vec<String> = mic.iter().filter(|a| !self.watched.mic.contains(a)).cloned().collect();
                if privacy {
                    if !new_cam.is_empty() {
                        self.toast('\u{E714}', CAM, format!("{} is using the camera", new_cam.join(", ")), "", None, 3200);
                    } else if !new_mic.is_empty() {
                        self.toast('\u{E720}', MIC, format!("{} is using the microphone", new_mic.join(", ")), "", None, 3200);
                    }
                }
                self.watched.mic = mic;
                self.watched.cam = cam;
                self.closed_sig.clear();
            }
            Event::Network { link, internet, prev } => {
                self.watched.link = link.clone();
                self.watched.internet = internet;
                // The first reading after start is not news.
                if prev.1.is_none() || !net {
                    return;
                }
                if prev.1 == Some(true) && internet == Some(false) {
                    self.watched.down_at = Some(Instant::now());
                    self.happened("Internet lost");
                    self.toast('\u{EB5E}', RED, if link == Link::None { "No connection" } else { "Internet lost" }, "", None, 4500);
                } else if prev.1 == Some(false) && internet == Some(true) {
                    let gone = self.watched.down_at.take().map(|t| t.elapsed());
                    let value = match gone {
                        Some(d) => {
                            let m = (d.as_secs_f32() / 60.0).round() as u64;
                            if m < 1 { "after a few seconds".to_owned() } else if m < 60 { format!("after {m} min") } else { format!("after {} h {} min", m / 60, m % 60) }
                        }
                        None => link_name(&link).to_owned(),
                    };
                    self.happened(format!("Back online {value}"));
                    self.toast('\u{E701}', GREEN, "Back online", value, None, 3500);
                } else if link == Link::Wifi && prev.0 != Link::Wifi && internet == Some(true) {
                    self.toast('\u{E701}', GREEN, "Connected to Wi-Fi", "", None, 3000);
                }
            }
            Event::Battery { charging, level } => {
                self.check_saver(charging, level);
                if !battery {
                    return;
                }
                let pct = format!("{}%", (level * 100.0).round());
                if self.watched.low_shown && level > 0.2 {
                    self.watched.low_shown = false;
                }
                if charging != self.battery_charging {
                    self.battery_charging = charging;
                    if charging {
                        self.toast('\u{EA93}', GREEN, "Charging", pct, Some(level), 3200);
                    } else {
                        let c = if level <= 0.2 { RED } else { WHITE };
                        self.toast('\u{E83F}', c, "On Battery", pct, Some(level), 3200);
                    }
                } else if !charging && level <= 0.2 && !self.watched.low_shown {
                    self.watched.low_shown = true;
                    self.toast('\u{E850}', RED, "Low Battery", pct, Some(level), 4500);
                }
            }
            Event::Volume { level, muted } => {
                if vol && !self.open {
                    let icon = if muted || level == 0.0 { '\u{E74F}' } else if level < 0.34 { '\u{E993}' } else if level < 0.67 { '\u{E994}' } else { '\u{E995}' };
                    let value = if muted { "Muted".to_owned() } else { format!("{}%", (level * 100.0).round()) };
                    self.toast(icon, WHITE, "", value, Some(if muted { 0.0 } else { level }), 1600);
                }
            }
            Event::Brightness(b) => {
                if bright && !self.open {
                    self.toast('\u{E706}', WHITE, "", format!("{b}%"), Some(b as f32 / 100.0), 1600);
                }
            }
        }
    }

    /// The clipboard changed (WM_CLIPBOARDUPDATE): a copied image gives a screenshot toast,
    /// copied text the smart clipboard's.
    pub fn clipboard_changed(&mut self) {
        use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
        let seq = unsafe { GetClipboardSequenceNumber() };
        let first = self.watched.clip_seq.is_none();
        if self.watched.clip_seq == Some(seq) {
            return;
        }
        self.watched.clip_seq = Some(seq);
        if first || !self.nova() || self.watched.own_copy == Some(seq) {
            return;
        }
        let Some(clip) = read_clipboard(self.hwnd) else { return };
        match clip {
            Clip::Private => {}
            Clip::Text(text) => {
                self.remember_clip(&text);
                if self.store.features.smart_clipboard && self.reminder.is_none() {
                    self.smart_clip(&text);
                }
            }
            Clip::Image(png, w, h) => {
                // (Some apps put one image on the clipboard in two steps: it's news once.)
                let hash = {
                    use std::hash::{Hash, Hasher};
                    let mut hs = std::collections::hash_map::DefaultHasher::new();
                    png.hash(&mut hs);
                    hs.finish()
                };
                if self.watched.shot_hash == Some(hash) {
                    return;
                }
                self.watched.shot_hash = Some(hash);
                let f = &self.store.features;
                if !f.screenshot_toast {
                    return;
                }
                let auto = f.shot_auto_shelf;
                let thumb = image::load_from_memory(&png).ok().and_then(|img| {
                    let t = img.thumbnail(144, 72).to_rgba8();
                    let (tw, th) = t.dimensions();
                    let bgra: Vec<u8> = t.pixels().flat_map(|p| {
                        let [r, g, b, a] = p.0;
                        let k = a as f32 / 255.0;
                        [(b as f32 * k) as u8, (g as f32 * k) as u8, (r as f32 * k) as u8, a]
                    }).collect();
                    self.gfx.image(&bgra, tw, th)
                });
                self.watched.shot = Some((png, w, h));
                let portrait = h > w;
                if auto {
                    let minutes = self.store.features.shot_shelf_minutes as u64;
                    self.shot_to_shelf(Some(minutes));
                    self.toast('\u{E722}', WHITE, if portrait { "Image saved" } else { "Screenshot saved" }, "Added to Shelf \u{2713}", None, 4500);
                    if let Some(t) = &mut self.toast {
                        t.action = Some(ToastAct::OpenShelf);
                    }
                } else {
                    self.toast('\u{E722}', WHITE, if portrait { "Image copied" } else { "Screenshot copied" }, "Add to Shelf", None, 6000);
                    if let Some(t) = &mut self.toast {
                        t.action = Some(ToastAct::SaveShot);
                    }
                }
                if let Some(t) = &mut self.toast {
                    t.thumb = thumb;
                }
            }
        }
    }

    /// Saves the last copied image to Pictures\Hilal and puts it on the Shelf (for a while,
    /// if `minutes`). The Snipping Tool saves its own copy to Pictures\Screenshots: that file
    /// goes on the Shelf instead of a second one.
    fn shot_to_shelf(&mut self, minutes: Option<u64>) {
        let Some((png, _, _)) = self.watched.shot.take() else { return };
        let pictures = crate::win::known_folder_pictures();
        let copied = std::time::SystemTime::now();
        let snip = minutes.and_then(|_| {
            let dir = pictures.as_ref()?.join("Screenshots");
            std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| {
                let p = e.path();
                let ext = p.extension()?.to_str()?.to_ascii_lowercase();
                let m = e.metadata().ok()?.modified().ok()?;
                (matches!(ext.as_str(), "png" | "jpg" | "jpeg") && copied.duration_since(m).map_or(true, |d| d < Duration::from_secs(4))).then_some((m, p))
            }).max_by_key(|(m, _)| *m).map(|(_, p)| p)
        });
        let file = match snip {
            Some(p) => p,
            None => {
                let Some(dir) = pictures.map(|p| p.join("Hilal")) else { return };
                let _ = std::fs::create_dir_all(&dir);
                let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
                let p = dir.join(format!("Screenshot {}-{:02}-{:02} {:02}{:02}{:02}.png", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond));
                if std::fs::write(&p, &png).is_err() {
                    return;
                }
                p
            }
        };
        if !self.store.shelf.contains(&file) {
            self.store.shelf.push(file.clone());
        }
        if let Some(m) = minutes {
            self.store.shelf_until.retain(|(p, _)| *p != file);
            self.store.shelf_until.push((file.clone(), crate::store::unix_now() + m * 60));
        } else {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.toast('\u{E722}', WHITE, name, "On the Shelf", None, 2600);
        }
        self.store.save();
    }

    /// What the smart clipboard makes of copied text: a link to open, a color, a sum to
    /// work out (a click copies the result), or a long text's word count.
    fn smart_clip(&mut self, text: &str) {
        let t = text.trim();
        if t.is_empty() || t.len() > 4000 {
            return;
        }
        if (t.starts_with("http://") || t.starts_with("https://")) && !t.contains(char::is_whitespace) && t.len() <= 2000 {
            let host = t.split("://").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("").trim_start_matches("www.").to_owned();
            self.toast('\u{E71B}', rgb(0x8a, 0xb4, 0xff), host, "Open", None, 5000);
            if let Some(toast) = &mut self.toast {
                toast.action = Some(ToastAct::OpenUrl(t.to_owned()));
            }
            return;
        }
        if let Some(c) = parse_color(t) {
            self.toast('\u{E790}', WHITE, t, "", None, 3000);
            if let Some(toast) = &mut self.toast {
                toast.swatch = Some(c);
            }
            return;
        }
        if let Some(result) = math(t) {
            self.toast('\u{E8EF}', rgb(0xfb, 0xbf, 0x24), format!("{t} ="), result.clone(), None, 6000);
            if let Some(toast) = &mut self.toast {
                toast.action = Some(ToastAct::Copy(result));
            }
            return;
        }
        let words = t.split_whitespace().count();
        if words >= 30 {
            self.toast('\u{E8D2}', white(0.7), format!("{} words", thousands(words)), format!("{} chars", thousands(t.chars().count())), None, 2600);
        }
    }

    /// Copied text, for the clipboard history on the System page (kept in memory only).
    fn remember_clip(&mut self, text: &str) {
        if !self.store.features.clipboard_history {
            return;
        }
        let t = text.trim();
        if t.is_empty() || t.len() > 20_000 {
            return;
        }
        self.clip_history.retain(|c| c != t);
        self.clip_history.push_front(t.to_owned());
        self.clip_history.truncate(20);
    }

    /// A click on a toast with something to do.
    pub(super) fn toast_click(&mut self) {
        let Some(act) = self.toast.as_mut().and_then(|t| t.action.take()) else { return };
        self.toast = None;
        match act {
            ToastAct::OpenShelf => {
                self.show_page(super::Page::Shelf);
                self.alert_until = Some(Instant::now() + Duration::from_secs(4));
            }
            ToastAct::SaveShot => self.shot_to_shelf(None),
            ToastAct::OpenUrl(url) => crate::win::open_url(&url),
            ToastAct::Update(p) => self.start_update(p),
            ToastAct::Reveal(p) => {
                let _ = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
            }
            ToastAct::Copy(text) => {
                if set_clipboard_text(self.hwnd, &text) {
                    self.watched.own_copy = Some(unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() });
                }
                self.toast('\u{E73E}', GREEN, "Copied", text, None, 1400);
            }
        }
    }

    /// The toast, on the closed notch: icon, what, value (or a bar); standing on a side, the
    /// icon, a bar or the value, down its length.
    pub(super) fn toast_view(&self, s: Shape, a: f32) {
        let Some(t) = &self.toast else { return };
        let g = &self.gfx;
        g.fade.set(a);
        let b = self.shape_rect(s);
        let x0 = b.x;
        if self.pos().vertical {
            let cx = b.x + b.w / 2.0;
            g.icon(t.icon, cx, 26.0, 15.0, t.color);
            match t.bar {
                Some(k) => {
                    let bar = Rect::new(cx - 3.0, 44.0, 6.0, 84.0);
                    g.fill_rrect(bar, 3.0, white(0.18));
                    let h = 84.0 * k.clamp(0.0, 1.0);
                    if h > 0.0 {
                        g.fill_rrect(Rect::new(bar.x, bar.y + 84.0 - h, 6.0, h), 3.0, t.color);
                    }
                }
                None => {
                    if let Some(c) = t.swatch {
                        g.fill_circle(cx, 86.0, 9.0, c);
                    }
                }
            }
            let v = if t.value.is_empty() { "\u{2713}" } else { t.value.as_str() };
            g.display(v, b.x + 2.0, 146.0, b.w - 4.0, 10.5, 600, WHITE, Align::Center);
            return;
        }
        let cy = s.h / 2.0;
        let lx;
        if let Some(img) = &t.thumb {
            let r = Rect::new(x0 + 6.0, 4.0, 40.0, s.h - 8.0);
            g.draw_image(img, r, 6.0, 1.0);
            lx = x0 + 54.0;
        } else if let Some(c) = t.swatch {
            g.fill_circle(x0 + 24.0, cy, 10.0, c);
            g.stroke_circle(x0 + 24.0, cy, 10.0, 1.0, white(0.3));
            lx = x0 + 42.0;
        } else {
            g.icon(t.icon, x0 + 24.0, cy, 14.0, t.color);
            lx = x0 + 42.0;
        }
        let right = x0 + s.w - 16.0;
        let vw = if t.value.is_empty() { 0.0 } else { g.measure(&t.value, 12.0, 600) + 4.0 };
        let vc = if t.action.is_some() { rgb(0x9c, 0xc3, 0xff) } else { white(0.75) };
        match t.bar {
            Some(k) if t.label.is_empty() => {
                // The volume and brightness: a bar across.
                let bar = Rect::new(lx, cy - 2.5, right - vw - 10.0 - lx, 5.0);
                g.fill_rrect(bar, 2.5, white(0.18));
                g.fill_rrect(Rect::new(bar.x, bar.y, bar.w * k.clamp(0.0, 1.0), 5.0), 2.5, WHITE);
            }
            Some(k) => {
                // The battery: a small gauge before the value.
                let gw = 22.0;
                let gr = Rect::new(right - vw - gw - 6.0, cy - 5.0, gw, 10.0);
                g.stroke_rrect(gr, 3.0, 1.2, white(0.6), false);
                g.fill_rrect(Rect::new(gr.x + 2.0, gr.y + 2.0, (gw - 4.0) * k.clamp(0.0, 1.0), 6.0), 1.5, t.color);
                g.text(&t.label, lx, cy, gr.x - 8.0 - lx, 12.5, 600, WHITE, Align::Start);
            }
            None => {
                g.text(&t.label, lx, cy, right - vw - 8.0 - lx, 12.5, 600, WHITE, Align::Start);
            }
        }
        if vw > 0.0 {
            g.text(&t.value, right - vw, cy, vw, 12.0, 600, vc, Align::End);
        }
        if t.action.is_some() {
            self.hit(Rect::new(x0, 0.0, s.w, s.h), super::Action::ToastClick);
        }
    }

    /// The Clipboard page (Nova mode): the connection and who's using the mic and camera on
    /// the left, the clipboard history on the right (a click copies an item back).
    pub(super) fn clipboard_page(&self) {
        let g = &self.gfx;
        self.header("Clipboard", None);
        g.text("Connection", 44.0, 41.0, 150.0, 10.5, 500, white(0.55), Align::Start);
        let net = self.network_line();
        let off = self.watched.link == Link::None || self.watched.internet == Some(false);
        g.text(&net, 44.0, 57.0, 152.0, 13.0, 600, if off { rgb(0xff, 0x8a, 0x80) } else { WHITE }, Align::Start);
        g.text("Mic & camera", 44.0, 81.0, 150.0, 10.5, 500, white(0.55), Align::Start);
        match self.privacy_line() {
            Some(line) => {
                g.fill_circle(48.0, 98.0, 3.0, if self.watched.cam.is_empty() { MIC } else { CAM });
                g.paragraph(&line, 56.0, 90.0, 140.0, 11.0, 500, WHITE, 2);
            }
            None => {
                g.text("Not in use", 44.0, 97.0, 152.0, 11.5, 500, white(0.75), Align::Start);
            }
        }
        g.line(198.0, 34.0, 198.0, 120.0, 1.0, white(0.12));
        let x = 208.0;
        let w = 172.0;
        if !self.store.features.clipboard_history {
            g.text("Clipboard history is off", x, 76.0, w, 11.5, 500, white(0.5), Align::Center);
            return;
        }
        if self.clip_history.is_empty() {
            g.text("Copy something", x, 70.0, w, 12.0, 600, white(0.7), Align::Center);
            g.text("It shows here (kept in memory only)", x, 86.0, w, 10.0, 400, white(0.45), Align::Center);
            return;
        }
        for (slot, (i, text)) in self.clip_history.iter().enumerate().skip(self.clip_scroll).take(4).enumerate() {
            let r = Rect::new(x, 34.0 + slot as f32 * 22.0, w, 20.0);
            let lit = self.hovered(r);
            g.fill_rrect(r, 7.0, white(if lit { 0.12 } else { 0.06 }));
            let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            g.text(&one, r.x + 8.0, r.y + 10.0, r.w - 16.0, 10.5, 500, white(0.9), Align::Start);
            self.hit(r, super::Action::ClipCopy(i));
        }
    }

    /// A clipboard history item copied back.
    pub(super) fn clip_copy(&mut self, i: usize) {
        let Some(text) = self.clip_history.get(i).cloned() else { return };
        if set_clipboard_text(self.hwnd, &text) {
            self.watched.own_copy = Some(unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() });
            self.clip_history.remove(i);
            self.clip_history.push_front(text);
            self.clip_scroll = 0;
        }
    }

    /// The mic and camera dots, on the notch in any shape.
    pub(super) fn privacy_dots(&self, s: Shape) {
        if !self.nova() || !self.store.features.privacy_dots || (self.watched.mic.is_empty() && self.watched.cam.is_empty()) {
            return;
        }
        let g = &self.gfx;
        g.fade.set(1.0);
        let b = self.shape_rect(s);
        let dots: Vec<Color> = [(!self.watched.mic.is_empty(), MIC), (!self.watched.cam.is_empty(), CAM)].into_iter().filter(|d| d.0).map(|d| d.1).collect();
        let big = s.h > 50.0;
        let (mut x, y) = if self.pos().vertical {
            (b.x + b.w / 2.0 - (dots.len() as f32 - 1.0) * 5.0, 10.0)
        } else {
            (b.x + b.w - if big { 26.0 } else { 15.0 } - (dots.len() as f32 - 1.0) * 10.0, if big { 12.0 } else { 7.0 })
        };
        for c in dots {
            g.radial(x, y, 6.0, 6.0, [c[0], c[1], c[2], 0.5]);
            g.fill_circle(x, y, 3.0, c);
            x += 10.0;
        }
    }

    /// The apps using the microphone and camera now (for the System page).
    pub(super) fn privacy_line(&self) -> Option<String> {
        let w = &self.watched;
        let parts: Vec<String> = [(!w.cam.is_empty()).then(|| format!("Camera: {}", w.cam.join(", "))), (!w.mic.is_empty()).then(|| format!("Mic: {}", w.mic.join(", ")))].into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join(" \u{00b7} "))
    }

    /// The connection, for the System page.
    pub(super) fn network_line(&self) -> String {
        let w = &self.watched;
        let name = link_name(&w.link);
        if w.link != Link::None && w.internet == Some(false) { format!("{name} \u{00b7} no internet") } else { name.to_owned() }
    }
}

fn link_name(l: &Link) -> &'static str {
    match l {
        Link::Wifi => "Wi-Fi",
        Link::Ethernet => "Ethernet",
        Link::None => "Offline",
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "#rgb", "#rrggbb", "#rrggbbaa" or "rgb(…)": the color.
fn parse_color(t: &str) -> Option<Color> {
    let hex = |s: &str| u8::from_str_radix(s, 16).ok().map(|v| v as f32 / 255.0);
    if let Some(h) = t.strip_prefix('#') {
        if !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return match h.len() {
            3 | 4 => {
                let d: Vec<f32> = h.chars().map(|c| hex(&format!("{c}{c}")).unwrap_or(0.0)).collect();
                Some([d[0], d[1], d[2], *d.get(3).unwrap_or(&1.0)])
            }
            6 | 8 => Some([hex(&h[0..2])?, hex(&h[2..4])?, hex(&h[4..6])?, if h.len() == 8 { hex(&h[6..8])? } else { 1.0 }]),
            _ => None,
        };
    }
    let low = t.to_ascii_lowercase();
    let inner = low.strip_prefix("rgba(").or_else(|| low.strip_prefix("rgb("))?.strip_suffix(')')?;
    let parts: Vec<f32> = inner.split([',', ' ', '/']).filter(|p| !p.is_empty()).map(|p| p.trim_end_matches('%').parse::<f32>()).collect::<Result<_, _>>().ok()?;
    if parts.len() < 3 {
        return None;
    }
    Some([parts[0] / 255.0, parts[1] / 255.0, parts[2] / 255.0, parts.get(3).map_or(1.0, |a| if *a > 1.0 { a / 100.0 } else { *a })])
}

/// Arithmetic (numbers, + - * / ^ % and parentheses, never anything else) — but not dates
/// (12-10-2026, 29/10/2026) or dashed phone numbers (010-1234-5678).
fn math(t: &str) -> Option<String> {
    if t.len() > 80 || !t.chars().all(|c| c.is_ascii_digit() || " +-*/().,%^\u{00d7}\u{00f7}".contains(c)) {
        return None;
    }
    let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    // date or phone
    let parts: Vec<&str> = compact.split(['-', '/', '.']).collect();
    let seps: Vec<char> = compact.chars().filter(|c| "-/.".contains(*c)).collect();
    if parts.len() == 3 && seps.len() == 2 && seps[0] == seps[1] && parts.iter().all(|p| (1..=4).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let phone = compact.trim_start_matches('+');
    if phone.split('-').count() > 1 && phone.split('-').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    // at least one operator between a digit and a digit or parenthesis
    let c: Vec<char> = compact.chars().collect();
    let has_op = c.windows(3).any(|w| w[0].is_ascii_digit() && "+-*/\u{00d7}\u{00f7}^%".contains(w[1]) && (w[2].is_ascii_digit() || w[2] == '('))
        || c.windows(2).any(|w| w[0].is_ascii_digit() && w[1] == '%');
    if !has_op {
        return None;
    }
    let s: Vec<char> = compact.replace('\u{00d7}', "*").replace('\u{00f7}', "/").replace(',', "").chars().collect();
    let mut i = 0;
    let v = expr(&s, &mut i)?;
    if i != s.len() || !v.is_finite() {
        return None;
    }
    Some(if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { let t = format!("{:.8}", v); t.trim_end_matches('0').trim_end_matches('.').to_owned() })
}

fn number(s: &[char], i: &mut usize) -> Option<f64> {
    let start = *i;
    while *i < s.len() && (s[*i].is_ascii_digit() || s[*i] == '.') {
        *i += 1;
    }
    s[start..*i].iter().collect::<String>().parse().ok()
}

fn atom(s: &[char], i: &mut usize) -> Option<f64> {
    match s.get(*i) {
        Some('-') => {
            *i += 1;
            Some(-atom(s, i)?)
        }
        Some('+') => {
            *i += 1;
            atom(s, i)
        }
        Some('(') => {
            *i += 1;
            let v = expr(s, i)?;
            if s.get(*i) != Some(&')') {
                return None;
            }
            *i += 1;
            Some(v)
        }
        _ => {
            let mut v = number(s, i)?;
            if s.get(*i) == Some(&'%') {
                *i += 1;
                v /= 100.0;
            }
            Some(v)
        }
    }
}

fn power(s: &[char], i: &mut usize) -> Option<f64> {
    let b = atom(s, i)?;
    if s.get(*i) == Some(&'^') {
        *i += 1;
        return Some(b.powf(power(s, i)?));
    }
    Some(b)
}

fn term(s: &[char], i: &mut usize) -> Option<f64> {
    let mut v = power(s, i)?;
    while let Some(&op) = s.get(*i).filter(|c| **c == '*' || **c == '/') {
        *i += 1;
        let r = power(s, i)?;
        v = if op == '*' { v * r } else { v / r };
    }
    Some(v)
}

fn expr(s: &[char], i: &mut usize) -> Option<f64> {
    let mut v = term(s, i)?;
    while let Some(&op) = s.get(*i).filter(|c| **c == '+' || **c == '-') {
        *i += 1;
        let r = term(s, i)?;
        v = if op == '+' { v + r } else { v - r };
    }
    Some(v)
}

enum Clip {
    /// Marked private (a password manager's copy): not read.
    Private,
    Text(String),
    /// A PNG of the copied image, and its size.
    Image(Vec<u8>, u32, u32),
}

fn read_clipboard(hwnd: windows::Win32::Foundation::HWND) -> Option<Clip> {
    use windows::core::w;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    const CF_UNICODETEXT: u32 = 13;
    const CF_DIB: u32 = 8;
    unsafe {
        let private = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
        // The clipboard may be busy for a moment with the app that just wrote it.
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(hwnd).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if !opened {
            return None;
        }
        let out = (|| {
            if IsClipboardFormatAvailable(private).is_ok() {
                return Some(Clip::Private);
            }
            if IsClipboardFormatAvailable(CF_DIB).is_ok() {
                let h = GetClipboardData(CF_DIB).ok()?;
                let g = HGLOBAL(h.0);
                let size = GlobalSize(g);
                let p = GlobalLock(g) as *const u8;
                if p.is_null() || size < 40 {
                    return None;
                }
                let dib = std::slice::from_raw_parts(p, size).to_vec();
                let _ = GlobalUnlock(g);
                return dib_to_png(&dib).map(|(png, w, h)| Clip::Image(png, w, h));
            }
            if IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() {
                let h = GetClipboardData(CF_UNICODETEXT).ok()?;
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if p.is_null() {
                    return None;
                }
                let mut n = 0;
                while *p.add(n) != 0 && n < 1 << 20 {
                    n += 1;
                }
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                let _ = GlobalUnlock(g);
                return Some(Clip::Text(text));
            }
            None
        })();
        let _ = CloseClipboard();
        out
    }
}

/// A clipboard DIB as a PNG (with a file header in front it's a BMP the image crate reads).
fn dib_to_png(dib: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let header = u32::from_le_bytes(dib[0..4].try_into().ok()?) as usize;
    let bits = u16::from_le_bytes(dib[14..16].try_into().ok()?) as usize;
    let compression = u32::from_le_bytes(dib[16..20].try_into().ok()?);
    let colors = u32::from_le_bytes(dib[32..36].try_into().ok()?) as usize;
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let palette = if bits <= 8 { (if colors == 0 { 1 << bits } else { colors }) * 4 } else { colors * 4 };
    let offset = 14 + header + masks + palette;
    let mut bmp = Vec::with_capacity(dib.len() + 14);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((dib.len() + 14) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&(offset as u32).to_le_bytes());
    bmp.extend_from_slice(dib);
    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()?;
    // 32-bit clipboard images often leave alpha at 0: show them opaque.
    let mut rgba = img.to_rgba8();
    if rgba.pixels().all(|p| p.0[3] == 0) {
        for p in rgba.pixels_mut() {
            p.0[3] = 255;
        }
    }
    let (w, h) = rgba.dimensions();
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(rgba).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some((png, w, h))
}

/// The clipboard's text (for pasting into Settings), unless it's marked private.
pub(super) fn clipboard_text(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    match read_clipboard(hwnd)? {
        Clip::Text(t) => Some(t),
        _ => None,
    }
}

fn set_clipboard_text(hwnd: windows::Win32::Foundation::HWND, text: &str) -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    unsafe {
        if OpenClipboard(hwnd).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = (|| {
            let g = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let p = GlobalLock(g) as *mut u16;
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(g);
            SetClipboardData(13, HANDLE(g.0)).ok()
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}
