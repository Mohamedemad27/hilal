//! The notch: state, animation, page drawing and input. Everything is drawn in dp on a
//! 640x190 transparent canvas; the notch hangs from the top-center of it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SetFocus, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::{self, rgb, rgba, white, Align, Color, Gfx, Image, Rect, BLACK, WHITE};
use crate::lyrics::{self, Lyrics};
use crate::media::{Command, Media, Track};
use crate::store::{self, Store, Task};
use crate::sysmon::{Snapshot, Sysmon};
use crate::{shell, win};
use std::collections::VecDeque;

pub const WIN_W: f32 = 640.0;
pub const WIN_H: f32 = 190.0;
const CX: f32 = WIN_W / 2.0;

const SKY: Color = rgb(0x3f, 0xb4, 0xff);
const BLUE: Color = rgb(0x26, 0x99, 0xff);
const GREEN: Color = rgb(0x2f, 0xd0, 0x6b);
const RED: Color = rgb(0xff, 0x4b, 0x55);
const BATTERY: Color = rgb(0x33, 0xe0, 0x7a);
const VIOLET: Color = rgb(0xa8, 0x7b, 0xff);
const AMBER: Color = rgb(0xff, 0xc4, 0x3d);

/// How long the notch stays open after the cursor leaves it.
const CLOSE_DELAY: Duration = Duration::from_millis(300);
/// How long a reminder holds the notch open on its own.
const ALERT_OPEN: Duration = Duration::from_secs(8);

// ----- shape & spring animation ---------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
struct Shape {
    w: f32,
    h: f32,
    /// Bottom corner radius.
    r: f32,
    /// Radius of the concave ears at the top corners.
    e: f32,
}

const IDLE: Shape = Shape { w: 150.0, h: 30.0, r: 14.0, e: 10.0 };
const MINIMAL: Shape = Shape { w: 120.0, h: 6.0, r: 3.0, e: 4.0 };
const PAGE: Shape = Shape { w: 420.0, h: 140.0, r: 32.0, e: 22.0 };
const WIDE: Shape = Shape { w: 520.0, h: 140.0, r: 32.0, e: 22.0 };
const PEEK: Shape = Shape { w: 310.0, h: 60.0, r: 19.0, e: 13.0 };

/// How long the welcome peek stays before closing.
const WELCOME: Duration = Duration::from_millis(3400);

/// A damped spring (unit mass), stepped with the real frame time. Unlike a fixed curve,
/// it keeps its velocity when the target changes mid-flight, so quick hover in/out
/// never jerks.
#[derive(Clone, Copy, Default)]
struct Spring {
    x: f32,
    v: f32,
}

impl Spring {
    /// `response`: period in seconds (lower is snappier). `damping`: 1 = no overshoot.
    fn step(&mut self, target: f32, dt: f32, response: f32, damping: f32) {
        let tau = std::f32::consts::TAU;
        let k = (tau / response).powi(2);
        let c = 2.0 * damping * tau / response;
        let n = (dt * 960.0).ceil().max(1.0);
        let h = dt / n;
        for _ in 0..n as u32 {
            let a = -k * (self.x - target) - c * self.v;
            self.v += a * h;
            self.x += self.v * h;
        }
    }
}

struct Anim {
    to: Shape,
    s: [Spring; 4],
    last: Instant,
}

impl Anim {
    fn new(at: Shape) -> Self {
        let s = [at.w, at.h, at.r, at.e].map(|x| Spring { x, v: 0.0 });
        Self { to: at, s, last: Instant::now() }
    }

    fn step(&mut self) {
        let dt = self.last.elapsed().as_secs_f32().min(1.0 / 30.0);
        self.last = Instant::now();
        // Opening is a little bouncy, closing settles calmly (like the Dynamic Island).
        let growing = self.to.h > self.s[1].x;
        let (response, damping) = if growing { (0.44, 0.74) } else { (0.38, 0.92) };
        let t = [self.to.w, self.to.h, self.to.r, self.to.e];
        for (s, t) in self.s.iter_mut().zip(t) {
            s.step(t, dt, response, damping);
        }
        if self.done() {
            self.s = t.map(|x| Spring { x, v: 0.0 });
        }
    }

    fn value(&self) -> Shape {
        let [w, h, r, e] = self.s.map(|s| s.x);
        Shape { w, h, r: r.max(0.0), e: e.max(0.0) }
    }

    fn done(&self) -> bool {
        let t = [self.to.w, self.to.h, self.to.r, self.to.e];
        self.s.iter().zip(t).all(|(s, t)| (s.x - t).abs() < 0.05 && s.v.abs() < 1.0)
    }

    fn retarget(&mut self, to: Shape) {
        if to != self.to {
            if self.done() {
                // Coming from rest: don't count the idle time as one huge step.
                self.last = Instant::now();
            }
            self.to = to;
        }
    }
}

// ----- pages, actions, small state ------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Music,
    Pomo,
    Water,
    Shelf,
    Todo,
    System,
}

const PAGES: [Page; 6] = [Page::Music, Page::Pomo, Page::Water, Page::Shelf, Page::Todo, Page::System];

