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

/// Something a click in the window does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Do {
    Close,
    Page(usize),
    Nova(bool),
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
    Feature(Feat),
    ShotMinutes(i32),
    TogglePage(usize),
    Slide(u8),
    PinApp(usize),
    Meetings,
    Companion(u8),
    Theme(usize),
    Perf(u8),
    SaverAt(i32),
    AddrIsland(bool),
    AutoUpdate,
    News,
    FolderDownloads,
    DlToasts,
    Field,
    SaveField,
}

/// The Nova switches on the Alerts & Privacy page.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Feat {
    Volume,
    Brightness,
    Battery,
    Keyboard,
    Caps,
    Internet,
    Screenshots,
    ShotShelf,
    SmartClip,
    PrivacyDots,
    ClipHistory,
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
    /// The text box being typed in (the calendar link), and whether its last save was refused.
    pub field: Option<String>,
    field_bad: bool,
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
            field: None,
            field_bad: false,
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
    /// A one-line text box: what it shows, its hint when empty, and a Save button.
    Field(String, &'static str),
}

struct Row {
    label: std::borrow::Cow<'static, str>,
    note: Option<String>,
    ctl: Ctl,
    /// What a click on the row (a switch) does.
    act: Option<Do>,
    sub: bool,
}

fn row(label: &'static str, ctl: Ctl) -> Row {
    Row { label: label.into(), note: None, ctl, act: None, sub: false }
}

fn switch(label: &'static str, on: bool, act: Do) -> Row {
    Row { label: label.into(), note: None, ctl: Ctl::Switch(on), act: Some(act), sub: false }
}

