//! The Settings window (as Nova Island has one): a sidebar of pages, and on each, cards of
//! rows with a switch, a stepper, a choice of a few, or a button, for everything the tray
//! menu has and more. Drawn with the notch's own graphics, in a window of its own.

use std::cell::RefCell;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{crescent, App, AMBER};
use crate::gfx::{rgb, rgba, white, Align, Color, Rect, WHITE};
use crate::{sound, store, win};

pub const SW_W: f32 = 760.0;
pub const SW_H: f32 = 560.0;
const TITLE_H: f32 = 46.0;
const SIDE_W: f32 = 200.0;
const ROW_H: f32 = 46.0;
const BG: Color = rgb(0x10, 0x10, 0x16);
const SIDE: Color = rgb(0x0b, 0x0b, 0x10);
const PAGES: [&str; 2] = ["General", "Prayer & Dhikr"];

/// Something a click in the window does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Do {
    Close,
    Page(usize),
    StartWithWindows,
    Vertical(bool),
    Fullscreen,
    MaxMode(u8),
    ResetPosition,
    Glass,
    Minimal,
    Share,
    Overlay,
    PrayerAlerts,
    Hanafi,
    Hijri(i32),
    Adhan,
    Bells,
    Reminders,
    Every(i32),
    ShowFor(i32),
    Ayah,
    Dhikr,
    Dua,
    Quiet,
    QuietFrom(i32),
    QuietTo(i32),
    ShowOneNow,
}

/// What can't run while the app is borrowed (it opens a dialog of its own).
pub enum After {
    PickAdhan,
}

pub struct SettingsWin {
    pub hwnd: HWND,
    surface: Option<usize>,
    pub shown: bool,
    page: usize,
    scroll: f32,
    /// How tall the page's content is (for scrolling), as last drawn.
    content_h: f32,
    cursor: (f32, f32),
    pressed: Option<Do>,
    regions: RefCell<Vec<(Rect, Do)>>,
    last_draw: Instant,
    /// A note under the buttons for a moment ("Saved").
    saved_at: Option<Instant>,
}

impl SettingsWin {
    pub fn new() -> Self {
        Self {
            hwnd: HWND::default(),
            surface: None,
            shown: false,
            page: 0,
            scroll: 0.0,
            content_h: 0.0,
            cursor: (-1.0, -1.0),
            pressed: None,
            regions: RefCell::new(Vec::new()),
            last_draw: Instant::now(),
            saved_at: None,
        }
    }

    fn at(&self, x: f32, y: f32) -> Option<Do> {
        self.regions.borrow().iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, d)| *d)
    }
}