impl Page {
    fn index(self) -> usize {
        PAGES.iter().position(|p| *p == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Action {
    PagePrev,
    PageNext,
    Page(usize),
    PlayPause,
    Previous,
    Next,
    Lyrics,
    /// Absolute x of the bar and its width.
    SeekBar(f32, f32),
    SeekTo(i64),
    PomoToggle,
    PomoReset,
    PomoMode(bool),
    WaterSwitch,
    Drink,
    WaterSettings(bool),
    WaterSub(usize),
    Bottle(i32),
    Goal(u32),
    FromH(i32),
    ToH(i32),
    WaterReset,
    ShelfOpen(usize),
    ShelfRemove(usize),
    ShelfClear,
    TodoAdd,
    TodoEditMode,
    TodoToggle(usize),
    TodoEdit(usize),
    FormField(bool),
    FormCancel,
    FormSave,
    FormDelete,
    PinOverlay,
}

struct Pomo {
    brk: bool,
    left: Duration,
    ends: Option<Instant>,
}

impl Pomo {
    fn total(brk: bool) -> Duration {
        Duration::from_secs(if brk { 5 * 60 } else { 25 * 60 })
    }

    fn remaining(&self) -> Duration {
        self.ends.map_or(self.left, |e| e.saturating_duration_since(Instant::now()))
    }

    fn running(&self) -> bool {
        self.ends.is_some()
    }

    fn toggle(&mut self) {
        match self.ends.take() {
            Some(e) => self.left = e.saturating_duration_since(Instant::now()),
            None => self.ends = Some(Instant::now() + self.left),
        }
    }

    fn set_mode(&mut self, brk: bool) {
        *self = Pomo { brk, left: Self::total(brk), ends: None };
    }

    /// When a session runs out, flips to the other mode and returns true.
    fn finished(&mut self) -> bool {
        if self.ends.is_some_and(|e| Instant::now() >= e) {
            self.set_mode(!self.brk);
            return true;
        }
        false
    }
}

struct Form {
    editing: Option<usize>,
    title: String,
    /// Up to 8 digits filling "__:__-__:__".
    digits: String,
    time_field: bool,
}

impl Form {
    fn mask(&self) -> String {
        let mut d = self.digits.chars();
        "__:__-__:__".chars().map(|m| if m == '_' { d.next().unwrap_or('_') } else { m }).collect()
    }

    fn time_value(&self) -> String {
        let d = &self.digits;
        let hhmm = |s: &str| {
            let (h, m): (u32, u32) = (s[..2].parse().ok()?, s[2..4].parse().ok()?);
            (h < 24 && m < 60).then(|| format!("{h:02}:{m:02}"))
        };
        match (d.len() >= 4).then(|| hhmm(&d[..4])).flatten() {
            Some(start) if d.len() == 8 => hhmm(&d[4..]).map_or(start.clone(), |end| format!("{start}-{end}")),
            Some(start) => start,
            None => String::new(),
        }
    }
}

struct Stats {
    since: Instant,
    last: Instant,
    frames: u32,
    max_gap: Duration,
    slow: u32,
    max_cpu: Duration,
}

impl Stats {
    fn new(now: Instant) -> Self {
        Self { since: now, last: now, frames: 0, max_gap: Duration::ZERO, slow: 0, max_cpu: Duration::ZERO }
    }
}

struct ShelfItem {
    thumb: Option<Image>,
    size: String,
    generation: u64,
}

// ----- the app --------------------------------------------------------------------------

pub struct App {
    pub hwnd: HWND,
    pub gfx: Gfx,
    /// Window position in physical pixels.
    pub origin: POINT,
    store: Store,
    media: Media,
    sysmon: Sysmon,
    stats_now: Snapshot,
    /// Start of the current on-screen FPS window: (time, screen frames counted so far).
    fps_window: (Instant, u64),
    screen_log: (Instant, u64),
    screen_fps: Option<f32>,
    screen_dirty: Option<(i32, i32, i32, i32)>,
    fps_history: VecDeque<f32>,
    refresh_hz: u32,
    /// Displayed values ease toward the samples: cpu, gpu, ram, fps.
    shown: Cell<[f32; 4]>,
    lyrics: Lyrics,
    track: Option<Track>,
    art: Option<(Arc<Vec<u8>>, Option<(Image, Color)>, u64)>,
    shelf_items: RefCell<HashMap<PathBuf, ShelfItem>>,

    page: Page,
    page_at: Instant,
    lyrics_on: bool,
    water_settings: bool,
    water_sub: usize,
    edit_mode: bool,
    form: Option<Form>,
    todo_scroll: usize,
    shelf_scroll: usize,
    pomo: Pomo,

    open: bool,
    anim: Anim,
    left_at: Option<Instant>,
    alert_until: Option<Instant>,
    cursor: (f32, f32),
    inside: bool,
    file_hover: bool,
    focusable: bool,
    pressed: Option<(Action, f32, f32)>,
    last_wheel: Instant,
    closed_sig: String,
    /// Window region currently set, in physical px [left, top, right, bottom].
    region: [i32; 4],
    /// When the welcome started, and whether it's a "welcome back".
    welcome_at: Option<(Instant, bool)>,
    /// Last time the windows were put back on top, and whether the notch is stepped
    /// aside for a full-screen app.
    top_check: Instant,
    hidden_for_fullscreen: bool,
    stats: Stats,
    frame_start: Instant,
    backdrop_retry: Instant,
    dumped: String,
    t0: Instant,

    regions: RefCell<Vec<(Rect, Action)>>,
    ox: Cell<f32>,
    lyric_pos: Cell<f32>,
    last_draw: Cell<Instant>,
    overlay: Overlay,
}

/// Size of the pinned FPS overlay, in dp.
pub const OV_W: f32 = 176.0;
pub const OV_H: f32 = 62.0;

struct Overlay {
    hwnd: HWND,
    /// Its drawing surface in `Gfx`.
    surface: usize,
    /// Top-left on the desktop, physical px.
    pos: (i32, i32),
    hover: bool,
    dirty: bool,
    /// Which debug dumps were taken (bit 0 plain, bit 1 hovered).
    dumps: u8,
    last: Instant,
}

fn overlay_close_rect() -> Rect {
    Rect::centered(OV_W - 15.0, 15.0, 18.0, 18.0)
}

impl App {
    /// `overlay_hwnd` is the (hidden) FPS overlay window; `overlay_default` is where it
    /// goes the first time (physical px).
    pub fn new(hwnd: HWND, mut gfx: Gfx, origin: POINT, overlay_hwnd: HWND, overlay_default: (i32, i32)) -> Self {
        let mut store = Store::load();
        if store.water.roll_day() {
            store.save();
        }
        let now = Instant::now();
        let overlay_pos = store.settings.overlay_pos.unwrap_or(overlay_default);
        let k = gfx.scale;
        unsafe {
            let _ = SetWindowPos(overlay_hwnd, HWND_TOPMOST, overlay_pos.0, overlay_pos.1, (OV_W * k) as i32, (OV_H * k) as i32, SWP_NOACTIVATE);
            if store.settings.overlay {
                let _ = ShowWindow(overlay_hwnd, SW_SHOWNOACTIVATE);
            }
        }
        let surface = gfx.add_surface(overlay_hwnd, OV_W, OV_H, overlay_pos, false).unwrap_or(gfx::MAIN);
        win::exclude_from_capture(hwnd, store.settings.glass);
        win::exclude_from_capture(overlay_hwnd, store.settings.glass);
        Self {
            overlay: Overlay { hwnd: overlay_hwnd, surface, pos: overlay_pos, hover: false, dirty: true, dumps: 0, last: now },
            hwnd,
            gfx,
            origin,
            store,
            media: Media::start(),
            sysmon: Sysmon::start(),
            stats_now: Snapshot::default(),
            fps_window: (now, 0),
            screen_log: (now, 0),
            screen_fps: None,
            screen_dirty: None,
            fps_history: VecDeque::new(),
            refresh_hz: win::refresh_hz(),
            shown: Cell::new([0.0; 4]),
            lyrics: Lyrics::default(),
            track: None,
            art: None,
            shelf_items: RefCell::new(HashMap::new()),
            page: Page::Music,
            page_at: now,
            lyrics_on: false,
            water_settings: false,
            water_sub: 0,
            edit_mode: false,
            form: None,
            todo_scroll: 0,
            shelf_scroll: 0,
            pomo: Pomo { brk: false, left: Pomo::total(false), ends: None },
            open: false,
            anim: Anim::new(IDLE),
            left_at: None,
            alert_until: None,
            cursor: (-1.0, -1.0),
            inside: false,
            file_hover: false,
            focusable: false,
            pressed: None,
            last_wheel: now,
            closed_sig: String::new(),
            region: [0; 4],
            welcome_at: None,
            top_check: now,
            hidden_for_fullscreen: false,
            stats: Stats::new(now),
            frame_start: now,
            backdrop_retry: now - Duration::from_secs(10),
            dumped: String::new(),
            t0: now,
            regions: RefCell::new(Vec::new()),
            ox: Cell::new(0.0),
            lyric_pos: Cell::new(0.0),
            last_draw: Cell::new(now),
        }
    }

    pub fn minimal(&self) -> bool {
        self.store.settings.minimal
    }

    pub fn set_minimal(&mut self, on: bool) {
        self.store.settings.minimal = on;
        self.store.save();
    }

    pub fn hide_in_fullscreen(&self) -> bool {
        self.store.settings.hide_in_fullscreen
    }

    pub fn set_hide_in_fullscreen(&mut self, on: bool) {
        self.store.settings.hide_in_fullscreen = on;
        self.store.save();
        self.keep_on_top();
    }

    /// Windows sometimes drops a topmost window out of the topmost band (Show desktop,
    /// other always-on-top apps), leaving it under ordinary windows. Called when the
    /// foreground window changes and every couple of seconds: puts the notch and the
    /// overlay back on top, and steps the notch aside while a full-screen app (a game,
    /// a video) is in front. The FPS overlay stays, since that's where it's wanted.
    pub fn keep_on_top(&mut self) {
        self.top_check = Instant::now();
        let fullscreen = self.store.settings.hide_in_fullscreen && foreground_is_fullscreen(&[self.hwnd, self.overlay.hwnd]);
        unsafe {
            if fullscreen != self.hidden_for_fullscreen {
                self.hidden_for_fullscreen = fullscreen;
                let _ = ShowWindow(self.hwnd, if fullscreen { SW_HIDE } else { SW_SHOWNOACTIVATE });
            }
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
            if !fullscreen {
                let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
            }
            if self.store.settings.overlay {
                let _ = SetWindowPos(self.overlay.hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
            }
        }
    }

    /// One pass of the frame loop: polls everything and redraws if needed. Returns how
    /// long the loop may sleep before the next pass, in ms: 0 while something moves (the
    /// vsync-paced present then sets the rhythm).
    pub fn frame(&mut self) -> u32 {
        let now = Instant::now();
        self.frame_start = now;
        if self.store.water.roll_day() {
            self.store.save();
        }
        if self.store.water.reminder_due() {
            self.store.water.last_reminder = store::now_secs();
            self.store.save();
            self.alert(Page::Water);
        }
        self.check_tasks();
        if self.pomo.finished() {
            unsafe {
                let _ = MessageBeep(MB_ICONASTERISK);
            }
            self.alert(Page::Pomo);
        }

        self.track = self.media.current();
        self.sync_art();
        if let Some(t) = &self.track {
            self.lyrics.follow(&t.title, &t.artist, t.end_ms());
        }

        self.anim.step();

        // Where is the cursor relative to the notch?
        if let Some((px, py)) = win::cursor_px() {
            let s = self.gfx.scale;
            self.cursor = ((px - self.origin.x) as f32 / s, (py - self.origin.y) as f32 / s);
        }
        let shape = self.anim.value();
        let notch = Rect::new(CX - shape.w / 2.0, 0.0, shape.w, shape.h);
        self.inside = notch.grow(3.0).contains(self.cursor.0, self.cursor.1.max(0.0));
        let button_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
        // A button held while entering means something is being dragged in (files, usually).
        self.file_hover = self.inside && button_down && self.pressed.is_none() && (self.file_hover || !self.open || self.page == Page::Shelf);
        if self.file_hover && self.page != Page::Shelf {
            self.set_page(Page::Shelf.index());
        }

        let alerting = self.alert_until.is_some_and(|t| now < t);
        if self.inside || self.form.is_some() || alerting {
            self.open = true;
            self.left_at = None;
        } else if self.open {
            let left = *self.left_at.get_or_insert(now);
            if left.elapsed() > CLOSE_DELAY {
                self.open = false;
                self.left_at = None;
                self.edit_mode = false;
            }
        }
        let target = self.target_shape();
        self.anim.retarget(target);

        // Keep the glass fed with what's behind it; retry quietly if capture isn't available.
        let mut backdrop_changed = false;
        if self.store.settings.glass {
            if !self.gfx.has_backdrop() && self.backdrop_retry.elapsed() > Duration::from_secs(3) {
                self.backdrop_retry = now;
                self.gfx.enable_backdrop();
            }
            let changed = self.gfx.update_backdrop();
            backdrop_changed = changed & (1 << gfx::MAIN) != 0;
            if changed & (1 << self.overlay.surface) != 0 {
                self.overlay.dirty = true;
            }
        }

        // Live stats feed the System page and the FPS overlay, and only run while shown.
        let overlay_on = self.store.settings.overlay;
        let on_system = self.open && self.page == Page::System;
        self.sysmon.set_active(on_system || overlay_on);
        if on_system || overlay_on {
            self.stats_now = self.sysmon.get();
            if self.sample_fps() {
                self.overlay.dirty = true;
            }
        }
        if overlay_on {
            let hover = win::cursor_px().is_some_and(|(x, y)| self.overlay_contains(x, y));
            if hover != self.overlay.hover {
                self.overlay.hover = hover;
                self.overlay.dirty = true;
            }
        }

        let animating = !self.anim.done();
        let playing = self.track.as_ref().is_some_and(|t| t.playing);
        let busy = animating || self.open || playing || self.welcoming() || self.file_hover;
        let sig = self.closed_signature();
        if busy || backdrop_changed || sig != self.closed_sig {
            self.closed_sig = sig;
            self.request_dump(animating);
            self.draw();
        }
        // The overlay's numbers change twice a second; its glass at most 30 times a second.
        if overlay_on && self.overlay.dirty && self.overlay.last.elapsed() >= Duration::from_millis(33) {
            self.draw_overlay();
        }
        self.update_region();
        if self.top_check.elapsed() > Duration::from_secs(2) {
            self.keep_on_top();
        }
        self.log_screen();
        if busy { 0 } else { 120 }
    }

    // ----- FPS overlay ------------------------------------------------------------------

    fn overlay_contains(&self, x: i32, y: i32) -> bool {
        let (ox, oy) = self.overlay.pos;
        let k = self.gfx.scale;
        x >= ox && y >= oy && x < ox + (OV_W * k) as i32 && y < oy + (OV_H * k) as i32
    }

    /// Whether a screen point (physical px) is on the overlay's close button. Everything
    /// else on the overlay drags it.
    pub fn overlay_on_close(&self, x: i32, y: i32) -> bool {
        let k = self.gfx.scale;
        let (lx, ly) = ((x - self.overlay.pos.0) as f32 / k, (y - self.overlay.pos.1) as f32 / k);
        self.overlay.hover && overlay_close_rect().contains(lx, ly)
    }

    /// The user finished dragging the overlay: remember where it sits.
    pub fn overlay_moved(&mut self) {
        let mut r = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = GetWindowRect(self.overlay.hwnd, &mut r);
        }
        self.overlay.pos = (r.left, r.top);
        self.store.settings.overlay_pos = Some(self.overlay.pos);
        self.store.save();
        self.gfx.move_surface(self.overlay.surface, self.overlay.pos);
        self.overlay.dirty = true;
    }

    pub fn overlay_shown(&self) -> bool {
        self.store.settings.overlay
    }

    pub fn set_overlay(&mut self, on: bool) {
        self.store.settings.overlay = on;
        self.store.save();
        unsafe {
            let _ = ShowWindow(self.overlay.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        self.overlay.dirty = true;
        self.fps_window = (Instant::now(), self.gfx.screen_frames);
    }

    /// A small glass pane with the on-screen FPS (colored by how smooth it is), its recent
    /// history, and CPU/GPU load. Drawn on its own window, on the same device as the notch.
    fn draw_overlay(&mut self) {
        self.overlay.dirty = false;
        self.overlay.last = Instant::now();
        if self.gfx.begin(self.overlay.surface).is_err() {
            return;
        }
        let g = &self.gfx;
        let pane = Rect::new(1.0, 1.0, OV_W - 2.0, OV_H - 2.0);
        if self.store.settings.glass {
            g.glass_rrect(pane, 18.0);
        } else {
            g.fill_rrect(pane, 18.0, rgba(8, 8, 12, 0.94));
            g.stroke_rrect(pane, 18.0, 1.0, white(0.18), false);
        }
        let c = match self.screen_fps {
            Some(f) if f >= 60.0 => GREEN,
            Some(f) if f >= 30.0 => AMBER,
            Some(_) => RED,
            None => white(0.5),
        };
        g.text("FPS", 14.0, 15.0, 50.0, 9.5, 600, white(0.55), Align::Start);
        let fps = self.screen_fps.map_or_else(|| "—".to_owned(), |f| format!("{f:.0}"));
        g.text(&fps, 13.0, 33.0, 64.0, 24.0, 700, c, Align::Start);
        // Headroom above the display's limit, so a maxed-out line doesn't fill the box.
        let top = self.refresh_hz.max(1) as f32 * 1.25;
        let history: Vec<f32> = self.fps_history.iter().map(|f| f / top).collect();
        g.sparkline(Rect::new(74.0, 9.0, 88.0, 30.0), &history, c);

        let s = &self.stats_now;
        let mut line = format!("CPU {:.0}%", s.cpu * 100.0);
        if let Some(gpu) = s.gpu {
            line += &format!("  ·  GPU {:.0}%", gpu * 100.0);
        }
        if let Some(t) = s.gpu_temp {
            line += &format!("  ·  {t}°");
        }
        g.text(&line, 14.0, 51.0, OV_W - 28.0, 9.5, 400, white(0.62), Align::Start);

        if self.overlay.hover {
            let x = overlay_close_rect();
            g.fill_circle(x.x + x.w / 2.0, x.y + x.h / 2.0, 8.0, rgb(60, 60, 66));
            g.icon('\u{E711}', x.x + x.w / 2.0, x.y + x.h / 2.0 + 0.5, 7.0, WHITE);
        }
        // With HILAL_DUMP set, save one picture of it plain and one hovered.
        if let Some(dir) = std::env::var_os("HILAL_DUMP") {
            let bit = 1 << (self.overlay.hover as u8);
            if self.overlay.dumps & bit == 0 && self.screen_fps.is_some() && self.fps_history.len() > 6 {
                self.overlay.dumps |= bit;
                let name = if self.overlay.hover { "overlay-hover.png" } else { "overlay.png" };
                self.gfx.dump_to = Some(PathBuf::from(dir).join(name));
            }
        }
        if self.gfx.present().is_err() {
            self.overlay.dirty = true;
        }
    }

    /// With HILAL_FPS set, also logs the screen-change rate every second even while the
    /// notch is closed and idle, to tell our own redraws apart from everything else.
    fn log_screen(&mut self) {
        let Some(path) = std::env::var_os("HILAL_FPS") else { return };
        let (t0, f0) = self.screen_log;
        if t0.elapsed() < Duration::from_secs(1) {
            return;
        }
        let f1 = self.gfx.screen_frames;
        let dirty = self.gfx.screen_dirty.take();
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "  [screen] {:.0} changed frames/s  dirty {:?}  open={} presents {}", (f1 - f0) as f32 / t0.elapsed().as_secs_f32(), dirty, self.open, self.gfx.presents);
        }
        self.screen_log = (Instant::now(), f1);
    }

    /// On-screen FPS over half-second windows, from the glass's desktop capture. Returns
    /// true when a new reading came in.
    fn sample_fps(&mut self) -> bool {
        let (t0, f0) = self.fps_window;
        let dt = t0.elapsed().as_secs_f32();
        if dt < 0.5 {
            return false;
        }
        let f1 = self.gfx.screen_frames;
        self.fps_window = (Instant::now(), f1);
        self.screen_dirty = self.gfx.screen_dirty.take();
        // The first window after the page opens spans time it wasn't watching.
        if !self.gfx.has_backdrop() || dt > 1.0 {
            self.screen_fps = if self.gfx.has_backdrop() { self.screen_fps } else { None };
            return true;
        }
        self.screen_fps = Some(((f1 - f0) as f32 / dt).min(self.refresh_hz as f32));
        if let Some(fps) = self.screen_fps {
            self.fps_history.push_back(fps);
            while self.fps_history.len() > 60 {
                self.fps_history.pop_front();
            }
        }
        true
    }

    /// Clicks only land on the notch itself (plus its ears); everywhere else in the
    /// window passes through to whatever is underneath. While animating, the region is
    /// set once to cover the whole motion (with room for overshoot), so it never has to
    /// chase the shape frame by frame.
    fn update_region(&mut self) {
        let cur = self.anim.value();
        let s = if self.anim.done() {
            cur
        } else {
            let t = self.anim.to;
            Shape { w: cur.w.max(t.w) * 1.1, h: cur.h.max(t.h) * 1.1, r: 0.0, e: cur.e.max(t.e) }
        };
        let k = self.gfx.scale;
        let r = [
            ((CX - s.w / 2.0 - s.e) * k).floor().max(0.0) as i32,
            0,
            ((CX + s.w / 2.0 + s.e) * k).ceil().min(WIN_W * k) as i32,
            (s.h * k).ceil().min(WIN_H * k) as i32 + 1,
        ];
        let grows = r[0] < self.region[0] || r[2] > self.region[2] || r[3] > self.region[3];
        if r != self.region && (self.anim.done() || grows) {
            self.region = r;
            unsafe {
                let rgn = CreateRectRgn(r[0], r[1], r[2], r[3]);
                SetWindowRgn(self.hwnd, rgn, false);
            }
        }
    }

    /// With HILAL_DUMP=<dir> set, saves one PNG of each settled view (and one mid-way
    /// through each animation), since the window itself is hidden from screenshots.
    fn request_dump(&mut self, animating: bool) {
        let Some(dir) = std::env::var_os("HILAL_DUMP") else { return };
        let s = self.anim.value();
        let key = if animating {
            let t = self.anim.to;
            let progress = (s.h - 30.0) / (t.h - 30.0).max(1.0);
            if !(0.45..0.6).contains(&progress) {
                return;
            }
            format!("mid-{}x{}", t.w as i32, t.h as i32)
        } else {
            let page = ["music", "pomo", "water", "shelf", "todo", "system"][self.page.index()];
            let what = if !self.open {
                if self.welcoming() { "welcome".to_owned() } else { "closed".to_owned() }
            } else if self.page == Page::Water && self.water_settings {
                "water-settings".to_owned()
            } else if self.page == Page::Music && self.lyrics_on {
                "lyrics".to_owned()
            } else {
                page.to_owned()
            };
            // Live pages need a moment for their numbers to fill in.
            let settle = if self.page == Page::System && self.open { 2500 } else { 300 };
            if self.page_at.elapsed() < Duration::from_millis(settle) {
                return;
            }
            what
        };
        if key != self.dumped {
            self.dumped = key.clone();
            self.gfx.dump_to = Some(PathBuf::from(dir).join(format!("{key}.png")));
        }
    }

    pub fn glass(&self) -> bool {
        self.store.settings.glass
    }

    pub fn set_glass(&mut self, on: bool) {
        self.store.settings.glass = on;
        self.store.save();
        if !on {
            self.gfx.disable_backdrop();
        }
        // Glass windows capture the screen behind themselves, so they must not appear in
        // that capture; plain ones can show up in screenshots again.
        win::exclude_from_capture(self.hwnd, on);
        win::exclude_from_capture(self.overlay.hwnd, on);
        self.backdrop_retry = Instant::now() - Duration::from_secs(10);
        self.closed_sig.clear();
        self.overlay.dirty = true;
    }

    fn welcoming(&self) -> bool {
        self.welcome_at.is_some_and(|(t, _)| t.elapsed() < WELCOME)
    }

    /// Opens the notch a little to say hello: on start, and when the PC is unlocked.
    pub fn welcome(&mut self, back: bool) {
        self.welcome_at = Some((Instant::now(), back));
    }

    fn target_shape(&self) -> Shape {
        if self.open {
            return if self.page == Page::Music && self.lyrics_on { WIDE } else { PAGE };
        }
        if self.welcoming() {
            return PEEK;
        }
        if let Some(line) = self.closed_lyric() {
            let w = (self.gfx.measure(&line, 13.0, 600) + 80.0).clamp(IDLE.w, WIDE.w);
            return Shape { w, ..IDLE };
        }
        if self.minimal() { MINIMAL } else { IDLE }
    }

    /// The closed notch only needs redrawing when what it shows changes.
    fn closed_signature(&self) -> String {
        format!("{}|{:?}|{}|{}", win::clock(), self.closed_lyric(), self.pomo.remaining().as_secs(), self.minimal())
    }

    fn alert(&mut self, page: Page) {
        self.set_page(page.index());
        self.alert_until = Some(Instant::now() + ALERT_OPEN);
    }

    fn check_tasks(&mut self) {
        let today = win::today();
        let now_min = win::hour() as u32 * 60 + win::minute() as u32;
        let mut fired = false;
        for t in &mut self.store.tasks {
            if !t.done && t.alerted != today && t.start_min() == Some(now_min) {
                t.alerted = today.clone();
                fired = true;
            }
        }
        if fired {
            self.store.save();
            unsafe {
                let _ = MessageBeep(MB_ICONASTERISK);
            }
            self.alert(Page::Todo);
        }
    }

    fn set_ex_style(&self, flag: WINDOW_EX_STYLE, on: bool) {
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let ex = if on { ex | flag.0 as isize } else { ex & !(flag.0 as isize) };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex);
        }
    }

    /// The notch normally never takes focus; typing a task needs it.
    fn set_focusable(&mut self, on: bool) {
        if self.focusable == on {
            return;
        }
        self.focusable = on;
        self.set_ex_style(WS_EX_NOACTIVATE, !on);
        if on {
            unsafe {
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(self.hwnd);
            }
        }
    }

    fn set_page(&mut self, i: usize) {
        let page = PAGES[i % PAGES.len()];
        if page != self.page {
            self.page = page;
            self.page_at = Instant::now();
            self.water_settings = false;
            self.edit_mode = false;
            self.close_form();
        }
    }

    fn close_form(&mut self) {
        self.form = None;
        self.set_focusable(false);
    }

    fn sync_art(&mut self) {
        let bytes = self.track.as_ref().and_then(|t| t.art.clone());
        let generation = self.gfx.generation;
        let same = match (&self.art, &bytes) {
            (Some((old, _, g)), Some(new)) => Arc::ptr_eq(old, new) && *g == generation,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.art = bytes.map(|b| {
                let img = decode_art(&b).and_then(|(px, w, h, avg)| Some((self.gfx.image(&px, w, h)?, avg)));
                (b, img, generation)
            });
        }
    }

    fn art(&self) -> Option<&(Image, Color)> {
        self.art.as_ref()?.1.as_ref()
    }

    fn lyric_lines(&self) -> Option<(Vec<(i64, String)>, isize)> {
        let t = self.track.as_ref()?;
        let state = self.lyrics.state.lock().unwrap();
        let lyrics::State::Found(lines) = &*state else { return None };
        let pos = t.position_ms();
        let cur = lines.partition_point(|l| l.ms <= pos) as isize - 1;
        Some((lines.iter().map(|l| (l.ms, l.text.clone())).collect(), cur))
    }

    /// While lyrics are on and a song plays, the closed notch shows the current line.
    fn closed_lyric(&self) -> Option<String> {
        if !self.lyrics_on || !self.track.as_ref().is_some_and(|t| t.playing) {
            return None;
        }
        let state = self.lyrics.state.lock().unwrap();
        let lyrics::State::Found(lines) = &*state else { return None };
        let pos = self.track.as_ref()?.position_ms();
        let cur = lines.partition_point(|l| l.ms <= pos).checked_sub(1)?;
        Some(lines[cur].text.clone()).filter(|s| !s.is_empty())
    }

    // ----- input ------------------------------------------------------------------------

    fn region_at(&self, x: f32, y: f32) -> Option<Action> {
        self.regions.borrow().iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| *a)
    }