/// A switch whose label isn't fixed text (an app's name).
fn switch_owned(label: String, on: bool, act: Do) -> Row {
    Row { label: label.into(), note: None, ctl: Ctl::Switch(on), act: Some(act), sub: false }
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
    /// Opens Settings at a page (Apps' Add opens the Start menu list).
    pub fn open_settings_at(&mut self, page: usize) {
        self.settings.page = page;
        self.settings.scroll = 0.0;
        self.open_settings();
    }

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

    /// Typing into the text box (Backspace, Enter saves, Esc leaves, Ctrl+V pastes). False
    /// when no box is being typed in.
    pub fn settings_char(&mut self, c: char) -> bool {
        let Some(text) = self.settings.field.as_mut() else { return false };
        match c {
            '\u{8}' => {
                text.pop();
            }
            '\r' => {
                let _ = self.settings_do(Do::SaveField);
            }
            '\u{1b}' => self.settings.field = None,
            '\u{16}' => {
                if let Some(p) = super::toasts::clipboard_text(self.settings.hwnd) {
                    text.push_str(p.trim());
                }
            }
            '\u{1}' => text.clear(),
            c if !c.is_control() && text.len() < 2000 => text.push(c),
            _ => {}
        }
        self.draw_settings();
        true
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
            Do::Nova(on) => self.set_nova(on),
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
            Do::Feature(which) => {
                let f = &mut self.store.features;
                let v = match which {
                    Feat::Volume => &mut f.hud_volume,
                    Feat::Brightness => &mut f.hud_brightness,
                    Feat::Battery => &mut f.battery,
                    Feat::Keyboard => &mut f.kbd_toast,
                    Feat::Caps => &mut f.caps_toast,
                    Feat::Internet => &mut f.net_toasts,
                    Feat::Screenshots => &mut f.screenshot_toast,
                    Feat::ShotShelf => &mut f.shot_auto_shelf,
                    Feat::SmartClip => &mut f.smart_clipboard,
                    Feat::PrivacyDots => &mut f.privacy_dots,
                    Feat::ClipHistory => &mut f.clipboard_history,
                };
                *v = !*v;
                if which == Feat::ClipHistory && !self.store.features.clipboard_history {
                    self.clip_history.clear();
                }
                self.store.save();
                self.closed_sig.clear();
            }
            Do::ShotMinutes(dir) => {
                let f = &mut self.store.features;
                if let Some(v) = step(&store::SHOT_SHELF, f.shot_shelf_minutes, dir, false) {
                    f.shot_shelf_minutes = v;
                }
                self.store.save();
            }
            Do::Meetings => {
                let f = &mut self.store.features;
                f.meetings = !f.meetings;
                self.store.save();
            }
            Do::Theme(i) => {
                if let Some(t) = crate::themes::all().get(i) {
                    self.store.features.theme = t.id.to_owned();
                    self.store.save();
                    self.closed_sig.clear();
                }
            }
            Do::Perf(k) => {
                let f = &mut self.store.features;
                match k {
                    0 => f.no_animations = !f.no_animations,
                    1 => f.glass_freeze = !f.glass_freeze,
                    _ => f.battery_saver = !f.battery_saver,
                }
                self.store.save();
                if let Some((pct, charging)) = win::battery() {
                    self.check_saver(charging, pct as f32 / 100.0);
                }
            }
            Do::SaverAt(dir) => {
                const STEPS: [u32; 12] = [10, 15, 20, 25, 30, 40, 50, 60, 70, 80, 90, 100];
                let f = &mut self.store.features;
                if let Some(v) = step(&STEPS, f.battery_saver_at, dir, false) {
                    f.battery_saver_at = v;
                }
                self.store.save();
            }
            Do::AutoUpdate => {
                let f = &mut self.store.features;
                f.auto_update = !f.auto_update;
                self.store.save();
            }
            Do::News => {
                let f = &mut self.store.features;
                f.news = !f.news;
                self.store.save();
            }
            Do::AddrIsland(on) => {
                self.store.features.addr_island = on;
                self.store.save();
                self.apply_position();
            }
            Do::Companion(k) => {
                let f = &mut self.store.features;
                let v = match k {
                    0 => &mut f.welcome_back,
                    1 => &mut f.morning_brief,
                    2 => &mut f.wellbeing,
                    3 => &mut f.celebrate,
                    4 => &mut f.nova_eyes,
                    5 => &mut f.nova_prayer,
                    _ => &mut f.nova_focus,
                };
                *v = !*v;
                self.store.save();
            }
            Do::FolderDownloads => {
                let f = &mut self.store.features;
                f.folder_downloads = !f.folder_downloads;
                self.store.save();
            }
            Do::DlToasts => {
                let f = &mut self.store.features;
                f.dl_toasts = !f.dl_toasts;
                self.store.save();
            }
            Do::Field => {
                if self.settings.field.is_none() {
                    self.settings.field = Some(self.store.features.calendar_url.clone());
                }
                self.settings.field_bad = false;
                return None;
            }
            Do::SaveField => {
                let text = self.settings.field.clone().unwrap_or_else(|| self.store.features.calendar_url.clone());
                if self.set_calendar_url(&text) {
                    self.settings.field = None;
                    self.settings.field_bad = false;
                } else {
                    self.settings.field_bad = true;
                    return None;
                }
            }
            Do::PinApp(i) => {
                if let Some(a) = self.store.apps_catalog.get(i).cloned() {
                    let mut list = self.pinned_apps();
                    if let Some(k) = list.iter().position(|p| p.id == a.id) {
                        list.remove(k);
                    } else if list.len() < crate::apps::MAX_PINNED {
                        list.push(a);
                    }
                    self.store.apps_pinned = Some(list);
                    self.store.save();
                }
            }
            Do::Slide(k) => {
                let f = &mut self.store.features;
                let v = match k {
                    0 => &mut f.slide_clock,
                    1 => &mut f.slide_stats,
                    2 => &mut f.slide_prayer,
                    _ => &mut f.track_peek,
                };
                *v = !*v;
                self.store.save();
                self.closed_sig.clear();
            }
            Do::TogglePage(i) => {
                let name = super::NOVA_PAGES[i].name().to_owned();
                let hidden = &mut self.store.features.hidden_pages;
                if let Some(k) = hidden.iter().position(|h| *h == name) {
                    hidden.remove(k);
                } else if super::NOVA_PAGES.len() - hidden.len() > 1 {
                    hidden.push(name);
                }
                self.store.save();
            }
        }
        self.settings.saved_at = Some(Instant::now());
        None
    }

    fn settings_groups(&self) -> (&'static str, Vec<Group>) {
        let set = &self.store.settings;
        let nova = set.nova;
        match self.settings.page {
            0 => {
                let max = self.maximized_mode() as usize;
                let mode_note = if nova { "Nova Island's features are on" } else { "Hilal as it was, before Nova's features" };
                let mut rows = vec![
                    row("Style", Ctl::Seg(vec![("Classic Hilal", Do::Nova(false)), ("Nova", Do::Nova(true))], nova as usize)).note(mode_note),
                    switch("Start with Windows", win::starts_with_windows(), Do::StartWithWindows),
                ];
                if nova {
                    rows.extend([
                        row("Orientation", Ctl::Seg(vec![("Horizontal", Do::Vertical(false)), ("Vertical", Do::Vertical(true))], set.vertical as usize))
                            .note("Vertical stands it on the left or right edge"),
                        row("Position", Ctl::Buttons(vec![("Reset", Do::ResetPosition)])).note("Drag the notch to move it; near the top it docks"),
                    ]);
                }
                rows.extend([
                    switch("Hide in full screen", set.hide_in_fullscreen, Do::Fullscreen),
                    row("Over maximized windows", Ctl::Seg(vec![("Stay", Do::MaxMode(0)), ("Shrink", Do::MaxMode(1)), ("Hide", Do::MaxMode(2))], max)),
                ]);
                let mut look = vec![
                    switch("Liquid Glass", set.glass, Do::Glass).note("Blurs what's behind the notch; off for solid black"),
                    switch("Minimal notch", set.minimal, Do::Minimal).note("A thin line while closed"),
                ];
                if nova {
                    look.push(switch("Show in screen sharing", self.share_visible(), Do::Share).note("Discord and other screen shares can see it"));
                }
                look.push(switch("FPS overlay", set.overlay, Do::Overlay));
                let mut groups = vec![Group { title: None, rows, action: None }, Group { title: Some("Look"), rows: look, action: None }];
                if nova {
                    let f = &self.store.features;
                    let mut perf = vec![
                        switch("Remove animations", f.no_animations, Do::Perf(0)),
                        switch("Freeze glass while closed", f.glass_freeze, Do::Perf(1)).note("The glass refreshes only while the notch is open"),
                        switch("Battery saver", f.battery_saver, Do::Perf(2)).note("Glass off, no animations and frozen glass when the battery runs low"),
                    ];
                    if f.battery_saver {
                        const STEPS: [u32; 12] = [10, 15, 20, 25, 30, 40, 50, 60, 70, 80, 90, 100];
                        let v = f.battery_saver_at;
                        perf.push(row("Below", Ctl::Stepper(format!("{v}%"), v > STEPS[0], v < STEPS[11], Do::SaverAt(-1), Do::SaverAt(1))).sub());
                    }
                    groups.push(Group { title: Some("Performance"), rows: perf, action: None });
                    let addr = vec![row("In the browser", Ctl::Seg(vec![("Address bar", Do::AddrIsland(true)), ("Stay in place", Do::AddrIsland(false))], if f.addr_island { 0 } else { 1 }))
                        .note("In Chrome, Edge or Brave, the notch sits in the address bar's right third (from its own place only)")];
                    groups.push(Group { title: Some("Browser"), rows: addr, action: None });
                    let app = vec![
                        switch("Auto-update", f.auto_update, Do::AutoUpdate).note("New versions from GitHub, offered on the notch"),
                        switch("News from Hilal", f.news, Do::News).note("Read from Hilal's GitHub page; nothing is sent from your PC"),
                    ];
                    groups.push(Group { title: Some("Hilal"), rows: app, action: None });
                }
                ("General", groups)
            }
            2 => {
                let f = &self.store.features;
                let sw = |label, on: bool, feat| switch(label, on, Do::Feature(feat));
                let shot_i = store::SHOT_SHELF.iter().position(|&v| v == f.shot_shelf_minutes);
                let mut alerts = vec![
                    sw("Volume", f.hud_volume, Feat::Volume),
                    sw("Brightness", f.hud_brightness, Feat::Brightness).note("Laptop screens"),
                    sw("Battery", f.battery, Feat::Battery).note("Charging, and when it runs low"),
                    sw("Keyboard language", f.kbd_toast, Feat::Keyboard),
                    sw("Caps Lock", f.caps_toast, Feat::Caps),
                    sw("Internet", f.net_toasts, Feat::Internet).note("When it drops or comes back, and Wi-Fi"),
                    sw("Screenshots", f.screenshot_toast, Feat::Screenshots).note("A copied screenshot or image"),
                ];
                if f.screenshot_toast {
                    alerts.push(sw("Save screenshots to Shelf", f.shot_auto_shelf, Feat::ShotShelf).note("Saved to Pictures\\Hilal; the file stays there").sub());
                    if f.shot_auto_shelf {
                        let v = f.shot_shelf_minutes;
                        let text = if v >= 60 && v % 60 == 0 { format!("{} h", v / 60) } else { format!("{v} min") };
                        alerts.push(row("Keep on Shelf", Ctl::Stepper(text, shot_i != Some(0), shot_i != Some(store::SHOT_SHELF.len() - 1), Do::ShotMinutes(-1), Do::ShotMinutes(1))).sub());
                    }
                }
                alerts.push(sw("Smart clipboard", f.smart_clipboard, Feat::SmartClip).note("Copied links, colors and sums"));
                let privacy = vec![
                    sw("Mic & camera dots", f.privacy_dots, Feat::PrivacyDots).note("And a toast naming the app"),
                    sw("Clipboard history", f.clipboard_history, Feat::ClipHistory).note("Kept in memory only"),
                ];
                ("Alerts & Privacy", vec![Group { title: Some("Alerts"), rows: alerts, action: None }, Group { title: Some("Privacy"), rows: privacy, action: None }])
            }
            7 => {
                let cur = &self.store.features.theme;
                let rows = crate::themes::all()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        let kind = if t.id == "original" { "Hilal's own Liquid Glass" } else if t.glass.is_some() { "Glass" } else { "Solid (lighter: no screen capture)" };
                        switch(t.name, t.id == cur, Do::Theme(i)).note(kind)
                    })
                    .collect();
                ("Themes", vec![Group { title: Some("Give the notch a new look"), rows, action: None }])
            }
            6 => {
                let f = &self.store.features;
                let c = |label, on, k| switch(label, on, Do::Companion(k));
                let moments = vec![
                    c("Welcome back", f.welcome_back, 0).note("What happened while you were away"),
                    c("Morning card", f.morning_brief, 1).note("The date, weather, next prayer and today's meetings"),
                    c("Breaks", f.wellbeing, 2).note("A gentle card after 50 minutes at the screen"),
                    c("Celebrations", f.celebrate, 3).note("Confetti when something's done"),
                ];
                let mascot = vec![
                    c("Eyes", f.nova_eyes, 4).note("On the notch when the mouse lingers beside it"),
                    c("At the adhan", f.nova_prayer, 5).note("A card with the verse, instead of the peek"),
                    c("At focus start and end", f.nova_focus, 6),
                ];
                ("Companion", vec![Group { title: Some("Moments"), rows: moments, action: None }, Group { title: Some("The mascot"), rows: mascot, action: None }])
            }
            5 => {
                let f = &self.store.features;
                let mut rows = vec![switch("Upcoming meetings", f.meetings, Do::Meetings).note("A toast 10 and 1 minutes before, and Join")];
                if f.meetings {
                    let shown = self.settings.field.clone().unwrap_or_else(|| f.calendar_url.clone());
                    let mut link = row("Link", Ctl::Field(shown, "iCal link (https://\u{2026} .ics)")).sub();
                    if self.settings.field_bad {
                        link = link.note("That isn't an https or webcal link");
                    }
                    rows.push(link);
                    rows.push(row("Status", Ctl::Buttons(Vec::new())).note(self.calendar_status()).sub());
                    rows.push(row("Google", Ctl::Buttons(Vec::new())).note("Settings \u{203a} your calendar \u{203a} Secret address in iCal format").sub());
                    rows.push(row("Outlook", Ctl::Buttons(Vec::new())).note("Settings \u{203a} Calendar \u{203a} Shared calendars \u{203a} Publish (ICS)").sub());
                }
                let downloads = vec![
                    switch("Browser downloads", f.folder_downloads, Do::FolderDownloads).note("Seen in your Downloads folder; no file is read"),
                    switch("Download toasts", f.dl_toasts, Do::DlToasts).note("When one starts and when it's done"),
                ];
                ("Connections", vec![Group { title: Some("Calendar"), rows, action: None }, Group { title: Some("Downloads"), rows: downloads, action: None }])
            }
            4 => {
                let pinned = self.pinned_apps();
                let rows: Vec<Row> = self
                    .store
                    .apps_catalog
                    .iter()
                    .enumerate()
                    .map(|(i, a)| switch_owned(a.name.clone(), pinned.iter().any(|p| p.id == a.id), Do::PinApp(i)))
                    .collect();
                let title = if rows.is_empty() { "Reading the Start menu\u{2026}" } else { "Pin up to 12 apps from the Start menu" };
                ("Apps", vec![Group { title: Some(title), rows, action: None }])
            }
            3 => {
                let hidden = &self.store.features.hidden_pages;
                let rows = super::NOVA_PAGES
                    .iter()
                    .enumerate()
                    .map(|(i, p)| switch(p.title(), !hidden.iter().any(|h| h == p.name()), Do::TogglePage(i)))
                    .collect();
                let f = &self.store.features;
                let collapsed = vec![
                    switch("Clock & weather", f.slide_clock, Do::Slide(0)),
                    switch("CPU & memory", f.slide_stats, Do::Slide(1)),
                    switch("Next prayer", f.slide_prayer, Do::Slide(2)).note("In Ramadan, iftar and imsak"),
                ];
                let playing = vec![switch("New track card", f.track_peek, Do::Slide(3)).note("The cover and title for a moment")];
                (
                    "Island",
                    vec![
                        Group { title: Some("Pages"), rows, action: None },
                        Group { title: Some("Collapsed (turns every 6 seconds)"), rows: collapsed, action: None },
                        Group { title: Some("Now playing"), rows: playing, action: None },
                    ],
                )
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
                let mut prayer = vec![switch("Prayer alerts", p.alerts, Do::PrayerAlerts)];
                if nova {
                    prayer.extend([
                        switch("Hanafi Asr", p.hanafi, Do::Hanafi).note("Asr when a shadow is twice its length"),
                        row("Hijri date", Ctl::Stepper(days_text(adj), adj > -2, adj < 2, Do::Hijri(-1), Do::Hijri(1))).note("Match the local moon sighting"),
                    ]);
                }
                prayer.push(row("Adhan sound", Ctl::Buttons(adhan_btns)).note(adhan_note));
                let mut groups = vec![Group { title: Some("Prayer"), rows: prayer, action: None }];
                if nova {
                    groups.push(Group { title: Some("Dhikr, du'a & Qur'an"), rows: dhikr, action: Some(("Show one now", Do::ShowOneNow)) });
                }
                (if nova { "Prayer & Dhikr" } else { "Prayer" }, groups)
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
        if self.settings.page == 4 && self.nova() {
            self.load_apps();
        }
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
                if ry + ROW_H < TITLE_H || ry > SW_H {
                    continue;
                }
                if i > 0 {
                    g.fill_rrect(Rect::new(x0 + 16.0, ry, cw - 32.0, 1.0), 0.0, white(0.06));
                }
                let lx = x0 + if r.sub { 34.0 } else { 16.0 };
                let cy = ry + ROW_H / 2.0;
                if let Some(note) = &r.note {
                    g.text(&r.label, lx, cy - 8.0, cw * 0.55, 13.0, 500, white(0.92), Align::Start);
                    g.text(note, lx, cy + 9.0, cw * 0.6, 11.0, 400, white(0.45), Align::Start);
                } else {
                    g.text(&r.label, lx, cy, cw * 0.55, 13.0, 500, white(if r.sub { 0.8 } else { 0.92 }), Align::Start);
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
                    Ctl::Field(text, hint) => {
                        let save_w = 56.0;
                        let save = Rect::new(right - save_w, cy - 14.0, save_w, 28.0);
                        let bx = Rect::new(x0 + 110.0, cy - 14.0, save.x - 8.0 - (x0 + 110.0), 28.0);
                        let focused = w.field.is_some();
                        g.fill_rrect(bx, 8.0, white(if focused { 0.1 } else { 0.06 }));
                        if focused {
                            g.stroke_rrect(bx, 8.0, 1.2, rgba(0xff, 0xc4, 0x3d, 0.7), false);
                        }
                        let shown = if text.is_empty() { (*hint).to_owned() } else { text.clone() };
                        // (The end of a long link shows, where the typing is.)
                        let mut s = shown.clone();
                        while g.measure(&s, 12.0, 500) > bx.w - 24.0 && s.chars().count() > 1 {
                            s.remove(0);
                        }
                        if s.len() < shown.len() {
                            s.insert(0, '\u{2026}');
                        }
                        g.text(&s, bx.x + 10.0, cy, bx.w - 20.0, 12.0, 500, white(if text.is_empty() { 0.35 } else { 0.9 }), Align::Start);
                        if focused && (w.last_draw.elapsed().as_millis() / 500) % 2 == 0 {
                            let cx = bx.x + 10.0 + if text.is_empty() { 0.0 } else { g.measure(&s, 12.0, 500) };
                            g.fill_rrect(Rect::new(cx + 1.0, cy - 8.0, 1.5, 16.0), 0.5, AMBER);
                        }
                        hit(bx, Do::Field);
                        g.fill_rrect(save, 8.0, rgba(0xff, 0xc4, 0x3d, if hov(save) { 0.32 } else { 0.22 }));
                        g.text("Save", save.x, cy, save.w, 12.0, 600, AMBER, Align::Center);
                        hit(save, Do::SaveField);
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
        let pages: &[&str] = if self.store.settings.nova { &["General", "Prayer & Dhikr", "Alerts & Privacy", "Island", "Apps", "Connections", "Companion", "Themes"] } else { &["General", "Prayer"] };
        for (i, name) in pages.iter().enumerate() {
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
        if self.store.settings.nova {
            g.text("Ctrl+Alt+H pins the notch open", 18.0, SW_H - 40.0, SIDE_W - 26.0, 11.0, 400, white(0.45), Align::Start);
        }
        g.text(&format!("Hilal {}", env!("CARGO_PKG_VERSION")), 18.0, SW_H - 22.0, SIDE_W - 26.0, 11.0, 400, white(0.35), Align::Start);
        if let Some(t) = w.saved_at.filter(|t| t.elapsed().as_secs_f32() < 1.6) {
            let a = (1.6 - t.elapsed().as_secs_f32()).min(1.0);
            g.text("Saved", SW_W - 120.0, TITLE_H / 2.0, 60.0, 11.5, 500, [0.6, 0.9, 0.6, 0.8 * a], Align::End);
        }
        let _ = self.gfx.present();
        self.settings.content_h = content_h;
    }
}