/// One row of a card.
enum Ctl {
    Switch(bool),
    /// The value shown, and whether it can go down and up; the actions for − and +.
    Stepper(String, bool, bool, Do, Do),
    /// The choices, which one is on, and what each does.
    Seg(Vec<(&'static str, Do)>, usize),
    Buttons(Vec<(&'static str, Do)>),
}

struct Row {
    label: &'static str,
    note: Option<String>,
    ctl: Ctl,
    /// What a click on the row (a switch) does.
    act: Option<Do>,
    sub: bool,
}

fn row(label: &'static str, ctl: Ctl) -> Row {
    let act = None;
    Row { label, note: None, ctl, act, sub: false }
}

fn switch(label: &'static str, on: bool, act: Do) -> Row {
    Row { label, note: None, ctl: Ctl::Switch(on), act: Some(act), sub: false }
}

impl Row {
    fn note(mut self, s: impl Into<String>) -> Self {
        self.note = Some(s.into());
        self
    }

    fn sub(mut self) -> Self {
        self.sub = true;
        self
    }
}

struct Group {
    title: Option<&'static str>,
    rows: Vec<Row>,
    /// A button under the card.
    action: Option<(&'static str, Do)>,
}

fn mins_text(v: u32) -> String {
    if v >= 60 && v % 60 == 0 { format!("{} h", v / 60) } else { format!("{v} min") }
}

fn secs_text(v: u32) -> String {
    format!("{v} s")
}

fn clock_text(m: u32) -> String {
    let (h, mm) = (m / 60 % 24, m % 60);
    let (hm, ampm) = win::hm(h as u16, mm as u16);
    match ampm {
        Some(a) => format!("{hm} {a}"),
        None => hm,
    }
}

fn days_text(n: i32) -> String {
    match n {
        0 => "None".to_owned(),
        _ => format!("{}{} day{}", if n > 0 { "+" } else { "\u{2212}" }, n.abs(), if n.abs() == 1 { "" } else { "s" }),
    }
}

/// A step along a list of choices (`dir` −1 or +1); clock times go round the day.
fn step(list: &[u32], v: u32, dir: i32, wraps: bool) -> Option<u32> {
    let next = if dir > 0 { list.iter().find(|&&s| s > v).copied() } else { list.iter().rev().find(|&&s| s < v).copied() };
    next.or_else(|| wraps.then(|| if dir > 0 { list[0] } else { list[list.len() - 1] }))
}

const QUIET_STEPS: [u32; 48] = {
    let mut a = [0u32; 48];
    let mut i = 0;
    while i < 48 {
        a[i] = i as u32 * 30;
        i += 1;
    }
    a
};

impl App {
    /// Opens the Settings window (or brings it to the front).
    pub fn open_settings(&mut self) {
        let k = self.gfx.scale;
        if self.settings.hwnd.is_invalid() {
            let Some(hwnd) = crate::create_settings_window() else { return };
            self.settings.hwnd = hwnd;
        }
        let (_, wa) = super::monitor_at(self.anchor_px);
        let (w, h) = ((SW_W * k).round() as i32, (SW_H * k).round() as i32);
        let pos = ((wa.left + wa.right - w) / 2, (wa.top + wa.bottom - h) / 2);
        if self.settings.surface.is_none() {
            unsafe {
                let _ = SetWindowPos(self.settings.hwnd, HWND_TOP, pos.0, pos.1, w, h, SWP_NOACTIVATE);
            }
            self.settings.surface = self.gfx.add_surface(self.settings.hwnd, SW_W, SW_H, pos, false).ok();
        } else if !self.settings.shown {
            unsafe {
                let _ = SetWindowPos(self.settings.hwnd, HWND_TOP, pos.0, pos.1, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
            }
            if let Some(s) = self.settings.surface {
                self.gfx.move_surface(s, pos);
            }
        }
        self.settings.shown = true;
        self.draw_settings();
        unsafe {
            let _ = ShowWindow(self.settings.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.settings.hwnd);
        }
        super::trace("[settings] open");
    }

    pub fn close_settings(&mut self) {
        self.settings.shown = false;
        unsafe {
            let _ = ShowWindow(self.settings.hwnd, SW_HIDE);
        }
    }

    /// The window was moved: its surface follows.
    pub fn settings_moved(&mut self) {
        let mut r = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = GetWindowRect(self.settings.hwnd, &mut r);
        }
        if let Some(s) = self.settings.surface {
            self.gfx.move_surface(s, (r.left, r.top));
        }
    }

    /// The title bar (away from the close button) drags the window.
    pub fn settings_drags(&self, x: f32, y: f32) -> bool {
        y < TITLE_H && self.settings.at(x, y).is_none()
    }

    pub fn settings_hover(&mut self, x: f32, y: f32) {
        if self.settings.cursor != (x, y) {
            self.settings.cursor = (x, y);
            self.draw_settings();
        }
    }

    pub fn settings_down(&mut self, x: f32, y: f32) {
        self.settings.pressed = self.settings.at(x, y);
        self.draw_settings();
    }

    pub fn settings_up(&mut self, x: f32, y: f32) -> Option<After> {
        let pressed = self.settings.pressed.take();
        let after = match pressed.filter(|d| self.settings.at(x, y) == Some(*d)) {
            Some(d) => self.settings_do(d),
            None => None,
        };
        if self.settings.shown {
            self.draw_settings();
        }
        after
    }

    pub fn settings_wheel(&mut self, delta: i32) {
        let room = SW_H - TITLE_H;
        let max = (self.settings.content_h - room).max(0.0);
        self.settings.scroll = (self.settings.scroll - delta as f32 / 120.0 * 60.0).clamp(0.0, max);
        self.draw_settings();
    }

    /// Keeps the window in step with changes made elsewhere (the tray, the notch).
    pub fn tick_settings(&mut self) {
        if self.settings.shown && self.settings.last_draw.elapsed().as_millis() > 500 {
            self.draw_settings();
        }
    }

    fn settings_do(&mut self, d: Do) -> Option<After> {
        let rem = |a: &mut App, f: &dyn Fn(&mut store::Reminders)| a.update_reminders(|r| f(r));
        match d {
            Do::Close => {
                self.close_settings();
                return None;
            }
            Do::Page(i) => {
                self.settings.page = i;
                self.settings.scroll = 0.0;
                return None;
            }
            Do::StartWithWindows => win::set_start_with_windows(!win::starts_with_windows()),
            Do::Vertical(on) => self.set_vertical(on),
            Do::Fullscreen => {
                let on = self.hide_in_fullscreen();
                self.set_hide_in_fullscreen(!on);
            }
            Do::MaxMode(m) => self.set_maximized_mode(m),
            Do::ResetPosition => self.reset_position(),
            Do::Glass => {
                let on = self.glass();
                self.set_glass(!on);
            }
            Do::Minimal => {
                let on = self.minimal_setting();
                self.set_minimal(!on);
            }
            Do::Share => {
                let on = self.share_visible();
                self.set_share_visible(!on);
            }
            Do::Overlay => {
                let on = self.overlay_shown();
                self.set_overlay(!on);
            }
            Do::PrayerAlerts => {
                let p = &mut self.store.prayer;
                p.alerts = !p.alerts;
                if !p.alerts {
                    self.prayer_ask = None;
                    sound::stop();
                }
                self.store.save();
            }
            Do::Hanafi => {
                let on = self.hanafi();
                self.set_hanafi(!on);
            }
            Do::Hijri(dir) => {
                let v = self.hijri_adjust();
                self.set_hijri_adjust(v + dir);
            }
            Do::Adhan => return Some(After::PickAdhan),
            Do::Bells => {
                sound::stop();
                let _ = sound::set_adhan(None);
            }
            Do::Reminders => rem(self, &|r| r.on = !r.on),
            Do::Every(dir) => rem(self, &|r| {
                if let Some(v) = step(&store::REMIND_EVERY, r.minutes, dir, false) {
                    r.minutes = v;
                }
            }),
            Do::ShowFor(dir) => rem(self, &|r| {
                if let Some(v) = step(&store::REMIND_FOR, r.seconds, dir, false) {
                    r.seconds = v;
                }
            }),
            Do::Ayah => rem(self, &|r| r.ayah = !r.ayah),
            Do::Dhikr => rem(self, &|r| r.dhikr = !r.dhikr),
            Do::Dua => rem(self, &|r| r.dua = !r.dua),
            Do::Quiet => rem(self, &|r| r.quiet_hours = !r.quiet_hours),
            Do::QuietFrom(dir) => rem(self, &|r| {
                if let Some(v) = step(&QUIET_STEPS, r.quiet_from, dir, true) {
                    r.quiet_from = v;
                }
            }),
            Do::QuietTo(dir) => rem(self, &|r| {
                if let Some(v) = step(&QUIET_STEPS, r.quiet_to, dir, true) {
                    r.quiet_to = v;
                }
            }),
            Do::ShowOneNow => self.reminder_now(),
        }
        self.settings.saved_at = Some(Instant::now());
        None
    }

    fn settings_groups(&self) -> (&'static str, Vec<Group>) {
        let set = &self.store.settings;
        match self.settings.page {
            0 => {
                let max = self.maximized_mode() as usize;
                let groups = vec![
                    Group {
                        title: None,
                        rows: vec![
                            switch("Start with Windows", win::starts_with_windows(), Do::StartWithWindows),
                            row("Orientation", Ctl::Seg(vec![("Horizontal", Do::Vertical(false)), ("Vertical", Do::Vertical(true))], set.vertical as usize))
                                .note("Vertical stands it on the left or right edge"),
                            row("Position", Ctl::Buttons(vec![("Reset", Do::ResetPosition)])).note("Drag the notch to move it; near the top it docks"),
                            switch("Hide in full screen", set.hide_in_fullscreen, Do::Fullscreen),
                            row("Over maximized windows", Ctl::Seg(vec![("Stay", Do::MaxMode(0)), ("Shrink", Do::MaxMode(1)), ("Hide", Do::MaxMode(2))], max)),
                        ],
                        action: None,
                    },
                    Group {
                        title: Some("Look"),
                        rows: vec![
                            switch("Liquid Glass", set.glass, Do::Glass).note("Blurs what's behind the notch; off for solid black"),
                            switch("Minimal notch", set.minimal, Do::Minimal).note("A thin line while closed"),
                            switch("Show in screen sharing", self.share_visible(), Do::Share).note("Discord and other screen shares can see it"),
                            switch("FPS overlay", set.overlay, Do::Overlay),
                        ],
                        action: None,
                    },
                ];
                ("General", groups)
            }
            _ => {
                let p = &self.store.prayer;
                let r = &self.store.reminders;
                let adj = p.hijri_adjust;
                let adhan = sound::adhan();
                let adhan_note = adhan.as_ref().and_then(|a| a.file_name()).map_or("The soft bells".to_owned(), |n| n.to_string_lossy().into_owned());
                let mut adhan_btns = vec![("Choose\u{2026}", Do::Adhan)];
                if adhan.is_some() {
                    adhan_btns.push(("Use the bells", Do::Bells));
                }
                let mut dhikr = vec![switch("Show reminders", r.on, Do::Reminders)];
                if r.on {
                    let every_i = store::REMIND_EVERY.iter().position(|&v| v == r.minutes);
                    let for_i = store::REMIND_FOR.iter().position(|&v| v == r.seconds);
                    dhikr.extend([
                        row("Every", Ctl::Stepper(mins_text(r.minutes), every_i != Some(0), every_i != Some(store::REMIND_EVERY.len() - 1), Do::Every(-1), Do::Every(1))).sub(),
                        row("On screen for", Ctl::Stepper(secs_text(r.seconds), for_i != Some(0), for_i != Some(store::REMIND_FOR.len() - 1), Do::ShowFor(-1), Do::ShowFor(1))).sub(),
                        switch("Qur'an verses", r.ayah, Do::Ayah).sub(),
                        switch("Dhikr", r.dhikr, Do::Dhikr).sub(),
                        switch("Du'a", r.dua, Do::Dua).sub(),
                        switch("Quiet hours", r.quiet_hours, Do::Quiet).sub(),
                    ]);
                    if r.quiet_hours {
                        dhikr.extend([
                            row("From", Ctl::Stepper(clock_text(r.quiet_from), true, true, Do::QuietFrom(-1), Do::QuietFrom(1))).sub(),
                            row("To", Ctl::Stepper(clock_text(r.quiet_to), true, true, Do::QuietTo(-1), Do::QuietTo(1))).sub(),
                        ]);
                    }
                }
                let groups = vec![
                    Group {
                        title: Some("Prayer"),
                        rows: vec![
                            switch("Prayer alerts", p.alerts, Do::PrayerAlerts),
                            switch("Hanafi Asr", p.hanafi, Do::Hanafi).note("Asr when a shadow is twice its length"),
                            row("Hijri date", Ctl::Stepper(days_text(adj), adj > -2, adj < 2, Do::Hijri(-1), Do::Hijri(1))).note("Match the local moon sighting"),
                            row("Adhan sound", Ctl::Buttons(adhan_btns)).note(adhan_note),
                        ],
                        action: None,
                    },
                    Group { title: Some("Dhikr, du'a & Qur'an"), rows: dhikr, action: Some(("Show one now", Do::ShowOneNow)) },
                ];
                ("Prayer & Dhikr", groups)
            }
        }
    }

    pub fn draw_settings(&mut self) {
        let Some(surface) = self.settings.surface else { return };
        if self.gfx.begin(surface).is_err() {
            return;
        }
        self.settings.last_draw = Instant::now();
        self.settings.regions.borrow_mut().clear();
        let (title, groups) = self.settings_groups();
        let w = &self.settings;
        let g = &self.gfx;
        let hit = |r: Rect, d: Do| w.regions.borrow_mut().push((r, d));
        let hov = |r: Rect| r.contains(w.cursor.0, w.cursor.1);
        let down = |d: Do| w.pressed == Some(d);
        g.fill_rrect(Rect::new(0.0, 0.0, SW_W, SW_H), 0.0, BG);

        // The page, scrolled under the title bar.
        let x0 = SIDE_W + 32.0;
        let cw = SW_W - x0 - 32.0;
        g.clip(Rect::new(SIDE_W, TITLE_H, SW_W - SIDE_W, SW_H - TITLE_H));
        let mut y = TITLE_H + 14.0 - w.scroll;
        g.text(title, x0, y + 14.0, cw, 22.0, 700, WHITE, Align::Start);
        y += 44.0;
        for group in &groups {
            if let Some(t) = group.title {
                g.text(t, x0 + 4.0, y + 10.0, cw, 11.5, 600, white(0.5), Align::Start);
                y += 24.0;
            }
            let card = Rect::new(x0, y, cw, ROW_H * group.rows.len() as f32);
            g.fill_rrect(card, 12.0, white(0.045));
            g.stroke_rrect(card, 12.0, 1.0, white(0.06), false);
            for (i, r) in group.rows.iter().enumerate() {
                let ry = y + i as f32 * ROW_H;
                let rr = Rect::new(x0, ry, cw, ROW_H);
                if i > 0 {
                    g.fill_rrect(Rect::new(x0 + 16.0, ry, cw - 32.0, 1.0), 0.0, white(0.06));
                }
                let lx = x0 + if r.sub { 34.0 } else { 16.0 };
                let cy = ry + ROW_H / 2.0;
                if let Some(note) = &r.note {
                    g.text(r.label, lx, cy - 8.0, cw * 0.55, 13.0, 500, white(0.92), Align::Start);
                    g.text(note, lx, cy + 9.0, cw * 0.6, 11.0, 400, white(0.45), Align::Start);
                } else {
                    g.text(r.label, lx, cy, cw * 0.55, 13.0, 500, white(if r.sub { 0.8 } else { 0.92 }), Align::Start);
                }
                let right = x0 + cw - 16.0;
                match &r.ctl {
                    Ctl::Switch(on) => {
                        let sw = Rect::new(right - 38.0, cy - 11.0, 38.0, 22.0);
                        let act = r.act.unwrap_or(Do::Close);
                        let lit = hov(rr);
                        g.fill_rrect(sw, 11.0, if *on { AMBER } else { white(if lit { 0.22 } else { 0.16 }) });
                        let kx = if *on { sw.x + sw.w - 11.0 } else { sw.x + 11.0 };
                        g.fill_circle(kx, cy, if down(act) { 9.5 } else { 8.5 }, if *on { rgb(0x2a, 0x1c, 0x00) } else { WHITE });
                        if lit {
                            g.fill_rrect(rr.grow(-1.0), 11.0, white(0.025));
                        }
                        hit(rr, act);
                    }
                    Ctl::Stepper(text, can_dec, can_inc, dec, inc) => {
                        let bw = 28.0;
                        let tw = 92.0;
                        let plus = Rect::new(right - bw, cy - 14.0, bw, 28.0);
                        let out = Rect::new(plus.x - tw, cy - 14.0, tw, 28.0);
                        let minus = Rect::new(out.x - bw, cy - 14.0, bw, 28.0);
                        g.fill_rrect(Rect::new(minus.x, minus.y, bw * 2.0 + tw, 28.0), 8.0, white(0.06));
                        for (b, sym, ok, d) in [(minus, "\u{2212}", *can_dec, *dec), (plus, "+", *can_inc, *inc)] {
                            if ok && hov(b) {
                                g.fill_rrect(b, 8.0, white(if down(d) { 0.16 } else { 0.1 }));
                            }
                            g.text(sym, b.x, cy, b.w, 15.0, 600, white(if ok { 0.9 } else { 0.25 }), Align::Center);
                            if ok {
                                hit(b, d);
                            }
                        }
                        g.text(text, out.x, cy, out.w, 12.5, 600, WHITE, Align::Center);
                    }
                    Ctl::Seg(items, on) => {
                        let ww: Vec<f32> = items.iter().map(|(t, _)| g.measure(t, 12.0, 600) + 24.0).collect();
                        let total: f32 = ww.iter().sum::<f32>() + 4.0;
                        let mut x = right - total;
                        g.fill_rrect(Rect::new(x, cy - 15.0, total, 30.0), 9.0, white(0.06));
                        x += 2.0;
                        for (i, ((t, d), bw)) in items.iter().zip(&ww).enumerate() {
                            let b = Rect::new(x, cy - 13.0, *bw, 26.0);
                            if i == *on {
                                g.fill_rrect(b, 7.0, rgba(0xff, 0xc4, 0x3d, 0.22));
                            } else if hov(b) {
                                g.fill_rrect(b, 7.0, white(0.07));
                            }
                            g.text(t, b.x, cy, b.w, 12.0, 600, if i == *on { AMBER } else { white(0.75) }, Align::Center);
                            hit(b, *d);
                            x += bw;
                        }
                    }
                    Ctl::Buttons(items) => {
                        let mut x = right;
                        for (t, d) in items.iter().rev() {
                            let bw = g.measure(t, 12.0, 600) + 26.0;
                            x -= bw;
                            let b = Rect::new(x, cy - 14.0, bw, 28.0);
                            g.fill_rrect(b, 8.0, white(if down(*d) { 0.18 } else if hov(b) { 0.13 } else { 0.08 }));
                            g.text(t, b.x, cy, b.w, 12.0, 600, WHITE, Align::Center);
                            hit(b, *d);
                            x -= 8.0;
                        }
                    }
                }
            }
            y += card.h + 10.0;
            if let Some((t, d)) = group.action {
                let bw = g.measure(t, 12.5, 600) + 30.0;
                let b = Rect::new(x0 + cw - bw, y, bw, 32.0);
                g.fill_rrect(b, 9.0, rgba(0xff, 0xc4, 0x3d, if down(d) { 0.4 } else if hov(b) { 0.32 } else { 0.24 }));
                g.text(t, b.x, b.y + 16.0, b.w, 12.5, 600, AMBER, Align::Center);
                hit(b, d);
                y += 42.0;
            }
            y += 14.0;
        }
        g.unclip();
        let content_h = y + w.scroll - TITLE_H;

        // The sidebar and the title bar, over the page.
        g.fill_rrect(Rect::new(0.0, 0.0, SIDE_W, SW_H), 0.0, SIDE);
        g.fill_rrect(Rect::new(0.0, 0.0, SW_W, TITLE_H), 0.0, SIDE);
        g.fill_poly(&crescent(24.0, TITLE_H / 2.0, 8.0, (3.7, -2.6), 6.7), AMBER);
        g.text("Hilal", 40.0, TITLE_H / 2.0, 120.0, 13.5, 700, WHITE, Align::Start);
        g.text("Settings", 40.0 + g.measure("Hilal ", 13.5, 700), TITLE_H / 2.0, 120.0, 13.5, 500, white(0.5), Align::Start);
        let close = Rect::new(SW_W - 46.0, 0.0, 46.0, TITLE_H - 6.0);
        if hov(close) {
            g.fill_rrect(close, 0.0, rgb(0xc4, 0x2b, 0x1c));
        }
        g.icon('\u{E8BB}', close.x + close.w / 2.0, close.y + close.h / 2.0, 10.0, white(if hov(close) { 1.0 } else { 0.7 }));
        hit(close, Do::Close);
        for (i, name) in PAGES.iter().enumerate() {
            let b = Rect::new(10.0, TITLE_H + 6.0 + i as f32 * 38.0, SIDE_W - 20.0, 34.0);
            let on = i == w.page;
            if on || hov(b) {
                g.fill_rrect(b, 8.0, white(if on { 0.09 } else { 0.05 }));
            }
            if on {
                g.fill_rrect(Rect::new(b.x, b.y + 9.0, 3.0, b.h - 18.0), 1.5, AMBER);
            }
            g.text(name, b.x + 14.0, b.y + b.h / 2.0, b.w - 20.0, 13.0, 500, white(if on { 1.0 } else { 0.65 }), Align::Start);
            hit(b, Do::Page(i));
        }
        g.text("Ctrl+Alt+H pins the notch open", 18.0, SW_H - 40.0, SIDE_W - 26.0, 11.0, 400, white(0.45), Align::Start);
        g.text(&format!("Hilal {}", env!("CARGO_PKG_VERSION")), 18.0, SW_H - 22.0, SIDE_W - 26.0, 11.0, 400, white(0.35), Align::Start);
        if let Some(t) = w.saved_at.filter(|t| t.elapsed().as_secs_f32() < 1.6) {
            let a = (1.6 - t.elapsed().as_secs_f32()).min(1.0);
            g.text("Saved", SW_W - 120.0, TITLE_H / 2.0, 60.0, 11.5, 500, [0.6, 0.9, 0.6, 0.8 * a], Align::End);
        }
        let _ = self.gfx.present();
        self.settings.content_h = content_h;
    }
}