    pub fn mouse_down(&mut self, x: f32, y: f32) {
        self.pressed = self.region_at(x, y).map(|a| (a, x, y));
        if self.form.is_some() && self.pressed.is_none() {
            // Clicking empty space inside the notch keeps the form; nothing to do.
        }
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) {
        let Some((Action::ShelfOpen(i), px, py)) = self.pressed else { return };
        if (x - px).hypot(y - py) < 6.0 {
            return;
        }
        self.pressed = None;
        if let Some(path) = self.store.shelf.get(i).cloned() {
            if shell::drag_out(&path) {
                self.store.shelf.retain(|p| *p != path);
                self.store.save();
            }
        }
    }

    pub fn mouse_up(&mut self, x: f32, y: f32) {
        let Some((a, _, _)) = self.pressed.take() else { return };
        if self.region_at(x, y) == Some(a) {
            self.perform(a, x);
            self.draw();
        }
    }

    pub fn wheel(&mut self, delta: i32) {
        if self.form.is_some() || !self.open {
            return;
        }
        let (x, y) = self.cursor;
        let lx = x - (CX - 210.0);
        let down = delta < 0;
        if self.page == Page::Todo && lx > 198.0 && y > 30.0 && self.store.tasks.len() > 3 {
            let max = self.store.tasks.len() - 3;
            self.todo_scroll = if down { (self.todo_scroll + 1).min(max) } else { self.todo_scroll.saturating_sub(1) };
            return;
        }
        if self.page == Page::Shelf && y > 30.0 && y < 120.0 && self.store.shelf.len() > 5 {
            let max = self.store.shelf.len() - 5;
            self.shelf_scroll = if down { (self.shelf_scroll + 1).min(max) } else { self.shelf_scroll.saturating_sub(1) };
            return;
        }
        if self.last_wheel.elapsed() > Duration::from_millis(180) {
            self.last_wheel = Instant::now();
            self.perform(if down { Action::PageNext } else { Action::PagePrev }, 0.0);
        }
    }

    pub fn key(&mut self, vk: u16) {
        const BACK: u16 = 0x08;
        const TAB: u16 = 0x09;
        const ENTER: u16 = 0x0D;
        const ESC: u16 = 0x1B;
        const LEFT: u16 = 0x25;
        const RIGHT: u16 = 0x27;
        match (&mut self.form, vk) {
            (Some(f), BACK) => {
                if f.time_field {
                    f.digits.pop();
                } else {
                    f.title.pop();
                }
            }
            (Some(f), TAB) => f.time_field = !f.time_field,
            (Some(_), ENTER) => self.perform(Action::FormSave, 0.0),
            (Some(_), ESC) => self.close_form(),
            (None, LEFT) => self.perform(Action::PagePrev, 0.0),
            (None, RIGHT) => self.perform(Action::PageNext, 0.0),
            _ => {}
        }
    }

    pub fn char(&mut self, c: char) {
        let Some(f) = &mut self.form else { return };
        if c.is_control() {
            return;
        }
        if f.time_field {
            if c.is_ascii_digit() && f.digits.len() < 8 {
                f.digits.push(c);
            }
        } else if f.title.chars().count() < 80 {
            f.title.push(c);
        }
    }

    /// The window lost focus: drop an untouched new-task form so the notch can close.
    pub fn deactivated(&mut self) {
        if self.form.as_ref().is_some_and(|f| f.title.trim().is_empty()) {
            self.close_form();
        }
    }

    pub fn drop_files(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            if !self.store.shelf.contains(&p) {
                self.store.shelf.push(p);
            }
        }
        self.store.save();
        self.file_hover = false;
        self.open = true;
        self.left_at = None;
        self.set_page(Page::Shelf.index());
    }

    fn perform(&mut self, a: Action, x: f32) {
        use Action::*;
        let idx = self.page.index();
        let w = &mut self.store.water;
        let mut save = true;
        match a {
            PagePrev => self.set_page(idx + PAGES.len() - 1),
            PageNext => self.set_page(idx + 1),
            Page(i) => self.set_page(i),
            PlayPause => self.media.send(Command::PlayPause),
            Previous => self.media.send(Command::Previous),
            Next => self.media.send(Command::Next),
            Lyrics => self.lyrics_on = !self.lyrics_on,
            SeekBar(x0, bw) => {
                if let Some(t) = &self.track {
                    let f = ((x - x0) / bw).clamp(0.0, 1.0);
                    self.media.send(Command::Seek((f * t.end_ms() as f32) as i64));
                }
            }
            SeekTo(ms) => self.media.send(Command::Seek(ms)),
            PomoToggle => self.pomo.toggle(),
            PomoReset => self.pomo.set_mode(self.pomo.brk),
            PomoMode(brk) => self.pomo.set_mode(brk),
            WaterSwitch => w.enabled = !w.enabled,
            Drink if w.enabled => {
                w.drink();
                self.alert_until = None;
            }
            Drink => {}
            WaterSettings(on) => self.water_settings = on,
            WaterSub(i) => self.water_sub = i,
            Bottle(d) => w.bottle_ml = (w.bottle_ml as i32 + d * 50).clamp(100, 2000) as u32,
            Goal(ml) => w.goal_ml = ml,
            FromH(d) => w.from_h = (w.from_h as i32 + 24 + d) as u32 % 24,
            ToH(d) => w.to_h = (w.to_h as i32 + 24 + d) as u32 % 24,
            WaterReset => w.reset(),
            ShelfOpen(i) => {
                if let Some(p) = self.store.shelf.get(i) {
                    shell::open(p);
                }
            }
            ShelfRemove(i) => {
                if i < self.store.shelf.len() {
                    self.store.shelf.remove(i);
                    self.shelf_scroll = self.shelf_scroll.min(self.store.shelf.len().saturating_sub(5));
                }
            }
            ShelfClear => {
                self.store.shelf.clear();
                self.shelf_scroll = 0;
            }
            TodoAdd => {
                self.form = Some(Form { editing: None, title: String::new(), digits: String::new(), time_field: false });
                self.set_focusable(true);
            }
            TodoEditMode => self.edit_mode = !self.edit_mode && !self.store.tasks.is_empty(),
            TodoToggle(i) => {
                if let Some(t) = self.store.tasks.get_mut(i) {
                    t.done = !t.done;
                }
            }
            TodoEdit(i) => {
                if let Some(t) = self.store.tasks.get(i) {
                    let digits = t.time.chars().filter(|c| c.is_ascii_digit()).collect();
                    self.form = Some(Form { editing: Some(i), title: t.text.clone(), digits, time_field: false });
                    self.set_focusable(true);
                }
            }
            FormField(time) => {
                if let Some(f) = &mut self.form {
                    f.time_field = time;
                }
                save = false;
            }
            FormCancel => {
                self.close_form();
                save = false;
            }
            FormSave => {
                let Some(f) = self.form.take() else { return };
                let text = f.title.trim().to_owned();
                if !text.is_empty() {
                    let time = f.time_value();
                    match f.editing.and_then(|i| self.store.tasks.get_mut(i)) {
                        Some(t) => {
                            t.text = text;
                            t.time = time;
                            t.alerted.clear();
                        }
                        None => self.store.tasks.push(Task { text, time, ..Default::default() }),
                    }
                }
                self.set_focusable(false);
                self.edit_mode = false;
            }
            PinOverlay => {
                let on = !self.store.settings.overlay;
                self.set_overlay(on);
                save = false;
            }
            FormDelete => {
                if let Some(i) = self.form.take().and_then(|f| f.editing) {
                    if i < self.store.tasks.len() {
                        self.store.tasks.remove(i);
                    }
                }
                self.set_focusable(false);
                self.edit_mode = false;
                self.todo_scroll = 0;
            }
        }
        if save {
            self.store.save();
        }
    }

    // ----- drawing ----------------------------------------------------------------------

    fn hit(&self, r: Rect, a: Action) {
        if self.gfx.fade.get() > 0.5 {
            let ox = self.ox.get();
            self.regions.borrow_mut().push((Rect::new(r.x + ox, r.y, r.w, r.h), a));
        }
    }

    fn hovered(&self, r: Rect) -> bool {
        r.contains(self.cursor.0 - self.ox.get(), self.cursor.1)
    }

    pub fn draw(&mut self) {
        let started = Instant::now();
        if self.gfx.begin(gfx::MAIN).is_err() {
            return;
        }
        self.regions.borrow_mut().clear();
        let s = self.anim.value();
        let g = &self.gfx;
        let notch = Rect::new(CX - s.w / 2.0, 0.0, s.w, s.h);
        if self.store.settings.glass {
            g.glass(CX, s.w, s.h, s.r, s.e);
        } else {
            g.notch(CX, s.w, s.h, s.r, s.e, BLACK);
        }
        g.clip(Rect::new(notch.x - s.e, 0.0, s.w + 2.0 * s.e, s.h));

        let open_a = ((s.h - 70.0) / 50.0).clamp(0.0, 1.0);
        let closed_a = ((44.0 - s.h) / 12.0).clamp(0.0, 1.0);
        if open_a > 0.0 {
            let page_a = (self.page_at.elapsed().as_secs_f32() / 0.22).min(1.0);
            g.fade.set(open_a * page_a);
            // Content grows out of the top edge with the glass instead of being uncovered.
            let k = 0.9 + 0.1 * open_a;
            if self.page == Page::Music && self.lyrics_on && self.anim.to == WIDE {
                self.ox.set(CX - 260.0);
                g.transform(self.ox.get(), k, CX);
                self.lyrics_page();
            } else {
                self.ox.set(CX - 210.0);
                g.transform(self.ox.get(), k, CX);
                self.page_content();
            }
        }
        if closed_a > 0.0 && s.h > 12.0 {
            g.fade.set(closed_a);
            self.ox.set(0.0);
            g.offset(0.0);
            self.closed(s);
        }
        if self.anim.to == PEEK || (self.welcoming() && open_a == 0.0) {
            let a = ((s.h - 34.0) / 20.0).clamp(0.0, 1.0);
            if a > 0.0 {
                g.fade.set(a);
                self.ox.set(CX - PEEK.w / 2.0);
                g.offset(self.ox.get());
                self.welcome_view();
            }
        }
        g.offset(0.0);
        g.unclip();
        self.last_draw.set(Instant::now());
        let cpu = self.frame_start.elapsed().max(started.elapsed());
        let _ = self.gfx.present();
        self.count_frame(cpu);
    }

    /// With HILAL_FPS=<file> set, appends per-second frame stats to that file: frames,
    /// the longest gap between two presents, how many gaps ran over 1.5 refresh periods,
    /// and the longest CPU time spent building a frame.
    fn count_frame(&mut self, cpu: Duration) {
        let Some(path) = std::env::var_os("HILAL_FPS") else { return };
        let now = Instant::now();
        let gap = now - self.stats.last;
        self.stats.last = now;
        if gap < Duration::from_millis(100) {
            self.stats.frames += 1;
            self.stats.max_gap = self.stats.max_gap.max(gap);
            self.stats.slow += (gap > Duration::from_micros(6250)) as u32;
        }
        self.stats.max_cpu = self.stats.max_cpu.max(cpu);
        if self.stats.since.elapsed() >= Duration::from_secs(1) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let s = &self.stats;
                let _ = writeln!(
                    f,
                    "{:3} fps  max gap {:5.2} ms  slow {:3}  max cpu {:5.2} ms  open={}  screen fps {:?} dirty {:?}  cpu {:.2} ready {}",
                    s.frames,
                    s.max_gap.as_secs_f32() * 1000.0,
                    s.slow,
                    s.max_cpu.as_secs_f32() * 1000.0,
                    self.open,
                    self.screen_fps.map(|f| f.round()),
                    self.screen_dirty,
                    self.stats_now.cpu,
                    self.stats_now.ready
                );
            }
            self.stats = Stats::new(now);
        }
    }

    /// Mascot eyes that glow, blink, then smile, next to a greeting.
    fn welcome_view(&self) {
        let g = &self.gfx;
        let Some((start, back)) = self.welcome_at else { return };
        let t = start.elapsed().as_secs_f32() - 0.25;
        for cx in [44.0, 64.0] {
            let cy = 30.0;
            g.radial(cx, cy, 14.0, 14.0, rgba(80, 140, 255, 0.45));
            if t > 1.7 {
                g.arc(cx, cy + 2.5, 4.6, 270.0, 180.0, 2.6, WHITE);
            } else {
                let blink = if (0.9..1.1).contains(&t) { 1.0 - 0.9 * (std::f32::consts::PI * (t - 0.9) / 0.2).sin() } else { 1.0 };
                g.fill_ellipse(cx, cy, 4.2, 6.2 * blink, WHITE);
            }
        }
        let title = if back { "Welcome back" } else { "Welcome" };
        g.text(title, 86.0, 23.0, 200.0, 15.0, 700, WHITE, Align::Start);
        let greeting = match win::hour() {
            5..=11 => "Good morning",
            12..=16 => "Good afternoon",
            17..=21 => "Good evening",
            _ => "Good night",
        };
        g.text(&format!("{greeting} · {}", win::clock()), 86.0, 41.0, 200.0, 12.0, 400, white(0.55), Align::Start);
    }

    fn page_content(&self) {
        match self.page {
            Page::Music => self.music_page(),
            Page::Pomo => self.pomo_page(),
            Page::Water if self.water_settings => self.water_settings_page(),
            Page::Water => self.water_page(),
            Page::Shelf => self.shelf_page(),
            Page::Todo => self.todo_page(),
            Page::System => self.system_page(),
        }
        if !(self.page == Page::Water && self.water_settings) {
            self.nav();
        }
    }

    // ----- shared chrome ----------------------------------------------------------------

    fn header(&self, title: &str, accent: Option<(&str, Color)>) {
        let g = &self.gfx;
        if !title.is_empty() {
            g.text(title, 15.0, 19.0, 200.0, 15.0, 600, WHITE, Align::Start);
            let x = 15.0 + g.measure(title, 15.0, 600);
            if let Some((word, c)) = accent {
                g.text(word, x, 19.0, 200.0, 15.0, 600, c, Align::Start);
            }
        }
        self.clock_group(216.0, 19.0);
    }

    fn clock_group(&self, cx: f32, cy: f32) {
        let g = &self.gfx;
        let (date, time) = (win::date_long(), win::clock());
        let dw = g.measure(&date, 13.0, 400);
        let tw = g.measure(&time, 13.0, 700);
        let battery = win::battery();
        let ring = if battery.is_some() { 21.0 } else { 0.0 };
        let mut x = cx - (ring + dw + 6.0 + tw) / 2.0;
        if let Some((pct, charging)) = battery {
            g.stroke_circle(x + 7.5, cy, 6.0, 2.2, rgba(0x33, 0xe0, 0x7a, 0.28));
            g.arc(x + 7.5, cy, 6.0, 0.0, pct as f32 * 3.6, 2.2, BATTERY);
            if charging {
                let (bx, by) = (x + 7.5, cy);
                g.fill_poly(
                    &[(bx + 1.2, by - 4.2), (bx - 2.4, by + 0.6), (bx - 0.2, by + 0.6), (bx - 1.2, by + 4.2), (bx + 2.4, by - 0.6), (bx + 0.2, by - 0.6)],
                    WHITE,
                );
            }
            x += ring;
        }
        g.text(&date, x, cy, dw + 2.0, 13.0, 400, WHITE, Align::Start);
        g.text(&time, x + dw + 6.0, cy, tw + 2.0, 13.0, 700, WHITE, Align::Start);
    }

    fn nav(&self) {
        let g = &self.gfx;
        for (cx, glyph, a) in [(22.0, '\u{E76B}', Action::PagePrev), (398.0, '\u{E76C}', Action::PageNext)] {
            let r = Rect::centered(cx, 69.0, 24.0, 24.0);
            g.fill_circle(cx, 69.0, 11.0, white(if self.hovered(r) { 0.4 } else { 0.28 }));
            g.icon(glyph, cx, 69.5, 9.0, WHITE);
            self.hit(r, a);
        }
        let active = self.page.index();
        let widths: Vec<f32> = (0..PAGES.len()).map(|i| if i == active { 15.0 } else { 3.6 }).collect();
        let total: f32 = widths.iter().sum::<f32>() + 4.5 * (PAGES.len() - 1) as f32;
        let mut x = 210.0 - total / 2.0;
        for (i, w) in widths.into_iter().enumerate() {
            g.fill_rrect(Rect::new(x, 131.2, w, 3.6), 1.8, if i == active { WHITE } else { white(0.45) });
            self.hit(Rect::new(x - 2.0, 125.0, w + 4.0, 14.0), Action::Page(i));
            x += w + 4.5;
        }
    }

    fn pill(&self, r: Rect, fill: Color, icon: Ico, label: &str, label_c: Color, a: Option<Action>) {
        let g = &self.gfx;
        let hover = a.is_some() && self.hovered(r);
        let fill = if hover { [fill[0], fill[1], fill[2], (fill[3] * 1.4).min(1.0)] } else { fill };
        g.fill_rrect(r, r.h / 2.0, fill);
        let tw = if label.is_empty() { 0.0 } else { g.measure(label, 12.0, 400) };
        let iw = if icon == Ico::None { 0.0 } else { 13.0 };
        let gap = if iw > 0.0 && tw > 0.0 { 6.0 } else { 0.0 };
        let x = r.x + (r.w - iw - gap - tw) / 2.0;
        let cy = r.y + r.h / 2.0;
        self.draw_ico(icon, x + iw / 2.0, cy, label_c);
        if tw > 0.0 {
            g.text(label, x + iw + gap, cy, tw + 2.0, 12.0, 400, label_c, Align::Start);
        }
        if let Some(a) = a {
            self.hit(r, a);
        }
    }

    fn draw_ico(&self, icon: Ico, cx: f32, cy: f32, c: Color) {
        let g = &self.gfx;
        match icon {
            Ico::None => {}
            Ico::Glyph(ch) => g.icon(ch, cx, cy + 0.5, 12.0, c),
            Ico::Drop => {
                g.fill_circle(cx, cy + 1.8, 4.2, c);
                g.fill_poly(&[(cx, cy - 6.0), (cx + 4.0, cy + 0.9), (cx - 4.0, cy + 0.9)], c);
            }
            Ico::Target => {
                g.stroke_circle(cx, cy, 5.5, 1.5, c);
                g.fill_circle(cx, cy, 2.0, c);
            }
            Ico::Play => g.fill_poly(&[(cx - 4.0, cy - 6.0), (cx + 6.0, cy), (cx - 4.0, cy + 6.0)], c),
            Ico::Pause => {
                g.fill_rrect(Rect::new(cx - 5.0, cy - 6.0, 3.6, 12.0), 1.2, c);
                g.fill_rrect(Rect::new(cx + 1.4, cy - 6.0, 3.6, 12.0), 1.2, c);
            }
            Ico::Plus => {
                g.fill_circle(cx, cy, 7.0, GREEN);
                g.fill_rrect(Rect::centered(cx, cy, 7.5, 1.6), 0.8, WHITE);
                g.fill_rrect(Rect::centered(cx, cy, 1.6, 7.5), 0.8, WHITE);
            }
        }
    }

    // ----- closed notch -----------------------------------------------------------------

    fn closed(&self, s: Shape) {
        let g = &self.gfx;
        let x0 = CX - s.w / 2.0;
        let cy = s.h / 2.0;
        let line = self.closed_lyric();
        if let Some(t) = self.track.as_ref().filter(|t| t.playing) {
            if let Some((img, _)) = self.art() {
                g.draw_image(img, Rect::new(x0 + 10.0, cy - 10.0, 20.0, 20.0), 5.0, 1.0);
            } else {
                g.fill_rrect(Rect::new(x0 + 10.0, cy - 10.0, 20.0, 20.0), 5.0, white(0.15));
            }
            self.bars(x0 + s.w - 30.0, cy);
            if let Some(line) = line {
                g.text(&line, x0 + 38.0, cy, s.w - 76.0, 13.0, 600, WHITE, Align::Center);
            } else if s.w > IDLE.w + 20.0 {
                g.text(&t.title, x0 + 38.0, cy, s.w - 76.0, 12.0, 600, white(0.85), Align::Center);
            }
        } else if self.pomo.running() {
            let left = self.pomo.remaining();
            let frac = left.as_secs_f32() / Pomo::total(self.pomo.brk).as_secs_f32();
            g.stroke_circle(x0 + 20.0, cy, 6.0, 2.2, rgba(0xff, 0x4b, 0x55, 0.3));
            g.arc(x0 + 20.0, cy, 6.0, 0.0, frac * 360.0, 2.2, RED);
            g.text(&mmss(left.as_millis() as i64), x0 + s.w - 70.0, cy, 56.0, 13.0, 600, WHITE, Align::End);
        } else if !self.minimal() {
            g.text(&win::clock(), x0, cy, s.w, 13.0, 600, white(0.9), Align::Center);
        }
    }

    fn bars(&self, x: f32, cy: f32) {
        let t = self.t0.elapsed().as_secs_f32();
        for i in 0..5 {
            let k = 0.5 + 0.5 * (t * 7.0 + i as f32 * 1.3).sin() * (t * 3.1 + i as f32).cos();
            let h = 14.0 * (0.3 + 0.7 * k);
            self.gfx.fill_rrect(Rect::new(x + i as f32 * 4.2, cy - h / 2.0, 2.2, h), 1.1, WHITE);
        }
    }

    // ----- music & lyrics ---------------------------------------------------------------

    fn music_page(&self) {
        self.header("", None);
        self.player(&NORMAL);
    }

    fn lyrics_page(&self) {
        let g = &self.gfx;
        if let Some((_, c)) = self.art() {
            g.linear(
                Rect::new(0.0, 20.0, 520.0, 120.0),
                32.0,
                &[(0.0, [c[0], c[1], c[2], 0.0]), (0.55, [c[0], c[1], c[2], 0.35]), (1.0, [c[0], c[1], c[2], 0.9])],
                true,
            );
        }
        self.player(&SPLIT);
        g.line(252.0, 22.0, 252.0, 118.0, 1.0, white(0.12));
        let center = 64.0;
        let Some((lines, cur)) = self.lyric_lines() else {
            let msg = match &*self.lyrics.state.lock().unwrap() {
                lyrics::State::Missing => "Lyrics not available",
                _ if self.track.is_none() => "Nothing playing",
                _ => "Looking for lyrics…",
            };
            g.text(msg, 264.0, 70.0, 234.0, 13.0, 600, white(0.45), Align::Center);
            return;
        };
        let target = cur.max(0) as f32;
        // Frame-rate independent easing toward the current line.
        let dt = self.last_draw.get().elapsed().as_secs_f32().min(0.05);
        let mut lp = self.lyric_pos.get();
        lp = if (target - lp).abs() > 6.0 { target } else { lp + (target - lp) * (1.0 - (-dt * 11.0).exp()) };
        self.lyric_pos.set(lp);
        g.clip(Rect::new(264.0, 8.0, 234.0, 128.0));
        let base = g.fade.get();
        for i in (cur - 3).max(0)..=cur + 4 {
            let Some((ms, text)) = lines.get(i as usize) else { continue };
            let y = center + (i as f32 - lp) * 29.0;
            let fade = (1.0 - ((y - center).abs() - 34.0) / 40.0).clamp(0.0, 1.0);
            g.fade.set(base * fade);
            let on = i == cur;
            let text = if text.is_empty() { "♪" } else { text.as_str() };
            g.text(text, 264.0, y, 234.0, if on { 16.0 } else { 14.9 }, 700, if on { WHITE } else { white(0.3) }, Align::Start);
            self.hit(Rect::new(264.0, y - 14.0, 234.0, 28.0), Action::SeekTo(*ms));
        }
        g.fade.set(base);
        g.unclip();
    }

    fn player(&self, l: &PlayerLayout) {
        let g = &self.gfx;
        let t = self.track.as_ref();
        match self.art() {
            Some((img, _)) => g.draw_image(img, l.art, 10.0, 1.0),
            None => {
                g.fill_rrect(l.art, 10.0, white(0.1));
                g.icon('\u{E8D6}', l.art.x + l.art.w / 2.0, l.art.y + l.art.h / 2.0, 22.0, white(0.45));
            }
        }
        let (title, artist) = t.map_or(("Not playing", ""), |t| (t.title.as_str(), t.artist.as_str()));
        g.text(title, l.text_x, 47.0, l.text_w, 15.0, 600, if t.is_some() { WHITE } else { white(0.5) }, Align::Start);
        g.text(artist, l.text_x, 64.0, l.text_w, 12.5, 400, white(0.5), Align::Start);

        let (pos, end) = t.map_or((0, 0), |t| (t.position_ms(), t.end_ms()));
        let frac = if end > 0 { (pos as f32 / end as f32).clamp(0.0, 1.0) } else { 0.0 };
        g.text(&mmss(pos), l.el_x, 92.0, 50.0, 12.0, 400, white(0.85), Align::Start);
        g.text(&format!("-{}", mmss(end - pos)), l.rem_r - 60.0, 92.0, 60.0, 12.0, 400, white(0.85), Align::End);
        let bar = Rect::new(l.bar_x, 90.0, l.bar_w, 4.0);
        g.fill_rrect(bar, 2.0, white(0.22));
        g.fill_rrect(Rect::new(bar.x, bar.y, bar.w * frac, 4.0), 2.0, WHITE);
        self.hit(Rect::new(bar.x, 83.0, bar.w, 18.0), Action::SeekBar(self.ox.get() + bar.x, bar.w));

        let cy = 117.0;
        let c = if t.is_some() { WHITE } else { white(0.4) };
        // Lyrics toggle: a speech bubble, filled while lyrics are on.
        let b = Rect::centered(l.bubble, cy - 1.0, 17.0, 12.5);
        if self.lyrics_on {
            g.fill_rrect(b, 3.5, WHITE);
            g.fill_poly(&[(b.x + 4.0, b.y + b.h - 0.5), (b.x + 4.0, b.y + b.h + 4.0), (b.x + 8.5, b.y + b.h - 0.5)], WHITE);
            for k in [-1.0, 1.0] {
                g.fill_rrect(Rect::centered(l.bubble + k * 3.0, cy - 1.0, 2.0, 2.0), 1.0, BLACK);
            }
        } else {
            g.stroke_rrect(b, 3.5, 1.5, white(0.85), false);
            g.fill_poly(&[(b.x + 4.0, b.y + b.h), (b.x + 4.0, b.y + b.h + 4.0), (b.x + 8.5, b.y + b.h)], white(0.85));
            for k in [-1.0, 1.0] {
                g.fill_circle(l.bubble + k * 3.0, cy - 1.0, 1.1, white(0.85));
            }
        }
        self.hit(b.grow(6.0), Action::Lyrics);

        let tri = |cx: f32, dir: f32| {
            for k in [-1.0f32, 0.0] {
                let x = cx + dir * k * 8.0;
                g.fill_poly(&[(x - dir * 4.0, cy - 5.5), (x + dir * 4.5, cy), (x - dir * 4.0, cy + 5.5)], c);
            }
        };
        tri(l.prev + 4.0, -1.0);
        tri(l.next - 4.0, 1.0);
        let playing = t.is_some_and(|t| t.playing);
        if playing {
            g.fill_rrect(Rect::new(l.play - 6.0, cy - 8.0, 4.4, 16.0), 1.5, c);
            g.fill_rrect(Rect::new(l.play + 1.6, cy - 8.0, 4.4, 16.0), 1.5, c);
        } else {
            g.fill_poly(&[(l.play - 5.0, cy - 8.5), (l.play + 8.0, cy), (l.play - 5.0, cy + 8.5)], c);
        }
        self.hit(Rect::centered(l.prev, cy, 28.0, 24.0), Action::Previous);
        self.hit(Rect::centered(l.play, cy, 28.0, 24.0), Action::PlayPause);
        self.hit(Rect::centered(l.next, cy, 28.0, 24.0), Action::Next);
    }

    // ----- pomodoro ---------------------------------------------------------------------

    fn pomo_page(&self) {
        let g = &self.gfx;
        self.header("Pomodoro", None);
        let (cx, cy) = (80.0, 74.0);
        g.radial(cx, cy, 46.0, 46.0, rgba(0xff, 0x3c, 0x3c, 0.32));
        let total = Pomo::total(self.pomo.brk).as_secs_f32();
        let left = self.pomo.remaining();
        let frac = left.as_secs_f32() / total;
        if self.pomo.running() || frac < 1.0 {
            g.stroke_circle(cx, cy, 28.8, 6.4, rgba(0xff, 0x5a, 0x6e, 0.25));
            g.arc(cx, cy, 28.8, 0.0, frac * 360.0, 6.4, rgb(0xff, 0x4a, 0x5a));
        } else {
            g.stroke_circle(cx, cy, 28.8, 6.4, rgb(0xff, 0x4a, 0x5a));
        }
        g.icon('\u{E916}', cx, cy, 24.0, WHITE);

        g.text(&mmss(left.as_millis() as i64 + 999), 200.0, 51.0, 130.0, 30.0, 700, WHITE, Align::Center);
        let reset = Rect::centered(330.0, 50.0, 22.0, 22.0);
        g.icon('\u{E72C}', 330.0, 50.0, 13.0, white(if self.hovered(reset) { 1.0 } else { 0.8 }));
        self.hit(reset, Action::PomoReset);
        let label = if self.pomo.brk { "Break time" } else { "Focus time" };
        g.text(label, 200.0, 75.0, 130.0, 12.0, 400, white(0.55), Align::Center);

        let sel = |on: bool| white(if on { 0.2 } else { 0.12 });
        self.pill(Rect::new(146.0, 92.0, 84.0, 28.0), sel(self.pomo.brk), Ico::Glyph('\u{EC32}'), "Short Break", WHITE, Some(Action::PomoMode(true)));
        let icon = if self.pomo.running() { Ico::Pause } else { Ico::Play };
        self.pill(Rect::new(239.0, 92.0, 52.0, 28.0), RED, icon, "", WHITE, Some(Action::PomoToggle));
        self.pill(Rect::new(301.0, 92.0, 84.0, 28.0), sel(!self.pomo.brk), Ico::Target, "Focus Time", WHITE, Some(Action::PomoMode(false)));
    }

    // ----- water ------------------------------------------------------------------------

    fn water_page(&self) {
        let g = &self.gfx;
        let w = &self.store.water;
        self.header("Water ", Some(("Reminder", SKY)));
        let frac = w.drunk_ml as f32 / w.goal_ml.max(1) as f32;

        let x = 44.0 + g.text(&liters(w.drunk_ml), 44.0, 48.0, 100.0, 26.0, 700, WHITE, Align::Start);
        g.text(&format!("/ {} L", liters(w.goal_ml)), x + 4.0, 52.0, 80.0, 13.0, 400, white(0.55), Align::Start);

        // Reminder switch with a bell on the knob.
        let sw = Rect::new(162.0, 37.0, 34.0, 20.0);
        g.fill_rrect(sw, 10.0, if w.enabled { BLUE } else { white(0.25) });
        let kx = if w.enabled { sw.x + sw.w - 10.0 } else { sw.x + 10.0 };
        g.fill_circle(kx, sw.y + 10.0, 8.0, WHITE);
        g.icon('\u{EA8F}', kx, sw.y + 10.5, 8.0, if w.enabled { BLUE } else { white(0.4) });
        self.hit(sw.grow(3.0), Action::WaterSwitch);

        let pct = format!("{:.0}%", (frac * 100.0).min(999.0));
        let mut tx = 44.0;
        g.text("Today · ", tx, 71.0, 100.0, 12.0, 400, white(0.55), Align::Start);
        tx += g.measure("Today · ", 12.0, 400);
        g.text(&pct, tx, 71.0, 60.0, 12.0, 700, SKY, Align::Start);
        tx += g.measure(&pct, 12.0, 700);
        let rest = if !w.enabled {
            " · Off".to_owned()
        } else if frac >= 1.0 {
            " · Goal reached".to_owned()
        } else {
            format!(" · Next {}", w.next_reminder())
        };
        g.text(&rest, tx, 71.0, 150.0, 12.0, 400, white(0.55), Align::Start);

        g.fill_rrect(Rect::new(44.0, 80.0, 152.0, 5.0), 2.5, white(0.15));
        if frac > 0.0 {
            g.linear(Rect::new(44.0, 80.0, 152.0 * frac.min(1.0), 5.0), 2.5, &[(0.0, rgb(0x66, 0xdb, 0xff)), (1.0, rgb(0x2d, 0x6b, 0xff))], false);
        }

        let drink = if w.enabled { rgb(0x1d, 0x74, 0xd8) } else { white(0.1) };
        self.pill(Rect::new(44.0, 98.0, 60.0, 26.0), drink, Ico::Drop, "Drink", if w.enabled { WHITE } else { white(0.4) }, Some(Action::Drink));
        self.pill(Rect::new(110.0, 98.0, 74.0, 26.0), white(0.12), Ico::Glyph('\u{E713}'), "Settings", WHITE, Some(Action::WaterSettings(true)));

        g.line(207.0, 34.0, 207.0, 120.0, 1.0, white(0.12));
        for i in 0..2 {
            let r = Rect::centered(360.0 + i as f32 * 8.0, 41.0, 4.0, 4.0);
            g.fill_circle(r.x + 2.0, r.y + 2.0, 2.0, if self.water_sub == i { WHITE } else { white(0.4) });
            self.hit(r.grow(4.0), Action::WaterSub(i));
        }
        if self.water_sub == 0 {
            let (cx, cy) = (293.0, 77.0);
            g.arc(cx, cy, 30.0, 210.0, 300.0, 5.0, white(0.12));
            g.arc(cx, cy, 30.0, 210.0, 300.0 * frac.min(1.0), 5.0, SKY);
            g.text(&format!("{:.0}", (frac * 100.0).min(999.0)), cx - 30.0, cy - 4.0, 60.0, 22.0, 700, WHITE, Align::Center);
            self.draw_ico(Ico::Drop, cx - 5.0, cy + 15.0, SKY);
            g.text("%", cx + 1.0, cy + 15.0, 20.0, 10.0, 600, WHITE, Align::Start);
        } else {
            self.water_chart();
        }
    }

    fn water_chart(&self) {
        let g = &self.gfx;
        let w = &self.store.water;
        let (x0, y0, cw, ch) = (220.0, 56.0, 112.0, 50.0);
        let top = w.goal_ml.max(2000) as f32 * 2.0;
        for (i, label) in [(0.0, liters_ml(top as u32)), (0.5, liters_ml(top as u32 / 2)), (1.0, "0 ml".to_owned())] {
            let y = y0 + ch * i;
            g.line(x0, y, x0 + cw + 6.0, y, 1.0, white(0.14));
            g.text(&label, x0 + cw + 10.0, y, 40.0, 9.5, 400, white(0.5), Align::Start);
        }
        let span = ((w.to_h + 24 - w.from_h) % 24).max(1);
        let step = cw / span as f32;
        for k in 0..span {
            let hour = (w.from_h + k) % 24;
            let ml = w.ml_in_hour(hour) as f32;
            let x = x0 + k as f32 * step + (step - 7.0).max(0.0) / 2.0;
            let h = (ml / top * ch).min(ch);
            g.fill_rrect(Rect::new(x, y0 + ch - h.max(3.0), 7.0_f32.min(step - 1.0), h.max(3.0)), 3.5, if ml > 0.0 { SKY } else { white(0.1) });
            if k == 0 || k == span - 1 {
                g.text(&format!("{hour:02}"), x - 6.0, y0 + ch + 10.0, 20.0, 9.5, 400, white(0.5), Align::Center);
            }
        }
    }

    fn water_settings_page(&self) {
        let g = &self.gfx;
        let w = &self.store.water;
        let back = Rect::centered(30.0, 19.0, 22.0, 22.0);
        g.fill_circle(30.0, 19.0, 11.0, white(if self.hovered(back) { 0.25 } else { 0.15 }));
        g.icon('\u{E72B}', 30.0, 19.5, 10.0, WHITE);
        self.hit(back, Action::WaterSettings(false));
        g.text("Settings", 46.0, 19.0, 100.0, 15.0, 600, WHITE, Align::Start);
        self.clock_group(250.0, 19.0);

        for (label, y) in [("Bottle", 50.0), ("Goal", 80.0), ("Hours", 110.0)] {
            g.text(label, 22.0, y, 60.0, 13.0, 400, WHITE, Align::Start);
        }
        self.stepper(86.0, 162.0, 50.0, &format!("{} ml", w.bottle_ml), Action::Bottle(-1), Action::Bottle(1));
        g.text(&format!("{} ml each", w.bottle_ml), 180.0, 50.0, 110.0, 9.5, 400, white(0.5), Align::Start);

        for (i, ml) in [1500u32, 2000, 2500, 3000].into_iter().enumerate() {
            let r = Rect::new(78.0 + i as f32 * 54.5, 69.0, 48.0, 22.0);
            let on = w.goal_ml == ml;
            g.fill_rrect(r, 11.0, if on { BLUE } else { white(if self.hovered(r) { 0.2 } else { 0.12 }) });
            g.text(&format!("{} L", liters(ml)), r.x, r.y + 11.0, r.w, 12.0, if on { 700 } else { 400 }, WHITE, Align::Center);
            self.hit(r, Action::Goal(ml));
        }

        self.stepper(86.0, 162.0, 110.0, &format!("{:02}:00", w.from_h), Action::FromH(-1), Action::FromH(1));
        g.text("to", 174.0, 110.0, 20.0, 12.0, 400, white(0.55), Align::Center);
        self.stepper(205.0, 282.0, 110.0, &format!("{:02}:00", w.to_h), Action::ToH(-1), Action::ToH(1));

        let card = Rect::new(306.0, 36.0, 92.0, 64.0);
        g.fill_rrect(card, 10.0, white(0.08));
        let n = w.reminders();
        g.text(&n.to_string(), card.x, 52.0, card.w, 20.0, 700, SKY, Align::Center);
        g.text("reminders", card.x, 67.0, card.w, 9.5, 400, white(0.55), Align::Center);
        let refills = format!("{}", n.saturating_sub(1));
        let rw = g.measure(&refills, 13.0, 700) + 4.0 + g.measure("refills", 9.5, 400);
        let rx = card.x + (card.w - rw) / 2.0;
        let used = g.text(&refills, rx, 87.0, 30.0, 13.0, 700, SKY, Align::Start);
        g.text("refills", rx + used + 4.0, 87.5, 60.0, 9.5, 400, white(0.55), Align::Start);

        let reset = Rect::new(306.0, 104.0, 92.0, 18.0);
        g.fill_rrect(reset, 9.0, white(if self.hovered(reset) { 0.16 } else { 0.08 }));
        g.icon('\u{E74D}', reset.x + 33.0, reset.y + 9.5, 9.0, WHITE);
        g.text("Reset", reset.x + 41.0, reset.y + 9.0, 40.0, 10.0, 400, WHITE, Align::Start);
        self.hit(reset, Action::WaterReset);
    }

    fn stepper(&self, minus_x: f32, plus_x: f32, cy: f32, value: &str, minus: Action, plus: Action) {
        let g = &self.gfx;
        for (x, glyph, a) in [(minus_x, '\u{E738}', minus), (plus_x, '\u{E710}', plus)] {
            let r = Rect::centered(x, cy, 22.0, 22.0);
            g.fill_circle(x, cy, 11.0, white(if self.hovered(r) { 0.25 } else { 0.15 }));
            g.icon(glyph, x, cy + 0.5, 9.0, WHITE);
            self.hit(r, a);
        }
        g.text(value, minus_x + 11.0, cy, plus_x - minus_x - 22.0, 13.0, 600, WHITE, Align::Center);
    }

    // ----- shelf ------------------------------------------------------------------------

    fn shelf_page(&self) {
        let g = &self.gfx;
        self.header("Shelf", None);
        let files = &self.store.shelf;
        if !files.is_empty() {
            let r = Rect::new(62.0, 11.0, 44.0, 16.0);
            g.fill_rrect(r, 8.0, white(if self.hovered(r) { 0.22 } else { 0.13 }));
            g.text("Clear", r.x, r.y + 8.0, r.w, 9.5, 400, white(0.85), Align::Center);
            self.hit(r, Action::ShelfClear);
        }
        let zone = Rect::new(44.0, 33.0, 336.0, 90.0);
        if files.is_empty() || self.file_hover {
            let hot = self.file_hover;
            let fill = if !files.is_empty() { rgba(10, 14, 24, 0.85) } else { white(if hot { 0.08 } else { 0.03 }) };
            if files.is_empty() || hot {
                self.cards();
            }
            g.fill_rrect(zone, 16.0, fill);
            g.stroke_rrect(zone, 16.0, 1.4, white(if hot { 0.8 } else { 0.3 }), true);
            let bob = if hot { 6.0 + 3.0 * (self.t0.elapsed().as_secs_f32() * 4.5).sin().abs() } else { 0.0 };
            g.icon('\u{E896}', 212.0, 60.0 + bob, 18.0, WHITE);
            g.text(if hot { "Release to add" } else { "Drop files here" }, zone.x, 88.0, zone.w, 13.0, 600, WHITE, Align::Center);
            if !hot {
                g.text("Drag out anytime · click to open", zone.x, 106.0, zone.w, 10.0, 400, white(0.5), Align::Center);
            }
            return;
        }
        self.cards();
    }

    fn cards(&self) {
        let g = &self.gfx;
        let mut items = self.shelf_items.borrow_mut();
        let mut loaded = 0;
        for (slot, (i, path)) in self.store.shelf.iter().enumerate().skip(self.shelf_scroll).take(5).enumerate() {
            let cx = 88.0 + slot as f32 * 61.0;
            let stale = items.get(path).is_none_or(|it| it.generation != self.gfx.generation);
            if stale && loaded < 2 {
                loaded += 1;
                let thumb = shell::thumbnail(path, 88).and_then(|(px, w, h)| g.image(&px, w, h));
                items.insert(path.clone(), ShelfItem { thumb, size: describe(path), generation: self.gfx.generation });
            }
            let card = Rect::new(cx - 30.0, 33.0, 60.0, 82.0);
            let hover = self.hovered(card);
            if hover {
                g.fill_rrect(card, 10.0, white(0.08));
            }
            let thumb = Rect::new(cx - 22.0, 37.0, 44.0, 44.0);
            match items.get(path).and_then(|it| it.thumb.as_ref()) {
                Some(img) => {
                    let s = 44.0 / img.w.max(img.h);
                    let r = Rect::centered(cx, 59.0, img.w * s, img.h * s);
                    g.draw_image(img, r, 6.0, 1.0);
                }
                None => g.fill_rrect(thumb, 9.0, white(0.12)),
            }
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            g.text(&name, cx - 29.0, 92.0, 58.0, 10.5, 600, WHITE, Align::Center);
            let size = items.get(path).map_or("", |it| it.size.as_str());
            g.text(size, cx - 29.0, 105.0, 58.0, 9.5, 400, white(0.5), Align::Center);
            self.hit(card, Action::ShelfOpen(i));
            if hover {
                let x = Rect::centered(cx + 22.0, 37.0, 16.0, 16.0);
                g.fill_circle(cx + 22.0, 37.0, 8.0, rgb(60, 60, 66));
                g.icon('\u{E711}', cx + 22.0, 37.5, 7.0, WHITE);
                self.hit(x, Action::ShelfRemove(i));
            }
        }
        if self.store.shelf.len() > 5 {
            let more = format!("{}/{}", (self.shelf_scroll + 5).min(self.store.shelf.len()), self.store.shelf.len());
            g.text(&more, 330.0, 19.0, 50.0, 10.0, 400, white(0.45), Align::End);
        }
    }

    // ----- to-do ------------------------------------------------------------------------

    fn todo_page(&self) {
        let g = &self.gfx;
        let tasks = &self.store.tasks;
        self.header("Todo", Some(("List", GREEN)));
        let done = tasks.iter().filter(|t| t.done).count();
        g.text(&format!("{done}/{}", tasks.len()), 44.0, 48.0, 140.0, 28.0, 700, WHITE, Align::Start);
        g.text("Task Done Today", 44.0, 80.0, 150.0, 12.0, 400, white(0.55), Align::Start);

        let adding = self.form.as_ref().is_some_and(|f| f.editing.is_none());
        self.pill(Rect::new(42.0, 94.0, 74.0, 26.0), if adding { rgba(0x2f, 0xd0, 0x6b, 0.25) } else { white(0.12) }, Ico::Plus, "Add Task", WHITE, Some(Action::TodoAdd));
        let can_edit = !tasks.is_empty();
        let edit_fill = if self.edit_mode { white(0.25) } else { white(if can_edit { 0.12 } else { 0.06 }) };
        self.pill(
            Rect::new(122.0, 94.0, 74.0, 26.0),
            edit_fill,
            Ico::Glyph('\u{E70F}'),
            "Edit Task",
            if can_edit { WHITE } else { white(0.35) },
            can_edit.then_some(Action::TodoEditMode),
        );

        if let Some(f) = &self.form {
            return self.todo_form(f);
        }
        if tasks.is_empty() {
            g.fill_rrect(Rect::new(198.0, 32.0, 185.0, 90.0), 14.0, white(0.08));
            g.fill_rrect(Rect::centered(290.0, 62.0, 30.0, 30.0), 8.0, white(0.12));
            g.icon('\u{E9D5}', 290.0, 62.5, 16.0, white(0.6));
            g.text("No tasks.", 198.0, 96.0, 185.0, 12.0, 400, white(0.55), Align::Center);
            return;
        }
        for (slot, (i, t)) in tasks.iter().enumerate().skip(self.todo_scroll).take(3).enumerate() {
            let row = Rect::new(202.0, 36.0 + slot as f32 * 31.0, 176.0, 28.0);
            let fill = if t.done { rgba(40, 120, 60, 0.38) } else { white(if self.hovered(row) { 0.12 } else { 0.08 }) };
            g.fill_rrect(row, 9.0, fill);
            let (ccx, ccy) = (row.x + 15.0, row.y + 14.0);
            if self.edit_mode {
                g.icon('\u{E70F}', ccx, ccy + 0.5, 10.0, WHITE);
            } else if t.done {
                g.fill_circle(ccx, ccy, 8.0, GREEN);
                g.icon('\u{E73E}', ccx, ccy + 0.5, 9.0, WHITE);
            } else {
                g.stroke_circle(ccx, ccy, 7.3, 1.4, white(0.45));
            }
            let time_w = if t.time.is_empty() { 0.0 } else { 38.0 };
            g.text(&t.text, row.x + 29.0, ccy, row.w - 35.0 - time_w, 12.0, 400, if t.done { white(0.75) } else { WHITE }, Align::Start);
            if t.done {
                let tw = g.measure(&t.text, 12.0, 400).min(row.w - 35.0 - time_w);
                g.line(row.x + 29.0, ccy + 0.5, row.x + 29.0 + tw, ccy + 0.5, 1.2, white(0.75));
            }
            if time_w > 0.0 {
                g.text(&t.time[..5.min(t.time.len())], row.x + row.w - 42.0, ccy, 36.0, 10.0, 400, white(0.5), Align::End);
            }
            self.hit(row, if self.edit_mode { Action::TodoEdit(i) } else { Action::TodoToggle(i) });
        }
        if tasks.len() > 3 {
            let total = tasks.len() as f32;
            let h = 90.0 * 3.0 / total;
            let y = 36.0 + (90.0 - h) * self.todo_scroll as f32 / (total - 3.0);
            g.fill_rrect(Rect::new(381.0, y, 2.5, h), 1.25, white(0.3));
        }
    }

    // ----- system -----------------------------------------------------------------------

    fn system_page(&self) {
        let g = &self.gfx;
        let s = &self.stats_now;
        self.header("System ", Some(("Live", BATTERY)));

        // Displayed values glide toward each new sample instead of jumping.
        let dt = self.last_draw.get().elapsed().as_secs_f32().min(0.1);
        let k = 1.0 - (-dt * 6.0).exp();
        let ram = if s.ram_total > 0 { s.ram_used as f32 / s.ram_total as f32 } else { 0.0 };
        let target = [s.cpu, s.gpu.unwrap_or(0.0), ram, self.screen_fps.unwrap_or(0.0)];
        let mut shown = self.shown.get();
        for (v, t) in shown.iter_mut().zip(target) {
            *v += (t - *v) * k;
        }
        self.shown.set(shown);

        let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
        let gpu_sub = match (s.gpu_temp, s.vram) {
            (Some(t), _) => format!("{t}°C"),
            (None, Some((used, _))) => format!("{:.1} GB", gb(used)),
            _ => "—".to_owned(),
        };
        let gauges = [
            ("CPU", shown[0], format!("{} threads", s.threads), SKY, s.ready),
            ("GPU", shown[1], gpu_sub, VIOLET, s.gpu.is_some()),
            ("RAM", shown[2], format!("{:.1}/{:.0} GB", gb(s.ram_used), gb(s.ram_total)), GREEN, s.ram_total > 0),
        ];
        for (i, (label, v, sub, c, ok)) in gauges.into_iter().enumerate() {
            let (cx, cy) = (74.0 + i as f32 * 60.0, 64.0);
            // The glow behind each ring brightens with load.
            g.radial(cx, cy, 30.0, 30.0, [c[0], c[1], c[2], 0.10 + 0.28 * v.clamp(0.0, 1.0)]);
            g.stroke_circle(cx, cy, 21.0, 5.0, white(0.1));
            if ok {
                g.arc(cx, cy, 21.0, 0.0, v.clamp(0.0, 1.0) * 360.0, 5.0, c);
            }
            let value = if ok { format!("{:.0}%", v * 100.0) } else { "—".to_owned() };
            g.text(&value, cx - 24.0, cy, 48.0, 12.5, 700, WHITE, Align::Center);
            g.text(label, cx - 30.0, 99.0, 60.0, 10.5, 600, white(0.85), Align::Center);
            g.text(&sub, cx - 30.0, 112.0, 60.0, 9.0, 400, white(0.5), Align::Center);
        }
        g.line(232.0, 34.0, 232.0, 120.0, 1.0, white(0.12));

        // What the screen is actually showing per second, against the display's limit.
        let (x, w) = (244.0, 136.0);
        g.text("FPS", x, 38.0, 60.0, 10.0, 600, white(0.55), Align::Start);
        g.text(&format!("{} Hz", self.refresh_hz), x, 38.0, w - 24.0, 10.0, 400, white(0.45), Align::End);
        // Pin: pops the FPS out into a small overlay you can place anywhere on screen.
        let pin = Rect::centered(x + w - 8.0, 38.0, 20.0, 20.0);
        let pinned = self.store.settings.overlay;
        let fill = if pinned { SKY } else { white(if self.hovered(pin) { 0.24 } else { 0.12 }) };
        g.fill_circle(pin.x + 10.0, 38.0, 9.0, fill);
        g.icon(if pinned { '\u{E841}' } else { '\u{E718}' }, pin.x + 10.0, 38.5, 9.0, WHITE);
        self.hit(pin.grow(2.0), Action::PinOverlay);
        let fps = match self.screen_fps {
            Some(_) => format!("{:.0}", shown[3]),
            None => "—".to_owned(),
        };
        let fw = g.measure(&fps, 26.0, 700);
        g.text(&fps, x, 57.0, w, 26.0, 700, WHITE, Align::Start);
        let note = if self.screen_fps.is_some() { "on screen" } else { "unavailable" };
        g.text(note, x + fw + 6.0, 61.0, w - fw - 6.0, 10.0, 400, white(0.5), Align::Start);
        let top = self.refresh_hz.max(1) as f32 * 1.25;
        let history: Vec<f32> = self.fps_history.iter().map(|f| f / top).collect();
        g.fill_rrect(Rect::new(x, 75.0, w, 28.0), 7.0, white(0.04));
        g.sparkline(Rect::new(x + 2.0, 78.0, w - 4.0, 22.0), &history, SKY);

        g.text(&format!("↓ {}", rate(s.net_down)), x, 115.0, w / 2.0, 10.0, 400, white(0.75), Align::Start);
        g.text(&format!("↑ {}", rate(s.net_up)), x + w / 2.0, 115.0, w / 2.0, 10.0, 400, white(0.75), Align::End);
    }

    fn todo_form(&self, f: &Form) {
        let g = &self.gfx;
        let blink = (self.t0.elapsed().as_millis() / 530) % 2 == 0;
        let input_w = if f.editing.is_some() { 142.0 } else { 168.0 };
        let input = Rect::new(202.0, 34.0, input_w, 26.0);
        g.stroke_rrect(input, 8.0, 1.2, white(if f.time_field { 0.3 } else { 0.85 }), false);
        if f.title.is_empty() {
            g.text("Task title", input.x + 10.0, input.y + 13.0, input.w - 20.0, 12.0, 400, white(0.4), Align::Start);
        } else {
            g.text(&f.title, input.x + 10.0, input.y + 13.0, input.w - 20.0, 12.0, 400, WHITE, Align::Start);
        }
        if !f.time_field && blink {
            let tw = g.measure(&f.title, 12.0, 400).min(input.w - 22.0);
            let x = if crate::gfx::is_rtl(&f.title) { input.x + input.w - 10.0 - tw - 2.0 } else { input.x + 10.0 + tw + 1.0 };
            g.line(x, input.y + 7.0, x, input.y + 19.0, 1.2, WHITE);
        }
        self.hit(input, Action::FormField(false));
        if f.editing.is_some() {
            let del = Rect::centered(363.0, 47.0, 22.0, 22.0);
            g.fill_circle(363.0, 47.0, 11.0, rgba(0xff, 0x4b, 0x55, if self.hovered(del) { 0.45 } else { 0.25 }));
            g.icon('\u{E74D}', 363.0, 47.5, 10.0, WHITE);
            self.hit(del, Action::FormDelete);
        }

        let time = Rect::new(202.0, 64.0, 168.0, 24.0);
        g.fill_rrect(time, 8.0, white(if f.time_field { 0.16 } else { 0.08 }));
        if f.time_field {
            g.stroke_rrect(time, 8.0, 1.2, white(0.85), false);
        }
        g.icon('\u{E823}', time.x + 13.0, time.y + 12.5, 11.0, white(0.7));
        let mask = f.mask();
        g.text(&mask, time.x + 26.0, time.y + 12.0, 120.0, 12.0, 400, if f.digits.is_empty() { white(0.45) } else { WHITE }, Align::Start);
        self.hit(time, Action::FormField(true));

        let cancel = Rect::new(202.0, 94.0, 80.0, 24.0);
        let save = Rect::new(289.0, 94.0, 81.0, 24.0);
        g.fill_rrect(cancel, 12.0, white(if self.hovered(cancel) { 0.2 } else { 0.12 }));
        g.text("Cancel", cancel.x, cancel.y + 12.0, cancel.w, 12.0, 400, WHITE, Align::Center);
        g.fill_rrect(save, 12.0, if self.hovered(save) { rgb(0x3a, 0xc8, 0x5c) } else { rgb(0x2f, 0xb5, 0x4f) });
        g.text("Save", save.x, save.y + 12.0, save.w, 12.0, 600, WHITE, Align::Center);
        self.hit(cancel, Action::FormCancel);
        self.hit(save, Action::FormSave);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Ico {
    None,
    Glyph(char),
    Drop,
    Target,
    Play,
    Pause,
    Plus,
}

/// Where the player pieces sit: the normal 420-wide page, or the left half of the lyrics view.
struct PlayerLayout {
    art: Rect,
    text_x: f32,
    text_w: f32,
    el_x: f32,
    bar_x: f32,
    bar_w: f32,
    rem_r: f32,
    bubble: f32,
    prev: f32,
    play: f32,
    next: f32,
}

const NORMAL: PlayerLayout = PlayerLayout {
    art: Rect::new(44.0, 12.0, 64.0, 64.0),
    text_x: 120.0,
    text_w: 256.0,
    el_x: 44.0,
    bar_x: 96.0,
    bar_w: 220.0,
    rem_r: 376.0,
    bubble: 75.0,
    prev: 162.0,
    play: 210.0,
    next: 256.0,
};

const SPLIT: PlayerLayout = PlayerLayout {
    art: Rect::new(22.0, 12.0, 64.0, 64.0),
    text_x: 98.0,
    text_w: 138.0,
    el_x: 25.0,
    bar_x: 76.0,
    bar_w: 104.0,
    rem_r: 236.0,
    bubble: 53.0,
    prev: 91.0,
    play: 132.0,
    next: 170.0,
};

/// Center-crops the cover to a square, scales it for the notch, premultiplies it to BGRA,
/// and returns its average color (for the lyrics glow).
fn decode_art(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32, Color)> {
    let img = image::load_from_memory(bytes).ok()?;
    let side = img.width().min(img.height());
    let square = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let rgba = square.thumbnail(160, 160).to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut sum = [0f64; 3];
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for p in rgba.pixels() {
        let [r, g, b, a] = p.0;
        let k = a as f32 / 255.0;
        bgra.extend_from_slice(&[(b as f32 * k) as u8, (g as f32 * k) as u8, (r as f32 * k) as u8, a]);
        sum[0] += r as f64;
        sum[1] += g as f64;
        sum[2] += b as f64;
    }
    let n = (w * h).max(1) as f64 * 255.0;
    let avg = [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32, 1.0];
    Some((bgra, w, h, brighten(avg)))
}

/// Pushes a muddy average toward a vivid glow color.
fn brighten(c: Color) -> Color {
    let max = c[0].max(c[1]).max(c[2]).max(0.05);
    let k = (0.85 / max).min(3.0);
    [(c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0), 1.0]
}

fn describe(path: &std::path::Path) -> String {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => "Folder".to_owned(),
        Ok(m) => {
            let b = m.len() as f64;
            if b >= 1e6 {
                format!("{:.1} MB", b / 1e6)
            } else {
                format!("{:.0} KB", (b / 1e3).max(1.0))
            }
        }
        Err(_) => "Missing".to_owned(),
    }
}

/// Bytes per second, short: "1.2 MB/s", "84 KB/s".
fn rate(bps: f64) -> String {
    if bps >= 1e6 {
        format!("{:.1} MB/s", bps / 1e6)
    } else if bps >= 1e3 {
        format!("{:.0} KB/s", bps / 1e3)
    } else {
        format!("{bps:.0} B/s")
    }
}

fn mmss(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// 1500 -> "1.5", 2000 -> "2.0".
fn liters(ml: u32) -> String {
    format!("{:.1}", ml as f32 / 1000.0)
}

fn liters_ml(ml: u32) -> String {
    ml.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_overshoots_then_settles() {
        let mut s = Spring::default();
        let mut peak = 0.0f32;
        for _ in 0..240 {
            s.step(1.0, 1.0 / 240.0, 0.44, 0.74);
            peak = peak.max(s.x);
        }
        assert!(peak > 1.0 && peak < 1.2, "peak {peak}");
        assert!((s.x - 1.0).abs() < 0.01 && s.v.abs() < 0.1);
    }

    #[test]
    fn spring_keeps_velocity_on_retarget() {
        let mut s = Spring::default();
        for _ in 0..30 {
            s.step(1.0, 1.0 / 240.0, 0.44, 0.74);
        }
        let v = s.v;
        s.step(0.0, 1.0 / 240.0, 0.38, 0.92);
        // One frame after reversing, it is still moving forward: no instant stop.
        assert!(v > 0.0 && s.v > 0.0);
    }

    #[test]
    fn form_time_parsing() {
        let f = |d: &str| Form { editing: None, title: String::new(), digits: d.into(), time_field: true };
        assert_eq!(f("0930").time_value(), "09:30");
        assert_eq!(f("09301000").time_value(), "09:30-10:00");
        assert_eq!(f("2561").time_value(), "");
        assert_eq!(f("09").mask(), "09:__-__:__");
    }
}

/// True when the foreground window covers its whole monitor (a game, a full-screen video
/// or slideshow). Maximized windows leave the taskbar visible, so they don't count; nor do
/// the desktop or our own windows.
fn foreground_is_fullscreen(ours: &[HWND]) -> bool {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || ours.contains(&fg) {
            return false;
        }
        let mut class = [0u16; 32];
        let n = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
        let mut r = windows::Win32::Foundation::RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            return false;
        }
        let m = info.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}
