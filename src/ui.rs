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
use crate::download::{self, Kind, Phase};
use crate::claude::{self, Session};
use crate::prayer;
use crate::reminders_data::REMINDERS;

mod settings;
mod apps_page;
mod companion;
mod dl_options;
pub use dl_options::{After as DlAfter, DW_H, DW_W};
mod extras;
mod folder_downloads;
mod meetings;
mod slides;
mod toasts;
pub use settings::{After as SettingsAfter, SW_H, SW_W};
use crate::store::{self, Store, Task};
use crate::sysmon::{Snapshot, Sysmon};
use crate::{shell, win};
use std::collections::VecDeque;

pub const WIN_W: f32 = 640.0;
pub const WIN_H: f32 = 280.0;
/// How far down the screen the mouse wakes the notch, and where the FPS overlay starts
/// (the window's old height, before it grew room for the reminder card).
pub const WAKE_H: f32 = 190.0;
/// The notch's middle in the window, where it starts out (at the top center of the screen).
const CX: f32 = WIN_W / 2.0;

/// Moving it (Nova Island's numbers): within this many dp of the top edge it docks there,
/// and docked within this many of the middle it snaps to the middle. Floating, it sits
/// this far down its window. It keeps this far from the screen's sides.
const SNAP_DP: f32 = 28.0;
const CENTER_SNAP_DP: f32 = 40.0;
const FLOAT_TOP: f32 = 8.0;
const PILL_HALF_W: f32 = 120.0;
/// Half the height of the closed notch standing on a side edge.
const V_HALF: f32 = 122.0;

const SKY: Color = rgb(0x3f, 0xb4, 0xff);
const BLUE: Color = rgb(0x26, 0x99, 0xff);
const GREEN: Color = rgb(0x2f, 0xd0, 0x6b);
const RED: Color = rgb(0xff, 0x4b, 0x55);
const BATTERY: Color = rgb(0x33, 0xe0, 0x7a);
const VIOLET: Color = rgb(0xa8, 0x7b, 0xff);
const AMBER: Color = rgb(0xff, 0xc4, 0x3d);
/// Claude's own coral.
const CLAUDE: Color = rgb(0xd9, 0x77, 0x57);

/// How long the notch stays open after the cursor leaves it.
/// Pill buttons: label size, and the room on each side of their content.
const PILL_TEXT: f32 = 11.5;
const PILL_PAD: f32 = 11.0;
/// How long a dhikr, du'a or verse stays.
/// The reminder card: as wide as Nova Island's, and at most this tall.
const REMINDER_W: f32 = 500.0;
const REMINDER_W_SIDE: f32 = 340.0;
const REMINDER_MAX_H: f32 = 270.0;
/// How long a prayer time stays in the notch if nobody dismisses it.
/// Egypt's cities come first in `prayer::CITIES`.
const EGYPT_CITIES: usize = 33;
const PRAYER_SHOWN: Duration = Duration::from_secs(180);
/// The heads-up before a prayer: how many minutes before, and how long it stays.
const PRAYER_HEADSUP: i32 = 10;
const HEADSUP_SHOWN: Duration = Duration::from_secs(5);
/// How long the mouse rests on the thin line (over a maximized window) before it opens.
const HOVER_REST: Duration = Duration::from_millis(250);
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
/// Closed, standing on a side edge: tall, or shorter with music or a timer on.
const V_IDLE: Shape = Shape { w: 44.0, h: 244.0, r: 22.0, e: 0.0 };
const V_COMPACT: Shape = Shape { w: 44.0, h: 136.0, r: 22.0, e: 0.0 };
const V_MINIMAL: Shape = Shape { w: 6.0, h: 120.0, r: 3.0, e: 0.0 };

/// How long the welcome peek stays before closing.
const WELCOME: Duration = Duration::from_millis(3400);

/// Where the notch is placed (see `App::pos`).
#[derive(Clone, Copy)]
struct Pos {
    anchor: Option<(i32, i32)>,
    docked: bool,
    vertical: bool,
    side_left: bool,
}

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
    /// Straight to where it's going (with animations off).
    fn snap(&mut self) {
        self.s = [self.to.w, self.to.h, self.to.r, self.to.e].map(|x| Spring { x, v: 0.0 });
    }

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
    Prayer,
    Download,
    Claude,
    Shelf,
    Todo,
    System,
    /// Nova mode: the connection, who's using the mic and camera, and the clipboard history.
    Clipboard,
    /// Nova mode: pinned apps to start or bring to the front.
    Apps,
    /// Nova mode: live Qur'an stations.
    Radio,
}

/// Classic Hilal's pages, in order.
const PAGES: [Page; 9] =
    [Page::Music, Page::Pomo, Page::Water, Page::Prayer, Page::Download, Page::Claude, Page::Shelf, Page::Todo, Page::System];
/// Nova mode's (each can be switched off).
const NOVA_PAGES: [Page; 12] = [
    Page::Music, Page::Apps, Page::Pomo, Page::Water, Page::Prayer, Page::Radio, Page::Download, Page::Claude, Page::Shelf, Page::Todo, Page::System, Page::Clipboard,
];

impl Page {
    /// Its name, for the settings and the test pictures.
    fn name(self) -> &'static str {
        match self {
            Page::Music => "music",
            Page::Pomo => "pomo",
            Page::Water => "water",
            Page::Prayer => "prayer",
            Page::Download => "download",
            Page::Claude => "claude",
            Page::Shelf => "shelf",
            Page::Todo => "todo",
            Page::System => "system",
            Page::Clipboard => "clipboard",
            Page::Apps => "apps",
            Page::Radio => "radio",
        }
    }

    /// What Settings calls it.
    fn title(self) -> &'static str {
        match self {
            Page::Music => "Music",
            Page::Pomo => "Pomodoro",
            Page::Water => "Water",
            Page::Prayer => "Prayer",
            Page::Download => "Video Download",
            Page::Claude => "Claude Code",
            Page::Shelf => "Shelf",
            Page::Todo => "Todo",
            Page::System => "System",
            Page::Clipboard => "Clipboard",
            Page::Apps => "Apps",
            Page::Radio => "Quran Radio",
        }
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
    WaterSnooze,
    ReminderDismiss,
    ToastClick,
    /// Copies a clipboard history item back.
    ClipCopy(usize),
    /// The Apps page: open a pinned app, take one off, edit, add.
    AppOpen(usize),
    AppRemove(usize),
    AppsEdit,
    AppsAdd,
    /// Pops the YouTube video playing out into the mini player.
    MiniPlayer,
    /// The radio: play or stop, a station, the volume.
    RadioToggle,
    RadioPick(usize),
    RadioVolume(f32),
    /// Step through the prayer cities by this much.
    PrayerCity(i32),
    PrayerAlerts,
    PrayerDismiss,
    /// Take the link from the clipboard.
    DlPaste,
    DlKind(Kind),
    DlStart,
    DlCancel,
    /// Show the saved file in Explorer.
    DlShow,
    /// Open the popup for the n-th session in the list.
    ClaudeOpen(usize),
    WaterRefilled,
    WaterDrank,
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
    popup: Popup,
    /// Claude Code's recent sessions, re-read from the background reader twice a second.
    sessions: claude::Sessions,
    claude_list: Vec<Session>,
    claude_at: Instant,
    claude_scroll: usize,
    /// The longest each part of a frame took lately (backdrop, notch, popup), for the
    /// HILAL_FPS log.
    parts: (Duration, Duration, Duration),
    /// Where the System page's little GPU fan has turned to (degrees).
    fan_angle: Cell<f32>,
    /// When the popup last opened (for test pictures).
    popup_opened_at: Option<Instant>,
    /// What's playing, as live levels for the closed notch's bars.
    meter: crate::meter::Meter,
    /// The bars as drawn: they leap up with the sound and settle back.
    bar_levels: Cell<[f32; crate::meter::BANDS]>,
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
    /// The button being pressed or just let go, for its press feedback: (action, when it
    /// went down, when it came up).
    press_fx: Option<(Action, Instant, Option<Instant>)>,
    last_wheel: Instant,
    closed_sig: String,
    /// Window region currently set, in physical px [left, top, right, bottom].
    region: [i32; 4],
    /// The settled shape the region currently outlines (None while it's the animation box).
    region_shape: Option<Shape>,
    /// When the welcome started, and whether it's a "welcome back".
    welcome_at: Option<(Instant, bool)>,
    /// While a screenshot is being taken: our windows show in the capture (with the glass
    /// frozen, since a live capture would now include them), until this time.
    capture_until: Option<Instant>,
    /// A screenshot key we held back, to send once the windows are capturable.
    replay: Option<(Instant, u16, u8)>,
    /// A water reminder waiting for an answer (shown as a peek).
    water_ask: Option<store::Ask>,
    /// When the reminder appeared (its icon animates from then).
    water_ask_at: Instant,
    /// How many times its chime has played (like Nusa: on appearing, then twice more,
    /// 12 s apart, while it goes unanswered).
    water_chimes: u32,
    /// A prayer time that just came (index into `prayer::NAMES`, its time in minutes),
    /// shown as a peek until dismissed or for a few minutes.
    prayer_ask: Option<(usize, i32, bool)>,
    /// A dhikr, du'a or verse showing (an index into `REMINDERS`), since when, until when
    /// (hovering it holds that back), how long it was given, and its card and font size.
    reminder: Option<usize>,
    reminder_at: Instant,
    reminder_until: Instant,
    reminder_total: Duration,
    reminder_card: (Shape, f32),
    /// When the next one is due (counted from when the last one left), and the interval it
    /// was counted with (a new interval counts from now).
    reminder_next: Option<(Instant, u32)>,
    /// Shown with "Show one now": stays even with the reminders off.
    reminder_preview: bool,
    /// The last frame's time, for holding the countdown while the card is hovered.
    reminder_tick: Instant,
    /// The card's shell is still shrinking back (the pages mustn't fade in on the way).
    from_card: Cell<bool>,
    /// A card just left from under the mouse: no opening until the mouse has moved off.
    hover_blocked: bool,
    /// "Auto" looking up where the PC is (the answer, once it comes).
    locating: Option<std::sync::mpsc::Receiver<Option<crate::store::AutoPlace>>>,
    /// When it last tried (a failed lookup waits a minute before the next).
    locate_at: Instant,
    prayer_ask_at: Instant,
    /// The download page: the link, what to save it as, and the download under way.
    dl_url: Option<String>,
    dl_kind: Kind,
    /// The format the last download was asked in ("MP3", "WEBM", ...), and the options window.
    dl_format: String,
    dl_win: dl_options::DlWin,
    dl_job: Option<download::Job>,
    /// A paste with no link on the clipboard, shown as a hint for a moment.
    dl_no_link_at: Option<Instant>,
    /// Last time the windows were put back on top, and whether the notch is stepped
    /// aside for a full-screen app.
    top_check: Instant,
    hidden_for_fullscreen: bool,
    /// The glass as it is right now (see `apply_glass`), the sharing option, and whether a
    /// screen recorder is running.
    glass_live: bool,
    share_visible: bool,
    recorder: bool,
    /// A maximized window is in front (on our screen), so the closed notch keeps out of
    /// its way: a thin line that opens only when the mouse rests on it.
    over_maximized: bool,
    /// Since when the mouse has been on the closed notch (for that resting).
    hover_since: Option<Instant>,
    /// Pinned open with Ctrl+Alt+H.
    pinned: bool,
    stats: Stats,
    frame_start: Instant,
    backdrop_retry: Instant,
    dumped: String,
    t0: Instant,

    regions: RefCell<Vec<(Rect, Action)>>,
    ox: Cell<f32>,
    /// How far down the window the notch's top is, as last drawn (hit areas follow it).
    oy: Cell<f32>,
    /// Where the anchor is: in screen px, and in the window (dp).
    anchor_px: (i32, i32),
    nx: f32,
    ny: f32,
    /// The Settings window.
    settings: settings::SettingsWin,
    /// A toast on the notch, and what they watch.
    toast: Option<toasts::Toast>,
    watched: toasts::Watched,
    battery_charging: bool,
    /// Meetings from the iCal link: the feed, what it last read, the link it reads, and the
    /// toasts already shown.
    folder_dl: folder_downloads::Watched,
    /// The companion: the card out (in the dhikr card's place) or waiting, what happened
    /// lately, when you went away and how long you've been at it, the confetti, and since
    /// when the cursor lingers by the notch.
    companion: Option<companion::Card>,
    card_pending: Option<companion::Card>,
    happenings: Vec<companion::Happening>,
    away_from: Option<Instant>,
    active_since: Instant,
    care_next: usize,
    brief_checked: bool,
    started_at: Instant,
    confetti: Option<companion::Confetti>,
    linger_since: Option<Instant>,
    calendar_feed: crate::calendar::Feed,
    calendar: crate::calendar::State,
    calendar_url: String,
    meetings_fired: Vec<String>,
    last_meeting_check: Instant,
    /// The weather (Nova mode), its feed, and when the closed notch's slides started.
    weather: Option<crate::weather::Weather>,
    weather_feed: crate::weather::Feed,
    slides_at: Instant,
    /// The song last seen, and until when a new one shows its card.
    last_track: Option<String>,
    track_peek_until: Option<Instant>,
    /// Copied text, newest first (in memory only).
    clip_history: VecDeque<String>,
    clip_scroll: usize,
    /// The Apps page: the Start menu being read, the icons, which are open, edit mode.
    apps_loading: Option<std::sync::mpsc::Receiver<Vec<crate::apps::AppEntry>>>,
    app_icons: RefCell<HashMap<String, Option<Image>>>,
    apps_running: Vec<bool>,
    apps_running_at: Instant,
    apps_edit: bool,
    apps_loaded: bool,
    /// Quran Radio: the player, its stations being read, the list's scroll.
    radio: crate::radio::Radio,
    radio_loading: Option<std::sync::mpsc::Receiver<Vec<crate::radio::Station>>>,
    radio_fetched: bool,
    radio_scroll: usize,
    /// Updates (and one being installed: since when, its installer), and news.
    update_feed: crate::updater::Feed,
    updating: Option<(Instant, std::path::PathBuf)>,
    news_feed: crate::news::Feed,
    /// The mini player looking for the video (Ok: it opened; Err: what to tell).
    mini_finding: Option<std::sync::mpsc::Receiver<Result<String, &'static str>>>,
    icons_wanted: RefCell<Vec<String>>,
    icons_asked: Vec<String>,
    icons_loading: Option<std::sync::mpsc::Receiver<(String, Option<(Vec<u8>, u32, u32)>)>>,
    /// The browser's address bar in front, when the notch sits in it (Nova mode).
    addr_watch: crate::addrbar::Watch,
    addr: Option<crate::addrbar::Bar>,
    /// The theme last looked up, and whether the battery saver is on.
    theme_cache: RefCell<Option<crate::themes::Theme>>,
    saver_on: bool,
    /// A press away from any button (where, in screen px): moved far enough, it drags.
    press_drag: Option<(i32, i32)>,
    /// Being dragged: where the anchor is held from the cursor (px).
    drag: Option<(i32, i32)>,
    lyric_pos: Cell<f32>,
    /// The first line shown of plain (not synced) lyrics, and the song it's for.
    plain_scroll: Cell<usize>,
    plain_for: RefCell<Option<String>>,
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

pub const PP_W: f32 = 440.0;
pub const PP_H: f32 = 400.0;

/// The Claude session popup.
struct Popup {
    hwnd: HWND,
    surface: usize,
    /// Top-left on the desktop, physical px.
    pos: (i32, i32),
    /// The session it shows (by id), while open.
    session: Option<String>,
    /// The cursor over it, in dp.
    hover: Option<(f32, f32)>,
    last: Instant,
    dumped: bool,
    /// Pinned, it stays open when the mouse leaves.
    pinned: bool,
    /// When the mouse left the notch and the popup (it closes a moment later).
    away_at: Option<Instant>,
    /// The message being written, and whether the box has the keyboard.
    draft: String,
    typing: bool,
    /// What became of the last send (text, when, whether it worked).
    note: Option<(String, Instant, bool)>,
    /// How far out it is (0 hidden, 1 fully shown) and how fast that's changing: a spring
    /// toward `showing`, so it eases out of the notch and back into it.
    shown: f32,
    speed: f32,
    showing: bool,
    /// Frames drawn since `fps_at` (logged once a second with HILAL_FPS set).
    frames: u32,
    fps_at: Instant,
}

impl Popup {
    /// Steps the open/close spring by `dt` seconds: a lively one opening (a hint of
    /// overshoot), a quick, calm one closing.
    fn step(&mut self, dt: f32) {
        let target = if self.showing { 1.0 } else { 0.0 };
        let (response, damping) = if self.showing { (0.34, 0.72) } else { (0.22, 1.0) };
        let stiffness = (std::f32::consts::TAU / response).powi(2);
        let friction = 4.0 * std::f32::consts::PI * damping / response;
        let mut left = dt.min(0.1);
        while left > 0.0 {
            let h = left.min(1.0 / 480.0);
            let a = stiffness * (target - self.shown) - friction * self.speed;
            self.speed += a * h;
            self.shown += self.speed * h;
            left -= h;
        }
        if (self.shown - target).abs() < 0.002 && self.speed.abs() < 0.01 {
            self.shown = target;
            self.speed = 0.0;
        }
    }

    fn animating(&self) -> bool {
        self.session.is_some() && (self.shown != if self.showing { 1.0 } else { 0.0 })
    }
}

fn popup_close_rect() -> Rect {
    Rect::centered(PP_W - 26.0, 26.0, 26.0, 26.0)
}

fn popup_pin_rect() -> Rect {
    Rect::centered(PP_W - 58.0, 26.0, 26.0, 26.0)
}

fn popup_field_rect() -> Rect {
    Rect::new(14.0, PP_H - 50.0, PP_W - 28.0, 36.0)
}

fn popup_send_rect() -> Rect {
    let f = popup_field_rect();
    Rect::centered(f.x + f.w - 18.0, f.y + f.h / 2.0, 28.0, 28.0)
}

fn overlay_close_rect() -> Rect {
    Rect::centered(OV_W - 15.0, 15.0, 18.0, 18.0)
}

impl App {
    /// `overlay_hwnd` is the (hidden) FPS overlay window; `overlay_default` is where it
    /// goes the first time (physical px).
    pub fn new(hwnd: HWND, mut gfx: Gfx, origin: POINT, overlay_hwnd: HWND, overlay_default: (i32, i32), popup_hwnd: HWND) -> Self {
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
        unsafe {
            // Rounded like its pane, so tools that can't capture it draw a pill, not a box;
            // a little rounder than the pane, so its anti-aliased corners aren't cut.
            let d = (28.0 * k) as i32;
            let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(0, 0, (OV_W * k) as i32 + 1, (OV_H * k) as i32 + 1, d, d);
            SetWindowRgn(overlay_hwnd, rgn, false);
        }
        // The popup sits under the open notch, centered.
        let popup_pos = (origin.x + ((WIN_W - PP_W) / 2.0 * k) as i32, origin.y + (150.0 * k) as i32);
        let popup_surface = gfx.add_surface(popup_hwnd, PP_W, PP_H, popup_pos, false).unwrap_or(gfx::MAIN);
        unsafe {
            let _ = SetWindowPos(popup_hwnd, HWND_TOPMOST, popup_pos.0, popup_pos.1, (PP_W * k) as i32, (PP_H * k) as i32, SWP_NOACTIVATE);
            let d = (52.0 * k) as i32;
            let rgn = windows::Win32::Graphics::Gdi::CreateRoundRectRgn(0, 0, (PP_W * k) as i32 + 1, (PP_H * k) as i32 + 1, d, d);
            SetWindowRgn(popup_hwnd, rgn, false);
        }
        let glass_at_start = store.settings.glass;
        let store_nova = store.settings.nova;
        let store_radio_volume = store.radio_volume;
        // The last weather, if it's from the last three hours.
        let store_weather = store.weather.as_ref().filter(|(_, at)| crate::store::unix_now().saturating_sub(*at) < 3 * 3600).map(|(w, _)| w.clone());
        // HILAL_REMINDER_TEST=<index> shows that reminder right away.
        let reminder_test = std::env::var_os("HILAL_REMINDER_TEST").map(|_| (now, store.reminders.minutes));
        win::exclude_from_capture(hwnd, store.settings.glass);
        win::exclude_from_capture(overlay_hwnd, store.settings.glass);
        win::exclude_from_capture(popup_hwnd, store.settings.glass);
        Self {
            popup: Popup { hwnd: popup_hwnd, surface: popup_surface, pos: popup_pos, session: None, hover: None, last: now, dumped: false, pinned: false, away_at: None, draft: String::new(), typing: false, note: None, shown: 0.0, speed: 0.0, showing: false, frames: 0, fps_at: now },
            sessions: claude::Sessions::start(),
            claude_list: Vec::new(),
            claude_at: now,
            claude_scroll: 0,
            popup_opened_at: None,
            fan_angle: Cell::new(0.0),
            parts: Default::default(),
            overlay: Overlay { hwnd: overlay_hwnd, surface, pos: overlay_pos, hover: false, dirty: true, dumps: 0, last: now },
            hwnd,
            gfx,
            origin,
            store,
            media: Media::start(),
            sysmon: Sysmon::start(),
            meter: crate::meter::Meter::new(),
            bar_levels: Cell::new([0.0; crate::meter::BANDS]),
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
            press_fx: None,
            last_wheel: now,
            closed_sig: String::new(),
            region: [0; 4],
            region_shape: None,
            welcome_at: None,
            top_check: now,
            water_ask: None,
            water_ask_at: now,
            water_chimes: 0,
            prayer_ask: None,
            reminder: None,
            reminder_at: now,
            reminder_until: now,
            reminder_total: Duration::from_secs(30),
            reminder_card: (PEEK, 26.0),
            reminder_next: reminder_test,
            reminder_preview: false,
            reminder_tick: now,
            from_card: Cell::new(false),
            hover_blocked: false,
            locating: None,
            locate_at: now - Duration::from_secs(3600),
            prayer_ask_at: now,
            dl_url: None,
            dl_kind: Kind::Mp4,
            dl_format: String::new(),
            dl_win: dl_options::DlWin::new(),
            dl_job: None,
            dl_no_link_at: None,
            capture_until: None,
            replay: None,
            hidden_for_fullscreen: false,
            glass_live: glass_at_start,
            share_visible: false,
            recorder: false,
            over_maximized: false,
            hover_since: None,
            pinned: false,
            stats: Stats::new(now),
            frame_start: now,
            backdrop_retry: now - Duration::from_secs(10),
            dumped: String::new(),
            t0: now,
            regions: RefCell::new(Vec::new()),
            ox: Cell::new(0.0),
            oy: Cell::new(0.0),
            anchor_px: (0, 0),
            nx: CX,
            ny: 0.0,
            settings: settings::SettingsWin::new(),
            toast: None,
            watched: toasts::Watched::new(store_nova),
            battery_charging: win::battery().is_some_and(|b| b.1),
            folder_dl: Default::default(),
            companion: None,
            card_pending: None,
            happenings: Vec::new(),
            away_from: None,
            active_since: now,
            care_next: 0,
            brief_checked: false,
            started_at: now,
            confetti: None,
            linger_since: None,
            calendar_feed: crate::calendar::Feed::start(String::new()),
            calendar: Default::default(),
            calendar_url: String::new(),
            meetings_fired: Vec::new(),
            last_meeting_check: now,
            weather: store_weather,
            weather_feed: crate::weather::Feed::start(store_nova),
            slides_at: now,
            last_track: None,
            track_peek_until: None,
            clip_history: VecDeque::new(),
            clip_scroll: 0,
            apps_loading: None,
            app_icons: RefCell::new(HashMap::new()),
            apps_running: Vec::new(),
            apps_running_at: now,
            apps_edit: false,
            apps_loaded: false,
            mini_finding: None,
            radio: crate::radio::Radio::new(store_radio_volume),
            radio_loading: None,
            radio_fetched: false,
            radio_scroll: 0,
            update_feed: crate::updater::Feed::start(store_nova),
            updating: None,
            news_feed: crate::news::Feed::start(store_nova),
            icons_wanted: RefCell::new(Vec::new()),
            icons_asked: Vec::new(),
            icons_loading: None,
            press_drag: None,
            addr_watch: crate::addrbar::Watch::start(),
            addr: None,
            theme_cache: RefCell::new(None),
            saver_on: false,
            drag: None,
            lyric_pos: Cell::new(0.0),
            plain_scroll: Cell::new(0),
            plain_for: RefCell::new(None),
            last_draw: Cell::new(now),
        }
    }

    /// The closed notch is a thin line: always, if chosen, or while a maximized window is
    /// in front.
    pub fn minimal(&self) -> bool {
        // (In a browser's address bar, the slim pill takes the thin line's place.)
        self.store.settings.minimal || (self.over_maximized && self.addr.is_none())
    }

    /// The "Minimal notch" choice itself (for the tray menu).
    pub fn minimal_setting(&self) -> bool {
        self.store.settings.minimal
    }

    /// Ctrl+Alt+H pins the notch open, until it's pressed again (then it closes, even
    /// with the mouse on it).
    pub fn hotkey(&mut self) {
        if !self.nova() {
            return;
        }
        self.pinned = !self.pinned;
        self.open = self.pinned;
        self.left_at = None;
        if !self.pinned {
            self.hover_blocked = self.inside;
        }
        trace(&format!("[hotkey] pinned {}", self.pinned));
    }

    /// Nova Island's features are on (else Hilal is "Classic", as it was before them).
    pub fn nova(&self) -> bool {
        self.store.settings.nova
    }

    /// Switches between Classic Hilal and Nova mode. Nova's own settings stay saved either
    /// way, for when it's back.
    pub fn set_nova(&mut self, on: bool) {
        self.store.settings.nova = on;
        self.store.save();
        if !on {
            if self.reminder.is_some() {
                self.reminder = None;
                self.reminder_preview = false;
            }
            self.reminder_next = None;
            self.pinned = false;
            self.drag = None;
            self.press_drag = None;
            if self.prayer_ask.is_some_and(|a| a.2) {
                self.prayer_ask = None;
            }
        }
        crate::set_hotkey(on);
        self.apply_position();
        self.apply_glass();
        self.closed_sig.clear();
        trace(&format!("[mode] {}", if on { "nova" } else { "classic" }));
    }

    /// The theme on the notch (Nova mode; None: Hilal's own look).
    fn theme(&self) -> Option<crate::themes::Theme> {
        if !self.nova() {
            return None;
        }
        let id = &self.store.features.theme;
        if self.theme_cache.borrow().as_ref().map(|t| t.id) != Some(id.as_str()) {
            *self.theme_cache.borrow_mut() = crate::themes::by_id(id);
        }
        self.theme_cache.borrow().clone()
    }

    /// The slim pill in the address bar (dp): as wide as the bar's third allows, a touch
    /// taller than the bar.
    fn addr_pill(&self) -> (f32, f32) {
        let Some(b) = self.addr else { return (0.0, 0.0) };
        let k = self.gfx.scale;
        let (w, h) = ((b.rect.right - b.rect.left) as f32 / k, (b.rect.bottom - b.rect.top) as f32 / k);
        ((w / 3.0 - 12.0).min(380.0), (h + 6.0).clamp(22.0, 30.0))
    }

    /// The notch's place in the address bar follows the browser in front (only from the
    /// notch's own place: not once it's been moved, nor standing on a side).
    fn tick_addr(&mut self) {
        let s = &self.store.settings;
        let allowed = s.nova && self.store.features.addr_island && s.anchor.is_none() && s.docked && !s.vertical && self.drag.is_none();
        self.addr_watch.set_on(allowed);
        let mut next = self.addr;
        while let Ok(b) = self.addr_watch.updates.try_recv() {
            next = b;
        }
        if !allowed {
            next = None;
        }
        // (While open it stays where it is.)
        if next != self.addr && !self.open {
            trace(&format!("[addr] {:?}", next.map(|b| (b.rect.left, b.rect.top, b.rect.right, b.rect.bottom, b.typing))));
            self.addr = next;
            self.apply_position();
            self.closed_sig.clear();
        }
    }

    /// Animations off (chosen, or by the battery saver).
    fn no_animations(&self) -> bool {
        self.nova() && (self.store.features.no_animations || self.saver_on)
    }

    fn glass_frozen(&self) -> bool {
        self.nova() && (self.store.features.glass_freeze || self.saver_on)
    }

    /// The battery saver: on below its charge (off the charger), off again above it.
    pub(super) fn check_saver(&mut self, charging: bool, level: f32) {
        let f = &self.store.features;
        let on = self.nova() && f.battery_saver && !charging && level * 100.0 <= f.battery_saver_at as f32;
        if on != self.saver_on {
            self.saver_on = on;
            self.apply_glass();
            let pct = format!("{}%", (level * 100.0).round());
            self.toast('\u{EBB5}', if on { AMBER } else { GREEN }, if on { "Battery saver is on" } else { "Battery saver is off" }, pct, None, 3500);
        }
    }

    /// Where the notch is placed: as saved, or in Classic always hanging from the top center.
    fn pos(&self) -> Pos {
        let s = &self.store.settings;
        // In a browser's address bar: its right third, the notch floating there.
        if let Some(b) = self.addr.filter(|_| s.nova) {
            let r = b.rect;
            let third = (r.right - r.left) / 3;
            let sh = self.addr_pill().1 * self.gfx.scale;
            let top = (r.top + r.bottom) / 2 - (sh / 2.0) as i32;
            return Pos { anchor: Some((r.right - third / 2, top)), docked: false, vertical: false, side_left: false };
        }
        if s.nova {
            Pos { anchor: s.anchor, docked: s.docked, vertical: s.vertical, side_left: s.side_left }
        } else {
            Pos { anchor: None, docked: true, vertical: false, side_left: false }
        }
    }

    /// Standing on a side edge (the nearest one), or back to lying along the top.
    pub fn vertical(&self) -> bool {
        self.pos().vertical
    }

    pub fn set_vertical(&mut self, on: bool) {
        if on == self.store.settings.vertical {
            return;
        }
        let k = self.gfx.scale;
        let (ax, ay) = self.anchor_px;
        let (b, _) = monitor_at((ax, ay));
        let set = &mut self.store.settings;
        if on {
            set.side_left = ax < (b.left + b.right) / 2;
            set.anchor = Some((ax, ay + (V_HALF * k) as i32));
        } else {
            set.docked = false;
            let x = if set.side_left { b.left + (260.0 * k) as i32 } else { b.right - (260.0 * k) as i32 };
            set.anchor = Some((x, ay - (V_HALF * k) as i32));
        }
        set.vertical = on;
        self.apply_position();
        self.store.settings.anchor = Some(self.anchor_px);
        self.store.save();
    }

    /// Back to its own place: hanging from the top of the main screen, in the middle.
    pub fn reset_position(&mut self) {
        let set = &mut self.store.settings;
        set.anchor = None;
        set.docked = true;
        set.vertical = false;
        self.apply_position();
        self.store.save();
    }

    /// Where a shape of the notch sits in the window: docked, at the top with its middle
    /// on the anchor; floating, a little below the anchor; on a side, flush against that
    /// edge and centered on the anchor. Always inside the window (so near a screen edge
    /// it opens toward the free space).
    fn shape_rect(&self, s: Shape) -> Rect {
        let set = self.pos();
        if set.vertical {
            let x = if set.side_left { 0.0 } else { WIN_W - s.w };
            let y = (self.ny - s.h / 2.0).clamp(4.0, (WIN_H - s.h - 4.0).max(4.0));
            return Rect::new(x, y, s.w, s.h);
        }
        let e = if set.docked { s.e } else { 0.0 };
        let half = (s.w / 2.0 + e + 4.0).min(WIN_W / 2.0);
        let cx = self.nx.clamp(half, WIN_W - half);
        let y = if set.docked { 0.0 } else { self.ny.clamp(0.0, (WIN_H - s.h - 6.0).max(0.0)) };
        Rect::new(cx - s.w / 2.0, y, s.w, s.h)
    }

    /// The corners of a free-standing shape (top-left, top-right, bottom-right,
    /// bottom-left): square where it touches a screen edge. None for the docked notch,
    /// which has its ears instead.
    fn shape_radii(&self, s: Shape) -> Option<[f32; 4]> {
        let set = self.pos();
        let r = s.r.min(s.h / 2.0).min(s.w / 2.0).max(0.0);
        if set.vertical {
            Some(if set.side_left { [0.0, r, r, 0.0] } else { [r, 0.0, 0.0, r] })
        } else if set.docked {
            None
        } else {
            Some([r; 4])
        }
    }

    /// Dragging the notch (Nova Island's way): a press anywhere but on a button, moved a
    /// few pixels, carries it. Let go within 28 dp of the top it docks there (snapping to
    /// the middle when near it), anywhere else it floats. Standing on a side it slides
    /// along that edge, and crossing the middle of the screen takes it to the other side.
    fn follow_drag(&mut self) {
        let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
        let Some(p) = win::cursor_px() else { return };
        let k = self.gfx.scale;
        let px = |dp: f32| (dp * k).round() as i32;
        if self.drag.is_none() {
            let Some(at) = self.press_drag else { return };
            if !down {
                self.press_drag = None;
                return;
            }
            if (((p.0 - at.0).pow(2) + (p.1 - at.1).pow(2)) as f32).sqrt() < 5.0 * k {
                return;
            }
            self.press_drag = None;
            self.pressed = None;
            let (ax, ay) = self.anchor_px;
            // Held near where it was grabbed, so the (shrinking) notch stays under the cursor.
            let ox = (p.0 - ax).clamp(-px(PILL_HALF_W - 16.0), px(PILL_HALF_W - 16.0));
            let oy = if self.pos().vertical {
                (p.1 - ay).clamp(-px(V_HALF - 20.0), px(V_HALF - 20.0))
            } else {
                (p.1 - ay).clamp(px(4.0), px(34.0 - 6.0))
            };
            self.drag = Some((ox, oy));
            self.open = false;
            self.left_at = None;
            self.pinned = false;
            trace("[drag] start");
        }
        let Some((ox, oy)) = self.drag else { return };
        let (b, _) = monitor_at(p);
        let set = &mut self.store.settings;
        if set.vertical {
            set.side_left = p.0 < (b.left + b.right) / 2;
            set.anchor = Some((p.0, p.1 - oy));
        } else {
            let mut next = (p.0 - ox, p.1 - oy);
            let docked = next.1 - b.top < px(SNAP_DP);
            if docked {
                next.1 = b.top;
                let middle = (b.left + b.right) / 2;
                if (next.0 - middle).abs() < px(CENTER_SNAP_DP) {
                    next.0 = middle;
                }
            }
            set.docked = docked;
            set.anchor = Some(next);
        }
        self.apply_position();
        self.store.settings.anchor = Some(self.anchor_px);
        if !down {
            self.drag = None;
            self.store.save();
            // It doesn't spring open under the cursor right after the move.
            self.hover_blocked = true;
            let set = &self.store.settings;
            trace(&format!("[drag] dropped at {:?} docked {} vertical {} left {}", self.anchor_px, set.docked, set.vertical, set.side_left));
        }
    }

    /// Puts the window where the notch's anchor is (Nova Island's way): hanging from the
    /// top edge, floating anywhere, or standing on the left or right edge. The window stays
    /// on its screen, and the notch sits inside it at the anchor.
    pub fn apply_position(&mut self) {
        let k = self.gfx.scale;
        let px = |dp: f32| (dp * k).round() as i32;
        let (ww, wh) = (px(WIN_W), px(WIN_H));
        let set = self.pos();
        let (b, wa) = monitor_at(set.anchor.unwrap_or((0, 0)));
        let (mut ax, mut ay) = set.anchor.unwrap_or(((b.left + b.right) / 2, b.top));
        let (wx, wy);
        if set.vertical {
            ax = if set.side_left { b.left } else { b.right };
            ay = ay.clamp(wa.top + px(V_HALF), (wa.bottom - px(V_HALF)).max(wa.top + px(V_HALF)));
            wx = if set.side_left { b.left } else { b.right - ww };
            wy = (ay - wh / 2).clamp(wa.top, (wa.bottom - wh).max(wa.top));
        } else {
            ax = ax.clamp(b.left + px(PILL_HALF_W), b.right - px(PILL_HALF_W));
            ay = if set.docked { b.top } else { ay.clamp(b.top, wa.bottom - px(34.0 + 6.0)) };
            wx = (ax - ww / 2).clamp(b.left, b.right - ww);
            wy = if set.docked { b.top } else { (ay - px(FLOAT_TOP)).clamp(b.top, (wa.bottom - wh).max(b.top)) };
        }
        self.anchor_px = (ax, ay);
        self.nx = (ax - wx) as f32 / k;
        self.ny = (ay - wy) as f32 / k;
        // Waking up when the mouse comes near: the top strip of the window (the whole
        // window when it stands on a side).
        let reach = if set.vertical { wh } else { px(WAKE_H) };
        crate::set_wake_zone([wx - px(40.0), wy - px(40.0), wx + ww + px(40.0), wy + reach + px(40.0)]);
        if (wx, wy) != (self.origin.x, self.origin.y) {
            self.origin = POINT { x: wx, y: wy };
            unsafe {
                let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, wx, wy, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
            }
            self.gfx.move_surface(gfx::MAIN, (wx, wy));
        }
        // The Claude popup: under the open notch, or beside it on a side, kept on screen.
        let page = self.shape_rect(PAGE);
        let (x, y) = if set.vertical {
            let x = if set.side_left { page.x + page.w + 10.0 } else { page.x - 10.0 - PP_W };
            (wx + px(x), wy + px(page.y))
        } else {
            (wx + px(page.x + page.w / 2.0 - PP_W / 2.0), wy + px(page.y + page.h + 10.0))
        };
        let pos = (x.clamp(wa.left, (wa.right - px(PP_W)).max(wa.left)), y.clamp(wa.top, (wa.bottom - px(PP_H)).max(wa.top)));
        if pos != self.popup.pos {
            self.popup.pos = pos;
            unsafe {
                let _ = SetWindowPos(self.popup.hwnd, HWND_TOPMOST, pos.0, pos.1, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
            }
            self.gfx.move_surface(self.popup.surface, pos);
        }
        self.closed_sig.clear();
    }

    /// What the notch does while a maximized window is in front: 0 stays as it is,
    /// 1 shrinks to a thin line, 2 hides.
    pub fn maximized_mode(&self) -> u8 {
        let s = &self.store.settings;
        if s.hide_over_maximized {
            2
        } else if s.shrink_over_maximized {
            1
        } else {
            0
        }
    }

    pub fn set_maximized_mode(&mut self, mode: u8) {
        self.store.settings.shrink_over_maximized = mode == 1;
        self.store.settings.hide_over_maximized = mode == 2;
        self.store.save();
        self.keep_on_top();
    }

    pub fn set_minimal(&mut self, on: bool) {
        self.store.settings.minimal = on;
        self.store.save();
    }

    /// A screenshot key was pressed (and held back by the keyboard hook). Become visible
    /// to capture for a few seconds, with the glass frozen on its last picture, then send
    /// the key on once the compositor has caught up.
    pub fn begin_screenshot(&mut self, vk: u16, mods: u8) {
        let now = Instant::now();
        win::exclude_from_capture(self.hwnd, false);
        win::exclude_from_capture(self.overlay.hwnd, false);
        win::exclude_from_capture(self.popup.hwnd, false);
        // CAPTURE_HIDDEN tracks the "hidden" state; we're only visible for a moment.
        win::CAPTURE_HIDDEN.store(true, std::sync::atomic::Ordering::Relaxed);
        self.capture_until = Some(now + Duration::from_secs(6));
        self.replay = Some((now + Duration::from_millis(45), vk, mods));
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
        // A recorder opening or closing turns the glass off or back on.
        let rec = win::recorder_running();
        if rec != self.recorder {
            self.recorder = rec;
            trace(&format!("[glass] screen recorder {}", if rec { "running: solid notch" } else { "gone: glass back" }));
            self.apply_glass();
        }
        let ours = [self.hwnd, self.overlay.hwnd, self.popup.hwnd];
        let maximized = (self.store.settings.shrink_over_maximized || self.store.settings.hide_over_maximized)
            && foreground_is_maximized(self.hwnd, &ours);
        // Full screen, or a maximized window when chosen: the notch steps aside entirely.
        let fullscreen = (self.store.settings.hide_in_fullscreen && foreground_is_fullscreen(&ours))
            || (maximized && self.store.settings.hide_over_maximized);
        unsafe {
            self.over_maximized = !fullscreen && maximized && self.store.settings.shrink_over_maximized;
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
            if self.popup.session.is_some() {
                let _ = SetWindowPos(self.popup.hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
            }
        }
    }

    /// One pass of the frame loop: polls everything and redraws if needed. Returns how
    /// long the loop may sleep before the next pass, in ms: 0 while something moves (the
    /// vsync-paced present then sets the rhythm).
    pub fn frame(&mut self) -> u32 {
        let now = Instant::now();
        // A pass that comes much later than the loop's longest rest means something held
        // it up: note it.
        let since = now - self.frame_start;
        if since > Duration::from_millis(250) {
            trace(&format!("[stall] {} ms since the last pass (notch open {}, popup {:?})", since.as_millis(), self.open, self.popup.session.is_some()));
        }
        self.frame_start = now;
        if self.store.water.roll_day() {
            self.store.save();
        }
        // A water reminder shows as a peek in the notch and waits for an answer (not over
        // the welcome, so its chime plays when it actually appears).
        if self.water_ask.is_none() && !self.welcoming() {
            if let Some(ask) = self.store.water.take_due() {
                self.water_ask = Some(ask);
                self.water_ask_at = now;
                self.water_chimes = 1;
                self.store.save();
                crate::sound::play_chime();
            }
        }
        // A prayer time shows the same way (after any water reminder), once per prayer.
        self.keep_located();
        if self.prayer_ask.is_none() && self.water_ask.is_none() && !self.welcoming() && self.store.prayer.alerts {
            let place = self.prayer_place();
            let calc = self.prayer_calc();
            if let Some((day, i, at)) = prayer::started(&place, calc) {
                if self.store.prayer.alerted != (day, i) {
                    self.store.prayer.alerted = (day, i);
                    self.store.save();
                    if self.nova() && self.store.features.nova_prayer {
                        self.adhan_scene(i, at);
                    } else {
                        self.prayer_ask = Some((i, at, false));
                        self.prayer_ask_at = now;
                    }
                    crate::sound::play_prayer();
                }
            } else if let Some((day, i, at)) = self.nova().then(|| prayer::coming(&place, PRAYER_HEADSUP, calc)).flatten() {
                // A silent heads-up a few minutes before, for a few seconds.
                if self.store.prayer.headsup != (day, i) {
                    self.store.prayer.headsup = (day, i);
                    self.store.save();
                    self.prayer_ask = Some((i, at, true));
                    self.prayer_ask_at = now;
                }
            }
        }
        // A dhikr, du'a or verse every few minutes, for some seconds (Nova Island's flow).
        // Hovering the card holds its countdown; the next is counted from when it leaves,
        // and waits for a calm moment: no focus session, quiet hours, hidden or open notch,
        // other peek, or welcome.
        let dt = now - self.reminder_tick;
        self.reminder_tick = now;
        let nova = self.store.settings.nova;
        let r = &self.store.reminders;
        if self.reminder.is_some() {
            if (!r.on || !nova) && !self.reminder_preview {
                self.hide_reminder();
            } else {
                if self.inside {
                    self.reminder_until += dt;
                }
                if now >= self.reminder_until {
                    self.hide_reminder();
                }
            }
        } else if r.on && nova {
            let every = r.minutes;
            let due = match self.reminder_next {
                Some((at, counted)) if counted == every => at,
                _ => {
                    let at = now + Duration::from_secs(every as u64 * 60);
                    self.reminder_next = Some((at, every));
                    at
                }
            };
            let focusing = self.pomo.running() && !self.pomo.brk;
            let calm = !focusing && !r.quiet_now() && !self.welcoming() && !self.hidden_for_fullscreen
                && !self.open && !self.pinned && !self.peeking();
            if now >= due && calm {
                self.show_reminder(false);
            }
        }
        crate::sound::tick();
        let shown_for = if self.prayer_ask.is_some_and(|a| a.2) { HEADSUP_SHOWN } else { PRAYER_SHOWN };
        if self.prayer_ask.is_some() && self.prayer_ask_at.elapsed() > shown_for {
            self.prayer_ask = None;
            crate::sound::stop();
        }
        if self.water_ask.is_some() && self.water_chimes < 3 && self.water_ask_at.elapsed() >= Duration::from_secs(12 * self.water_chimes as u64) {
            self.water_chimes += 1;
            crate::sound::play_chime();
        }
        self.check_tasks();
        if !self.pages().contains(&self.page) {
            self.set_page(0);
        }
        self.tick_settings();
        self.tick_dl();
        self.tick_toasts();
        self.tick_apps();
        self.tick_meetings();
        self.tick_folder_downloads();
        self.tick_companion();
        self.tick_addr();
        self.tick_extras();
        if self.open && self.page == Page::Radio {
            self.radio_seen();
        }
        if let Some(Ok(found)) = self.mini_finding.as_ref().map(|rx| rx.try_recv()) {
            self.mini_finding = None;
            match found {
                // The browser's copy pauses; the mini player carries on.
                Ok(_) => {
                    if self.track.as_ref().is_some_and(|t| t.playing) {
                        self.media.send(Command::PlayPause);
                    }
                    self.open = false;
                    self.pinned = false;
                }
                Err(why) => self.toast('\u{E783}', rgb(0xff, 0x8a, 0x80), why, "", None, 3500),
            }
        }
        if self.pomo.finished() {
            unsafe {
                let _ = MessageBeep(MB_ICONASTERISK);
            }
            // (Now on a break: a focus session just ended.)
            if self.pomo.brk {
                self.focus_scene(false);
            }
            self.alert(Page::Pomo);
        }

        self.track = self.media.current();
        let song = self.track.as_ref().map(|t| format!("{}|{}", t.title, t.artist));
        if *self.plain_for.borrow() != song {
            *self.plain_for.borrow_mut() = song;
            self.plain_scroll.set(0);
        }
        self.follow_track();
        self.sync_art();
        if let Some(t) = &self.track {
            self.lyrics.follow(&t.title, &t.artist, t.end_ms());
        }

        self.anim.step();
        if self.no_animations() {
            self.anim.snap();
        }

        // Where is the cursor relative to the notch?
        if let Some((px, py)) = win::cursor_px() {
            let s = self.gfx.scale;
            self.cursor = ((px - self.origin.x) as f32 / s, (py - self.origin.y) as f32 / s);
        }
        let shape = self.anim.value();
        let notch = self.shape_rect(shape);
        // (Hanging from the top, the screen's very top row counts too.)
        let cy = if self.pos().docked && !self.pos().vertical { self.cursor.1.max(0.0) } else { self.cursor.1 };
        self.inside = notch.grow(3.0).contains(self.cursor.0, cy);
        let near = notch.grow(70.0).contains(self.cursor.0, cy);
        self.follow_linger(near);
        self.follow_drag();
        let button_down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
        // A button held while entering means something is being dragged in (files, usually).
        self.file_hover = self.inside && button_down && self.pressed.is_none() && (self.file_hover || !self.open || self.page == Page::Shelf);
        if self.file_hover && self.page != Page::Shelf {
            self.show_page(Page::Shelf);
        }

        let alerting = self.alert_until.is_some_and(|t| now < t);
        // While a water reminder is up, hovering it doesn't open the page (its buttons are
        // right there); an already-open notch stays open.
        // Over a maximized window, a pass over the thin line on the way to its tabs doesn't
        // open it: the mouse has to rest there a moment.
        if self.inside && !self.open {
            self.hover_since.get_or_insert(now);
        } else {
            self.hover_since = None;
        }
        let rested = !self.over_maximized || self.open || self.hover_since.is_some_and(|t| now - t >= HOVER_REST);
        if !self.inside {
            self.hover_blocked = false;
        }
        let toast_click = self.toast_shown() && self.toast.as_ref().is_some_and(|t| t.action.is_some());
        let hover_opens = self.inside && rested && (!self.peeking() || self.open) && !self.hover_blocked && self.drag.is_none() && !toast_click;
        if hover_opens || self.form.is_some() || alerting || self.pinned {
            if !self.open {
                trace(&format!("[open] hover {} rested {:?} over maximized {} form {} alert {}", hover_opens, self.hover_since.map(|t| (now - t).as_millis()), self.over_maximized, self.form.is_some(), alerting));
            }
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

        // A screenshot key was held back: let it through now that we're capturable, and
        // go back to being hidden once the screenshot has surely been taken.
        if let Some((at, vk, mods)) = self.replay {
            if now >= at {
                self.replay = None;
                win::replay_keys(vk, mods);
            }
        }
        if self.capture_until.is_some_and(|t| now >= t) {
            self.capture_until = None;
            if self.glass_live {
                win::exclude_from_capture(self.hwnd, true);
                win::exclude_from_capture(self.overlay.hwnd, true);
                win::exclude_from_capture(self.popup.hwnd, true);
            }
        }

        // Keep the glass fed with what's behind it; retry quietly if capture isn't available.
        // During a screenshot the capture would see us, so the glass keeps its last image.
        let mut backdrop_changed = false;
        // (Frozen glass refreshes only while the notch is open.)
        let frozen = self.glass_frozen() && !self.open;
        if self.glass_live && self.capture_until.is_none() && !frozen {
            if !self.gfx.has_backdrop() && self.backdrop_retry.elapsed() > Duration::from_secs(3) {
                self.backdrop_retry = now;
                self.gfx.enable_backdrop();
            }
            let t_bd = Instant::now();
            let changed = self.gfx.update_backdrop();
            self.parts.0 = self.parts.0.max(t_bd.elapsed());
            if t_bd.elapsed() > Duration::from_millis(50) {
                trace(&format!("[slow] backdrop {} ms", t_bd.elapsed().as_millis()));
            }
            backdrop_changed = changed & (1 << gfx::MAIN) != 0;
            if changed & (1 << self.overlay.surface) != 0 {
                self.overlay.dirty = true;
            }
        }

        // Live stats feed the System page and the FPS overlay, and only run while shown.
        let overlay_on = self.store.settings.overlay;
        let on_system = self.open && self.page == Page::System;
        let side_stats = (self.pos().vertical || (self.nova_idle() && self.store.features.slide_stats)) && !self.open;
        self.sysmon.set_active(on_system || overlay_on || side_stats);
        self.meter.set_active(self.track.as_ref().is_some_and(|t| t.playing));
        if on_system || overlay_on || side_stats {
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
        let busy = animating || self.open || playing || self.welcoming() || self.file_hover || self.peeking() || self.popup.animating()
            || self.drag.is_some() || self.press_drag.is_some() || self.toast_shown() || self.confetti_up() || self.eyes_shown();
        let sig = self.closed_signature();
        if busy || backdrop_changed || sig != self.closed_sig {
            self.closed_sig = sig;
            self.request_dump(animating);
            let t_n = Instant::now();
            self.draw();
            self.parts.1 = self.parts.1.max(t_n.elapsed());
            if t_n.elapsed() > Duration::from_millis(50) {
                trace(&format!("[slow] notch draw {} ms", t_n.elapsed().as_millis()));
            }
        }
        // Claude's sessions: the list refreshes twice a second; the popup redraws at 60 fps
        // while open (its spinners turn), noting where the cursor is over it.
        if self.claude_at.elapsed() > Duration::from_millis(500) {
            self.claude_at = now;
            {
                let t_x = Instant::now();
                self.claude_list = self.sessions.list();
                if t_x.elapsed() > Duration::from_millis(50) {
                    trace(&format!("[slow] sessions list {} ms", t_x.elapsed().as_millis()));
                }
            }
        }
        // (Every frame while it springs open or shut.)
        let popup_due = self.popup.animating() || self.popup.last.elapsed() >= Duration::from_millis(15);
        if self.popup.session.is_some() && popup_due {
            let k = self.gfx.scale;
            self.popup.hover = win::cursor_px()
                .map(|(x, y)| ((x - self.popup.pos.0) as f32 / k, (y - self.popup.pos.1) as f32 / k))
                .filter(|&(x, y)| x >= 0.0 && y >= 0.0 && x < PP_W && y < PP_H);
            let t_p = Instant::now();
            self.draw_popup();
            self.parts.2 = self.parts.2.max(t_p.elapsed());
            if t_p.elapsed() > Duration::from_millis(50) {
                trace(&format!("[slow] popup draw {} ms", t_p.elapsed().as_millis()));
            }
            // Unpinned, it follows the notch: once the mouse has left both for a moment, it
            // closes. Only a message being typed holds it (a draft left behind waits for
            // next time).
            let p = &self.popup;
            if !p.showing {
                // Closing: once it has folded away, hide the window.
                if p.shown <= 0.0 {
                    self.popup.session = None;
                    unsafe {
                        {
                            let t_x = Instant::now();
                            let _ = ShowWindow(self.popup.hwnd, SW_HIDE);
                            if t_x.elapsed() > Duration::from_millis(50) {
                                trace(&format!("[slow] hide popup {} ms", t_x.elapsed().as_millis()));
                            }
                        }
                    }
                }
            } else if p.pinned || (p.typing && !p.draft.is_empty()) || p.hover.is_some() || self.inside {
                self.popup.away_at = None;
            } else if self.popup.away_at.get_or_insert(now).elapsed() > Duration::from_millis(450) {
                self.close_popup();
            }
        }
        // The overlay's numbers change twice a second; its glass at most 30 times a second.
        if overlay_on && self.overlay.dirty && self.overlay.last.elapsed() >= Duration::from_millis(33) {
            self.draw_overlay();
        }
        {
            let t_x = Instant::now();
            self.update_region();
            if t_x.elapsed() > Duration::from_millis(50) {
                trace(&format!("[slow] update region {} ms", t_x.elapsed().as_millis()));
            }
        }
        if self.top_check.elapsed() > Duration::from_secs(2) {
            let t_k = Instant::now();
            self.keep_on_top();
            if t_k.elapsed() > Duration::from_millis(50) {
                trace(&format!("[slow] keep on top {} ms", t_k.elapsed().as_millis()));
            }
        }
        self.log_screen();
        // An open popup keeps a 60 fps pace of its own (its spinners, caret and hover),
        // even while the notch itself rests.
        if busy {
            0
        } else if self.popup.session.is_some() {
            16
        } else {
            120
        }
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
        if self.glass_live {
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
        g.display(&fps, 13.0, 33.0, 64.0, 24.0, 700, c, Align::Start);
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
            let _ = writeln!(
                f,
                "  [screen] {:.0} changed frames/s  dirty {:?}  open={} presents {}  font {:?}",
                (f1 - f0) as f32 / t0.elapsed().as_secs_f32(),
                dirty,
                self.open,
                self.gfx.presents,
                self.gfx.display_family()
            );
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
        // Confetti flies outside the notch: the whole window takes the mouse meanwhile.
        if self.confetti_up() {
            let k = self.gfx.scale;
            let all = [0, 0, (WIN_W * k) as i32, (WIN_H * k) as i32];
            if self.region != all {
                self.region = all;
                self.region_shape = None;
                unsafe {
                    SetWindowRgn(self.hwnd, CreateRectRgn(0, 0, all[2], all[3]), false);
                }
            }
            return;
        }
        let cur = self.anim.value();
        let s = if self.anim.done() {
            cur
        } else {
            let t = self.anim.to;
            Shape { w: cur.w.max(t.w) * 1.1, h: cur.h.max(t.h) * 1.1, r: 0.0, e: cur.e.max(t.e) }
        };
        let k = self.gfx.scale;
        let b = self.shape_rect(s);
        let e = if self.shape_radii(s).is_none() { s.e } else { 0.0 };
        let r = [
            ((b.x - e) * k).floor().max(0.0) as i32,
            (b.y * k).floor().max(0.0) as i32,
            ((b.x + b.w + e) * k).ceil().min(WIN_W * k) as i32,
            ((b.y + b.h) * k).ceil().min(WIN_H * k) as i32 + 1,
        ];
        if self.anim.done() {
            // At rest the region is the notch's exact outline. Besides clicks, it's what
            // a screenshot tool that can't see us paints black, so that comes out as a
            // plain black notch rather than a box.
            if self.region_shape != Some(cur) {
                self.region_shape = Some(cur);
                self.region = r;
                let b = self.shape_rect(cur);
                let rgn = match self.shape_radii(cur) {
                    None => notch_region(cur, b.x + b.w / 2.0, k),
                    Some(radii) => slab_region(b, radii, k),
                };
                unsafe {
                    SetWindowRgn(self.hwnd, rgn, false);
                }
            }
            return;
        }
        let grows = self.region_shape.is_some() || r[0] < self.region[0] || r[2] > self.region[2] || r[3] > self.region[3];
        if r != self.region && grows {
            self.region = r;
            self.region_shape = None;
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
            let page = if self.page == Page::Radio { format!("radio-{:?}", self.radio.state()) } else if self.page == Page::Apps { format!("apps-{}-icons{}", self.pinned_apps().len(), self.app_icons.borrow().values().filter(|i| i.is_some()).count()) } else { self.page.name().to_owned() };
            let what = if !self.open {
                if self.welcoming() { "welcome".to_owned() } else if self.prayer_ask.is_some() { "prayer-peek".to_owned() } else if self.water_ask.is_some() { "water-peek".to_owned() } else if self.toast_shown() { self.toast.as_ref().map_or(String::new(), |t| format!("toast-{:04X}-{}", t.icon as u32, t.value.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect::<String>())) } else if self.reminder.is_some() { "reminder-peek".to_owned() } else if self.track_peeking() { "track-peek".to_owned() } else if self.nova_idle() && self.eyes_shown() { "closed-eyes".to_owned() } else if self.nova_idle() { format!("closed-slide{}", self.slide_now()) } else { "closed".to_owned() }
            } else if self.page == Page::Water && self.water_settings {
                "water-settings".to_owned()
            } else if self.page == Page::Music && self.lyrics_on {
                "lyrics".to_owned()
            } else if self.page == Page::Download {
                // One picture per stage of a download.
                let stage = match self.dl_job.as_ref().map(|j| j.status().phase) {
                    None if self.dl_url.is_some() => "link",
                    None => "empty",
                    Some(Phase::Starting) => "starting",
                    Some(Phase::Downloading) => "downloading",
                    Some(Phase::Finishing) => "finishing",
                    Some(Phase::Done(_)) => "done",
                    Some(Phase::Failed(_)) => "failed",
                };
                format!("download-{stage}")
            } else {
                page.to_owned()
            };
            // Live pages need a moment for their numbers to fill in.
            let settle = if self.page == Page::System && self.open { 2500 } else { 300 };
            if self.page_at.elapsed() < Duration::from_millis(settle) {
                return;
            }
            // And the press feedback: one while held, one as the release ripple spreads.
            match self.press_fx {
                Some((a, _, None)) if self.press_k(a) >= 1.0 => format!("{what}-pressed"),
                Some((a, _, Some(_))) if self.released_for(a).is_some_and(|t| t > 0.1) => format!("{what}-released"),
                _ => what,
            }
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
        self.apply_glass();
    }

    /// "Show in screen sharing": while on, the notch is solid and shows up in shares and
    /// recordings (for Discord and the like, which can't be told apart from just running).
    pub fn share_visible(&self) -> bool {
        self.share_visible
    }

    pub fn set_share_visible(&mut self, on: bool) {
        self.share_visible = on;
        self.apply_glass();
    }

    /// Whether the glass is live right now: chosen, and nothing asks for the notch to show
    /// up in captures (the sharing option, or a recorder running).
    fn apply_glass(&mut self) {
        let nova = self.store.settings.nova;
        let want = self.store.settings.glass && !(nova && (self.share_visible || self.recorder || self.saver_on));
        if want == self.glass_live {
            return;
        }
        self.glass_live = want;
        if !want {
            self.gfx.disable_backdrop();
        }
        // Glass windows capture the screen behind themselves, so they must not appear in
        // that capture; plain ones can show up in screenshots, shares and recordings.
        win::exclude_from_capture(self.hwnd, want);
        win::exclude_from_capture(self.overlay.hwnd, want);
        win::exclude_from_capture(self.popup.hwnd, want);
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
        if self.welcoming() || self.water_ask.is_some() || self.prayer_ask.is_some() {
            return PEEK;
        }
        if self.toast_shown() {
            return if self.pos().vertical { toasts::V_TOAST } else { toasts::TOAST };
        }
        if self.track_peeking() {
            return PEEK;
        }
        if self.reminder.is_some() {
            return self.reminder_card.0;
        }
        if self.pos().vertical {
            let busy = self.track.as_ref().is_some_and(|t| t.playing) || self.pomo.running();
            return if self.store.settings.minimal { V_MINIMAL } else if busy { V_COMPACT } else { V_IDLE };
        }
        // (Over a maximized window, not even the lyric line widens it.)
        if let Some(line) = self.closed_lyric().filter(|_| !self.over_maximized) {
            let w = (self.gfx.measure(&line, 13.0, 600) + 80.0).clamp(IDLE.w, WIDE.w);
            return Shape { w, ..IDLE };
        }
        if let Some(b) = self.addr {
            // A thin line while the address is being typed.
            if b.typing {
                return Shape { w: 90.0, h: 3.0, r: 1.5, e: 0.0 };
            }
            let (w, h) = self.addr_pill();
            return Shape { w, h, r: h / 2.0, e: 0.0 };
        }
        if self.minimal() { MINIMAL } else if self.nova_idle() { slides::N_IDLE } else { IDLE }
    }

    /// The closed notch only needs redrawing when what it shows changes.
    fn closed_signature(&self) -> String {
        // (On a side it also shows the CPU and memory load.)
        let stats = if self.pos().vertical { (self.stats_now.cpu * 100.0).round() as i32 * 1000 + self.ram_pct() } else { 0 };
        let dots = (self.watched.mic.len(), self.watched.cam.len());
        // Nova's slides: which shows, and (for the CPU and memory) their load.
        let slide = if self.nova_idle() {
            let t = self.slides_at.elapsed().as_secs_f32();
            // (A fade in, a frame at a time, for the first third of a second of each.)
            let fading = t % 6.0 < 0.35;
            format!("{}{}{}", (t / 6.0) as u64, if fading { format!("~{}", (t * 30.0) as u64) } else { String::new() }, (self.stats_now.cpu * 100.0).round() as i32)
        } else {
            String::new()
        };
        let dots = (dots, slide, self.weather.as_ref().map(|w| w.temp));
        format!("{}|{:?}|{}|{}|{:?}|{stats}|{dots:?}", win::clock(), self.closed_lyric(), self.pomo.remaining().as_secs(), self.minimal(), self.water_ask)
    }

    /// A reminder (water or prayer) is showing in the notch.
    fn peeking(&self) -> bool {
        self.water_ask.is_some() || self.prayer_ask.is_some() || self.reminder.is_some()
    }

    /// Days the Hijri date is moved by.
    pub fn hijri_adjust(&self) -> i32 {
        self.store.prayer.hijri_adjust
    }

    pub fn set_hijri_adjust(&mut self, days: i32) {
        self.store.prayer.hijri_adjust = days.clamp(-2, 2);
        self.store.save();
    }

    /// Asr by the Hanafi rule.
    pub fn hanafi(&self) -> bool {
        self.store.prayer.hanafi
    }

    pub fn set_hanafi(&mut self, on: bool) {
        self.store.prayer.hanafi = on;
        self.store.save();
    }

    /// The dhikr, du'a and Qur'an reminders' settings.
    pub fn reminders(&self) -> crate::store::Reminders {
        self.store.reminders.clone()
    }

    pub fn update_reminders(&mut self, change: impl FnOnce(&mut crate::store::Reminders)) {
        change(&mut self.store.reminders);
        self.store.save();
    }

    /// "Show one now": a card right away, even with the reminders off.
    pub fn reminder_now(&mut self) {
        if !self.nova() {
            return;
        }
        self.show_reminder(true);
    }

    fn show_reminder(&mut self, preview: bool) {
        let i = self.store.reminders.draw();
        self.store.save();
        self.reminder_card = self.fit_reminder(i);
        self.reminder = Some(i);
        self.reminder_preview = preview;
        let now = Instant::now();
        self.reminder_at = now;
        self.reminder_total = Duration::from_secs(self.store.reminders.seconds.max(1) as u64);
        self.reminder_until = now + self.reminder_total;
        trace(&format!("[reminder] {i} for {} s", self.reminder_total.as_secs()));
    }

    /// The card leaves; the next one is counted from now.
    fn hide_reminder(&mut self) {
        if self.companion.take().is_some_and(|c| c.stops_sound) {
            crate::sound::stop();
        }
        self.reminder = None;
        self.reminder_preview = false;
        // (so it doesn't spring open under the mouse as the card goes)
        self.hover_blocked = self.inside;
        let every = self.store.reminders.minutes;
        self.reminder_next = Some((Instant::now() + Duration::from_secs(every as u64 * 60), every));
    }

    /// The card for a reminder: as wide as Nova's, as tall as its words need (a long one
    /// steps its font down until it fits), and the font size.
    fn fit_reminder(&self, i: usize) -> (Shape, f32) {
        let item = &REMINDERS[i];
        let (text, spans) = reminder_words(item);
        // Diacritics and Qur'anic marks add characters but no width.
        let len = item.text.chars().filter(|c| !matches!(*c as u32, 0x64B..=0x65F | 0x670 | 0x6D6..=0x6ED)).count();
        let mut fs: f32 = if len <= 40 { 30.0 } else if len <= 80 { 26.0 } else { 23.0 };
        let w = if self.pos().vertical { REMINDER_W_SIDE } else { REMINDER_W };
        // Inside: 12 padding, 24 head, 6 + 6 gaps, 16 source line, 16 bottom (and 2 spare).
        let mut h = self.gfx.quran_height(&text, w - 44.0, fs, &spans);
        while fs > 17.0 && h + 82.0 > REMINDER_MAX_H {
            fs -= 2.0;
            h = self.gfx.quran_height(&text, w - 44.0, fs, &spans);
        }
        let h = (h.ceil() + 82.0).min(REMINDER_MAX_H);
        (Shape { w, h, r: 30.0, e: 20.0 }, fs)
    }

    fn alert(&mut self, page: Page) {
        self.show_page(page);
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

    /// The pages to flip through: Classic's, or Nova mode's that are switched on.
    fn pages(&self) -> Vec<Page> {
        if !self.nova() {
            return PAGES.to_vec();
        }
        let hidden = &self.store.features.hidden_pages;
        let list: Vec<Page> = NOVA_PAGES.iter().copied().filter(|p| !hidden.iter().any(|h| h == p.name())).collect();
        if list.is_empty() { vec![Page::Music] } else { list }
    }

    /// Where the current page is in the list.
    fn page_pos(&self) -> usize {
        self.pages().iter().position(|p| *p == self.page).unwrap_or(0)
    }

    /// Goes to a page (if it's in the list).
    fn show_page(&mut self, page: Page) {
        if let Some(i) = self.pages().iter().position(|p| *p == page) {
            self.set_page(i);
        }
    }

    fn set_page(&mut self, i: usize) {
        let pages = self.pages();
        let page = pages[i % pages.len()];
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
        // Away from the buttons a press may become a drag (once it moves).
        if self.pressed.is_none() && self.form.is_none() && self.nova() {
            self.press_drag = win::cursor_px();
        }
        self.press_fx = self.pressed.map(|(a, _, _)| (a, Instant::now(), None));
        self.draw();
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
        if let Some((_, _, up)) = &mut self.press_fx {
            up.get_or_insert_with(Instant::now);
        }
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
        let (lx, y) = (x - self.ox.get(), y - self.oy.get());
        let down = delta < 0;
        if self.page == Page::Todo && lx > 198.0 && y > 30.0 && self.store.tasks.len() > 3 {
            let max = self.store.tasks.len() - 3;
            self.todo_scroll = if down { (self.todo_scroll + 1).min(max) } else { self.todo_scroll.saturating_sub(1) };
            return;
        }
        if self.page == Page::Clipboard && lx > 198.0 && y > 30.0 && self.clip_history.len() > 4 {
            let max = self.clip_history.len() - 4;
            self.clip_scroll = if down { (self.clip_scroll + 1).min(max) } else { self.clip_scroll.saturating_sub(1) };
            return;
        }
        if self.page == Page::Music && self.lyrics_on && lx > 252.0 {
            if let lyrics::State::Plain(lines) = &*self.lyrics.state.lock().unwrap() {
                let max = lines.len().saturating_sub(5);
                let s = self.plain_scroll.get();
                self.plain_scroll.set(if down { (s + 1).min(max) } else { s.saturating_sub(1) });
                return;
            }
        }
        if self.page == Page::Radio && lx > 208.0 && y > 30.0 {
            self.radio_scroll_by(down);
            return;
        }
        if self.page == Page::Claude && y > 30.0 && self.claude_list.len() > 3 {
            let max = self.claude_list.len() - 3;
            self.claude_scroll = if down { (self.claude_scroll + 1).min(max) } else { self.claude_scroll.saturating_sub(1) };
            return;
        }
        // Scrolling over the city pill steps through the cities.
        if self.page == Page::Prayer && lx < 198.0 && y > 90.0 && y < 126.0 {
            self.perform(Action::PrayerCity(if down { 1 } else { -1 }), 0.0);
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
        self.show_page(Page::Shelf);
    }

    fn perform(&mut self, a: Action, x: f32) {
        trace(&format!("[action] {a:?}"));
        use Action::*;
        let idx = self.page_pos();
        let count = self.pages().len();
        let w = &mut self.store.water;
        let mut save = true;
        match a {
            PagePrev => self.set_page(idx + count - 1),
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
            PomoToggle => {
                let starting = !self.pomo.running() && !self.pomo.brk;
                self.pomo.toggle();
                if starting && self.pomo.running() {
                    self.focus_scene(true);
                }
            }
            PomoReset => self.pomo.set_mode(self.pomo.brk),
            PomoMode(brk) => self.pomo.set_mode(brk),
            WaterSwitch => {
                w.enabled = !w.enabled;
                // Turning reminders back on doesn't replay the ones missed meanwhile.
                w.skip_past();
                w.snooze_until = 0;
                if !w.enabled {
                    self.water_ask = None;
                }
            }
            Drink => {
                let below = w.drunk_ml < w.goal_ml;
                w.drink();
                if below && w.drunk_ml >= w.goal_ml {
                    self.celebrate(&[rgb(0x40, 0xc8, 0xff), rgb(0x30, 0xd1, 0x58), WHITE]);
                }
            }
            PrayerCity(d) => {
                let n = self.prayer_choices() as i32;
                let p = &mut self.store.prayer;
                let from = if (p.city as i32) < n { p.city as i32 } else { 0 };
                p.city = (from + d).rem_euclid(n) as usize;
            }
            PrayerAlerts => {
                let p = &mut self.store.prayer;
                p.alerts = !p.alerts;
                if !p.alerts {
                    self.prayer_ask = None;
                    crate::sound::stop();
                }
            }
            DlPaste => {
                match download::clipboard_link() {
                    Some(url) => {
                        // A new link replaces a finished download; a running one keeps going.
                        if !self.dl_job.as_ref().is_some_and(|j| j.running()) {
                            self.dl_job = None;
                        }
                        self.dl_url = Some(url);
                        self.dl_no_link_at = None;
                    }
                    None => self.dl_no_link_at = Some(Instant::now()),
                }
                save = false;
            }
            DlKind(k) => {
                self.dl_kind = k;
                save = false;
            }
            DlStart => {
                // The options first: type, format, quality, name and tags.
                self.open_dl_options();
                save = false;
            }
            DlCancel => {
                self.dl_job = None;
                save = false;
            }
            ClaudeOpen(i) => {
                if let Some(s) = self.claude_list.get(i) {
                    let id = s.id.clone();
                    self.open_popup(id);
                }
                save = false;
            }
            DlShow => {
                if let Some(Phase::Done(path)) = self.dl_job.as_ref().map(|j| j.status().phase) {
                    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
                }
                save = false;
            }
            ReminderDismiss => {
                self.hide_reminder();
                save = false;
            }
            ToastClick => {
                self.toast_click();
                save = false;
            }
            ClipCopy(i) => {
                self.clip_copy(i);
                save = false;
            }
            AppOpen(i) => {
                if let Some(a) = self.pinned_apps().get(i) {
                    crate::apps::open(&a.id);
                }
                save = false;
            }
            AppRemove(i) => {
                let mut list = self.pinned_apps();
                if i < list.len() {
                    list.remove(i);
                }
                self.store.apps_pinned = Some(list);
            }
            AppsEdit => {
                self.apps_edit = !self.apps_edit;
                save = false;
            }
            AppsAdd => {
                self.open_settings_at(4);
                save = false;
            }
            RadioToggle => {
                self.radio_toggle();
                save = false;
            }
            RadioPick(i) => {
                self.radio_pick(i);
                save = false;
            }
            RadioVolume(d) => self.radio_volume(d as f64),
            MiniPlayer => {
                if self.mini_finding.is_none() {
                    if let Some(t) = &self.track {
                        let (title, secs) = (t.title.clone(), (t.position_ms().max(0) / 1000) as u64);
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let found = crate::mini::find(&title);
                            if let Ok((id, _)) = &found {
                                crate::mini::open(id, secs);
                            }
                            let _ = tx.send(found.map(|f| f.0));
                        });
                        self.mini_finding = Some(rx);
                    }
                }
                save = false;
            }
            PrayerDismiss => {
                self.prayer_ask = None;
                crate::sound::stop();
            }
            WaterSettings(on) => self.water_settings = on,
            WaterSub(i) => self.water_sub = i,
            Bottle(d) => {
                w.bottle_ml = (w.bottle_ml as i32 + d * 50).clamp(250, 3000) as u32;
                w.in_bottle_ml = w.in_bottle_ml.min(w.bottle_ml);
            }
            Goal(ml) => w.goal_ml = ml,
            FromH(d) => w.shift_start(d * 30),
            ToH(d) => w.shift_end(d * 30),
            WaterReset => w.reset(),
            WaterSnooze => {
                w.snooze();
                self.water_ask = None;
            }
            WaterRefilled => {
                w.refill();
                // A full bottle: the same reminder now asks you to drink (quietly: it was answered).
                self.water_ask = Some(store::Ask::Drink);
                self.water_chimes = 3;
            }
            WaterDrank => {
                w.drink();
                self.water_ask = None;
            }
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

    /// How pressed the button for `a` looks, 0..1: it sinks in fast while held and eases
    /// back out over a quarter second after release (so even a quick tap shows).
    fn press_k(&self, a: Action) -> f32 {
        let Some((pa, down, up)) = self.press_fx else { return 0.0 };
        if pa != a {
            return 0.0;
        }
        match up {
            None => (down.elapsed().as_secs_f32() / 0.06).min(1.0),
            Some(u) => (1.0 - u.elapsed().as_secs_f32() / 0.25).max(0.0).powi(2),
        }
    }

    /// Seconds since the button for `a` was let go, while its release ripple runs.
    fn released_for(&self, a: Action) -> Option<f32> {
        match self.press_fx {
            Some((pa, _, Some(u))) if pa == a && u.elapsed().as_secs_f32() < 0.4 => Some(u.elapsed().as_secs_f32()),
            _ => None,
        }
    }

    /// A round button's press feedback around (`cx`, `cy`): a soft disc while pressed and
    /// a ripple on release. Returns the scale to draw its glyph at.
    fn press_disc(&self, a: Action, cx: f32, cy: f32, r: f32) -> f32 {
        let g = &self.gfx;
        let k = self.press_k(a);
        if k > 0.0 {
            g.fill_circle(cx, cy, r * (0.8 + 0.2 * k), white(0.2 * k));
        }
        if let Some(t) = self.released_for(a) {
            let p = t / 0.4;
            g.stroke_circle(cx, cy, r * (1.0 + 0.45 * (1.0 - (1.0 - p).powi(2))), 1.2, white(0.3 * (1.0 - p)));
        }
        1.0 - 0.14 * k
    }

    fn hit(&self, r: Rect, a: Action) {
        // Small buttons without their own feedback get a soft highlight while pressed.
        let own = matches!(a, Action::Previous | Action::PlayPause | Action::Next | Action::PagePrev | Action::PageNext | Action::SeekBar(..) | Action::Page(_) | Action::ShelfOpen(_));
        if !own && r.w <= 140.0 && r.h <= 60.0 {
            let k = self.press_k(a);
            if k > 0.0 {
                self.gfx.fill_rrect(r, (r.h / 2.0).min(10.0), white(0.14 * k));
            }
        }
        if self.gfx.fade.get() > 0.5 {
            let ox = self.ox.get();
            self.regions.borrow_mut().push((Rect::new(r.x + ox, r.y + self.oy.get(), r.w, r.h), a));
        }
    }

    fn hovered(&self, r: Rect) -> bool {
        r.contains(self.cursor.0 - self.ox.get(), self.cursor.1 - self.oy.get())
    }

    pub fn draw(&mut self) {
        let started = Instant::now();
        if self.gfx.begin(gfx::MAIN).is_err() {
            return;
        }
        self.regions.borrow_mut().clear();
        let s = self.anim.value();
        let g = &self.gfx;
        let notch = self.shape_rect(s);
        let cx = notch.x + notch.w / 2.0;
        let theme = self.theme();
        g.ink.set(theme.as_ref().and_then(|t| t.ink));
        // (The retro theme's corners are nearly square.)
        let s = match theme.as_ref().and_then(|t| t.radius) {
            Some(r) => Shape { r: s.r.min(r), ..s },
            None => s,
        };
        let e = match (self.shape_radii(s), &theme) {
            (radii, Some(t)) => {
                g.themed((cx, s.w, s.h, s.r, s.e), radii.map(|r| (notch, r)), t, self.glass_live);
                if radii.is_none() { s.e } else { 0.0 }
            }
            (None, None) => {
                if self.glass_live {
                    g.glass(cx, s.w, s.h, s.r, s.e);
                } else {
                    g.notch(cx, s.w, s.h, s.r, s.e, BLACK);
                }
                s.e
            }
            (Some(radii), None) => {
                if self.glass_live {
                    g.glass_slab(notch, radii);
                } else {
                    g.slab(notch, radii, BLACK);
                }
                0.0
            }
        };
        g.clip(Rect::new(notch.x - e, notch.y, s.w + 2.0 * e, s.h));
        // Everything inside is drawn from the notch's top.
        g.set_dy(notch.y);
        self.oy.set(notch.y);
        let vertical = self.pos().vertical;

        // The reminder card is tall like a page, but it isn't one: no page fades in under it,
        // nor as its shell shrinks back afterwards.
        if self.open {
            self.from_card.set(false);
        } else if self.reminder.is_some() {
            self.from_card.set(true);
        }
        // (Standing on a side, the closed notch is tall and narrow: width tells the states apart.)
        let grown = if vertical { ((s.w - 200.0) / 150.0).clamp(0.0, 1.0) } else { ((s.h - 70.0) / 50.0).clamp(0.0, 1.0) };
        let open_a = if self.from_card.get() { 0.0 } else { grown };
        let closed_a = if vertical { ((90.0 - s.w) / 30.0).clamp(0.0, 1.0) } else { ((44.0 - s.h) / 12.0).clamp(0.0, 1.0) };
        let peek_a = |full: f32| if vertical { ((s.w - 100.0) / 120.0).clamp(0.0, 1.0) } else { ((s.h - 34.0) / full).clamp(0.0, 1.0) };
        if open_a > 0.0 {
            let page_a = (self.page_at.elapsed().as_secs_f32() / 0.22).min(1.0);
            g.fade.set(open_a * page_a);
            // Content grows out of the top edge with the glass instead of being uncovered.
            let k = 0.9 + 0.1 * open_a;
            if self.page == Page::Music && self.lyrics_on && self.anim.to == WIDE {
                self.ox.set(cx - 260.0);
                g.transform(self.ox.get(), k, cx);
                self.lyrics_page();
            } else {
                self.ox.set(cx - 210.0);
                g.transform(self.ox.get(), k, cx);
                self.page_content();
            }
        }
        // A toast takes the closed notch's place (fading in over a fifth of a second).
        let toast_a = if self.toast_shown() && !self.open { self.toast.as_ref().map_or(0.0, |t| (t.at.elapsed().as_secs_f32() / 0.2).min(1.0)) } else { 0.0 };
        let closed_a = closed_a * (1.0 - toast_a);
        if closed_a > 0.0 && s.h > 12.0 && s.w > 12.0 {
            g.fade.set(closed_a);
            self.ox.set(0.0);
            g.offset(0.0);
            self.closed(s);
        }
        if self.anim.to == PEEK || (self.welcoming() && open_a == 0.0) {
            let a = peek_a(20.0);
            if a > 0.0 {
                g.fade.set(a);
                self.ox.set(notch.x + (s.w - PEEK.w) / 2.0);
                g.offset(self.ox.get());
                match (self.water_ask, self.prayer_ask) {
                    (Some(ask), _) if !self.welcoming() => self.water_ask_view(ask),
                    (None, Some((i, at, soon))) if !self.welcoming() => self.prayer_ask_view(i, at, soon),
                    (None, None) if self.track_peeking() && !self.welcoming() => self.track_peek_view(),
                    _ => self.welcome_view(),
                }
            }
        }
        if let Some(i) = self.reminder.filter(|_| self.anim.to == self.reminder_card.0) {
            let a = peek_a(40.0);
            if a > 0.0 {
                g.fade.set(a);
                self.ox.set(notch.x + (s.w - self.reminder_card.0.w) / 2.0);
                g.offset(self.ox.get());
                if i == companion::CARD {
                    self.card_view();
                } else {
                    self.reminder_view(i);
                }
            }
        }
        if toast_a > 0.0 {
            self.ox.set(0.0);
            g.offset(0.0);
            self.toast_view(s, toast_a);
        }
        self.ox.set(0.0);
        g.offset(0.0);
        self.privacy_dots(s);
        g.offset(0.0);
        g.unclip();
        if self.confetti_up() {
            g.set_dy(0.0);
            self.confetti_view();
        }
        g.ink.set(None);
        self.last_draw.set(Instant::now());
        let cpu = self.frame_start.elapsed().max(started.elapsed());
        if started.elapsed() > Duration::from_millis(50) {
            trace(&format!("[slow] notch building {} ms", started.elapsed().as_millis()));
        }
        let t_present = Instant::now();
        let _ = self.gfx.present();
        if t_present.elapsed() > Duration::from_millis(50) {
            trace(&format!("[slow] notch present {} ms", t_present.elapsed().as_millis()));
        }
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

    /// The water reminder, in the 310-wide peek: an icon, what it wants, and two answers.
    /// An empty bottle asks for a refill first; once refilled it asks you to drink.
    fn water_ask_view(&self, ask: store::Ask) {
        let g = &self.gfx;
        let w = &self.store.water;
        let (cx, cy) = (32.0, 30.0);
        let icon = Rect::centered(cx, cy, 40.0, 40.0);
        let lit = self.hovered(icon.grow(8.0));
        let t = self.water_ask_at.elapsed().as_secs_f32();
        let cyan = |a: f32| rgba(0x64, 0xdc, 0xfc, a);
        // The ring settles into place as the reminder appears.
        let k = 1.0 - (1.0 - (t / 0.3).min(1.0)).powi(3);
        let ring_r = 13.0 + 6.0 * k;
        g.fill_circle(cx, cy, ring_r, cyan(0.06));
        g.stroke_circle(cx, cy, ring_r, 1.6, cyan(if lit { 0.95 } else { 0.45 }));
        // A ripple leaves the icon every 0.46 s, growing and fading.
        let phase = t % 0.46;
        if phase < 0.42 {
            let p = phase / 0.42;
            let e = 1.0 - (1.0 - p).powi(2);
            g.stroke_circle(cx, cy, 9.0 + 14.0 * e, 1.5, cyan(0.55 * (1.0 - p)));
        }
        // Every 1.35 s the icon wobbles from its base: a damped little shake.
        let wobble = |amp: f32| {
            let wt = t % 1.35;
            if wt < 0.45 { amp * (std::f32::consts::TAU * 2.6 * wt).sin() * (-wt * 7.0).exp() } else { 0.0 }
        };
        let ox = self.ox.get();
        let (title, sub, yes, yes_act) = match ask {
            store::Ask::Refill => {
                // A bottle: cap, body, and the water line inside.
                g.rotate(ox, cx, cy + 11.0, wobble(22.0));
                g.fill_rrect(Rect::centered(cx, cy - 9.5, 6.5, 4.0), 1.2, cyan(1.0));
                g.stroke_rrect(Rect::new(cx - 6.5, cy - 6.5, 13.0, 17.5), 4.0, 1.8, cyan(1.0), false);
                g.line(cx - 4.0, cy + 3.0, cx + 4.0, cy + 3.0, 1.6, cyan(1.0));
                g.offset(ox);
                ("Time to Refill", "Your bottle is probably empty".to_owned(), "Refilled", Action::WaterRefilled)
            }
            store::Ask::Drink => {
                g.rotate(ox, cx, cy + 10.0, wobble(9.0));
                g.fill_circle(cx, cy + 3.0, 7.5, cyan(1.0));
                g.fill_poly(&[(cx, cy - 10.5), (cx + 7.2, cy + 1.6), (cx - 7.2, cy + 1.6)], cyan(1.0));
                g.offset(ox);
                ("Time to Drink", format!("Stay hydrated! · {} ml", w.per_ml()), "Drank", Action::WaterDrank)
            }
        };
        // Buttons sized from their words, ending at the peek's right edge.
        let bw = |s: &str| g.measure(s, PILL_TEXT, 600) + 2.0 * PILL_PAD;
        let ok = Rect::new(301.0 - bw(yes), 16.0, bw(yes), 28.0);
        let snooze = Rect::new(ok.x - 6.0 - bw("Snooze"), 16.0, bw("Snooze"), 28.0);
        g.text(title, 60.0, 23.0, snooze.x - 64.0, 14.0, 700, WHITE, Align::Start);
        g.text(&sub, 60.0, 40.0, snooze.x - 64.0, 10.5, 400, white(0.55), Align::Start);
        g.fill_rrect(snooze, 14.0, white(if self.hovered(snooze) { 0.22 } else { 0.14 }));
        g.text("Snooze", snooze.x, snooze.y + 14.0, snooze.w, PILL_TEXT, 600, WHITE, Align::Center);
        self.hit(snooze, Action::WaterSnooze);
        g.fill_rrect(ok, 14.0, if self.hovered(ok) { rgb(0x2a, 0x84, 0xec) } else { rgb(0x1d, 0x6f, 0xd8) });
        g.text(yes, ok.x, ok.y + 14.0, ok.w, PILL_TEXT, 600, WHITE, Align::Center);
        self.hit(ok, yes_act);
    }

    /// A prayer time has come: a glowing crescent, the prayer's name, and an OK.
    fn prayer_ask_view(&self, i: usize, at: i32, soon: bool) {
        let g = &self.gfx;
        let (cx, cy) = (32.0, 30.0);
        let t = self.prayer_ask_at.elapsed().as_secs_f32();
        // The glow breathes slowly; the crescent rises into place as the peek appears.
        let breath = 0.5 + 0.5 * (t * std::f32::consts::TAU / 2.4).sin();
        g.radial(cx, cy, 22.0, 22.0, rgba(0xff, 0xc4, 0x3d, 0.18 + 0.16 * breath));
        let k = 1.0 - (1.0 - (t / 0.45).min(1.0)).powi(3);
        let my = cy + 6.0 * (1.0 - k);
        g.fill_poly(&crescent(cx, my, 11.0, (5.0, -3.5), 9.2), AMBER);
        g.fill_circle(cx + 9.5, my + 7.5, 1.3, rgba(0xff, 0xe0, 0x8a, 0.9 * k * breath));

        let ok = Rect::new(301.0 - (g.measure("OK", PILL_TEXT, 600) + 4.0 * PILL_PAD), 16.0, g.measure("OK", PILL_TEXT, 600) + 4.0 * PILL_PAD, 28.0);
        let time = win::hm_text((at / 60 % 24) as u16, (at % 60) as u16);
        let (title, sub) = if soon {
            (format!("{} in {PRAYER_HEADSUP} minutes", prayer::NAMES[i]), format!("Get ready · {time}"))
        } else {
            let (y, m, d, _) = prayer::local_now(&self.prayer_place());
            if i == 4 && self.ramadan(y, m, d) {
                (format!("{} prayer", prayer::NAMES[i]), format!("\u{062a}\u{0642}\u{0628}\u{0651}\u{0644} \u{0627}\u{0644}\u{0644}\u{0647} \u{0635}\u{064a}\u{0627}\u{0645}\u{0643} · {time}"))
            } else {
                (format!("{} prayer", prayer::NAMES[i]), format!("It's time to pray · {time}"))
            }
        };
        g.text(&title, 60.0, 23.0, ok.x - 64.0, 14.0, 700, WHITE, Align::Start);
        g.text(&sub, 60.0, 40.0, ok.x - 64.0, 10.5, 400, white(0.55), Align::Start);
        g.fill_rrect(ok, 14.0, if self.hovered(ok) { rgba(0xff, 0xc4, 0x3d, 0.4) } else { rgba(0xff, 0xc4, 0x3d, 0.28) });
        g.text("OK", ok.x, ok.y + 14.0, ok.w, PILL_TEXT, 600, AMBER, Align::Center);
        self.hit(ok, Action::PrayerDismiss);
    }

    /// A dhikr, du'a or verse, as a card sized to its words: its kind (a crescent and the
    /// kind's name, in the kind's color) and a close button at the top, the words in the
    /// Qur'an face, where they're from, and the time left as a thin bar (dimmed while the
    /// mouse holds it).
    fn reminder_view(&self, i: usize) {
        let g = &self.gfx;
        let item = &REMINDERS[i];
        let (card, fs) = self.reminder_card;
        let (w, h) = (card.w, card.h);
        let accent = kind_color(item.kind);
        let t = self.reminder_at.elapsed().as_secs_f32();
        let breath = 0.5 + 0.5 * (t * std::f32::consts::TAU / 4.0).sin();
        g.radial(w - 30.0, 0.0, 190.0, 120.0, rgba(0xff, 0xd4, 0x79, 0.10 + 0.04 * breath));
        g.radial(30.0, h, 200.0, 140.0, [accent[0], accent[1], accent[2], 0.12 + 0.05 * breath]);

        // The kind, on the right (the card reads right to left).
        let name = ["\u{0622}\u{064a}\u{0629}", "\u{0630}\u{0643}\u{0631}", "\u{062f}\u{0639}\u{0627}\u{0621}"][item.kind as usize % 3];
        let nw = g.measure(name, 12.0, 600);
        let badge = Rect::new(w - 22.0 - (nw + 37.0), 13.0, nw + 37.0, 22.0);
        g.fill_rrect(badge, 11.0, [accent[0], accent[1], accent[2], 0.14]);
        g.fill_poly(&crescent(badge.x + badge.w - 15.0, 24.0, 6.0, (2.8, -1.9), 5.0), accent);
        g.text(name, badge.x + 8.0, 24.0, badge.w - 35.0, 12.0, 600, accent, Align::Center);
        let close = Rect::centered(34.0, 24.0, 24.0, 24.0);
        let lit = self.hovered(close);
        if lit {
            g.fill_circle(34.0, 24.0, 12.0, white(0.14));
        }
        g.icon('\u{E711}', 34.0, 24.5, 9.0, white(if lit { 1.0 } else { 0.55 }));
        self.hit(close, Action::ReminderDismiss);

        let (text, spans) = reminder_words(item);
        g.quran_text(&text, Rect::new(22.0, 42.0, w - 44.0, h - 80.0), fs, WHITE, &spans);
        g.text(item.from, 22.0, h - 24.0, w - 44.0, 11.5, 500, white(0.6), Align::Center);

        // The time left, running out toward the right.
        let left = self.reminder_until.saturating_duration_since(Instant::now()).as_secs_f32();
        let frac = (left / self.reminder_total.as_secs_f32().max(0.001)).clamp(0.0, 1.0);
        let track = Rect::new(30.0, h - 9.0, w - 60.0, 2.0);
        g.fill_rrect(track, 1.0, white(0.08));
        if frac > 0.0 {
            let bw = track.w * frac;
            let held = if self.inside { 0.45 } else { 1.0 };
            g.fill_rrect(Rect::new(track.x + track.w - bw, track.y, bw, 2.0), 1.0, [accent[0], accent[1], accent[2], held]);
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
            Page::Prayer => self.prayer_page(),
            Page::Download => self.download_page(),
            Page::Claude => self.claude_page(),
            Page::Shelf => self.shelf_page(),
            Page::Todo => self.todo_page(),
            Page::System => self.system_page(),
            Page::Clipboard => self.clipboard_page(),
            Page::Apps => self.apps_page(),
            Page::Radio => self.radio_page(),
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
        self.header_weather();
    }

    /// The time the iOS way: digits in the display face, AM/PM smaller and dimmer beside
    /// them, sharing a baseline. Draws starting at `x`; returns the width.
    fn time(&self, x: f32, cy: f32, size: f32, weight: u32, c: Color) -> f32 {
        let g = &self.gfx;
        let (t, ampm) = win::clock_parts();
        let tw = g.display(&t, x, cy, 200.0, size, weight, c, Align::Start);
        let Some(ampm) = ampm else { return tw };
        let small = size * 0.64;
        let gap = size * 0.2;
        // Smaller text sits a little lower so both share the baseline.
        let aw = g.display(ampm, x + tw + gap, cy + (size - small) * 0.3, 60.0, small, 600, [c[0], c[1], c[2], c[3] * 0.72], Align::Start);
        tw + gap + aw
    }

    fn time_width(&self, size: f32, weight: u32) -> f32 {
        let g = &self.gfx;
        let (t, ampm) = win::clock_parts();
        // measure_display includes trailing space; the digits have none.
        let tw = g.measure_display(&t, size, weight);
        ampm.map_or(tw, |a| tw + size * 0.2 + g.measure_display(a, size * 0.64, 600))
    }

    fn clock_group(&self, cx: f32, cy: f32) {
        let g = &self.gfx;
        let date = win::date_long();
        let dw = g.measure_display(&date, 12.5, 500);
        let tw = self.time_width(13.0, 700);
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
        g.display(&date, x, cy, dw + 2.0, 12.5, 500, white(0.72), Align::Start);
        self.time(x + dw + 7.0, cy, 13.0, 700, WHITE);
    }

    fn nav(&self) {
        let g = &self.gfx;
        for (cx, glyph, a) in [(22.0, '\u{E76B}', Action::PagePrev), (398.0, '\u{E76C}', Action::PageNext)] {
            let r = Rect::centered(cx, 69.0, 24.0, 24.0);
            let k = self.press_k(a);
            let s = 1.0 - 0.14 * k;
            g.fill_circle(cx, 69.0, 11.0 * s, white(if self.hovered(r) { 0.4 } else { 0.28 } + 0.3 * k));
            if let Some(t) = self.released_for(a) {
                let p = t / 0.4;
                g.stroke_circle(cx, 69.0, 11.0 + 7.0 * (1.0 - (1.0 - p).powi(2)), 1.2, white(0.3 * (1.0 - p)));
            }
            g.scale_at(cx, 69.0, s);
            g.icon(glyph, cx, 69.5, 9.0, WHITE);
            g.unscale();
            self.hit(r, a);
        }
        let active = self.page_pos();
        let count = self.pages().len();
        let widths: Vec<f32> = (0..count).map(|i| if i == active { 15.0 } else { 3.6 }).collect();
        let total: f32 = widths.iter().sum::<f32>() + 4.5 * (count - 1) as f32;
        let mut x = 210.0 - total / 2.0;
        for (i, w) in widths.into_iter().enumerate() {
            g.fill_rrect(Rect::new(x, 131.2, w, 3.6), 1.8, if i == active { WHITE } else { white(0.45) });
            self.hit(Rect::new(x - 2.0, 125.0, w + 4.0, 14.0), Action::Page(i));
            x += w + 4.5;
        }
    }

    /// The width a pill needs for its icon and label, with even room on both sides.
    fn pill_w(&self, icon: Ico, label: &str) -> f32 {
        let tw = if label.is_empty() { 0.0 } else { self.gfx.measure(label, PILL_TEXT, 500) };
        let iw = if icon == Ico::None { 0.0 } else { 13.0 };
        let gap = if iw > 0.0 && tw > 0.0 { 5.0 } else { 0.0 };
        iw + gap + tw + 2.0 * PILL_PAD
    }

    fn pill(&self, r: Rect, fill: Color, icon: Ico, label: &str, label_c: Color, a: Option<Action>) {
        let g = &self.gfx;
        let hover = a.is_some() && self.hovered(r);
        let fill = if hover { [fill[0], fill[1], fill[2], (fill[3] * 1.4).min(1.0)] } else { fill };
        g.fill_rrect(r, r.h / 2.0, fill);
        let tw = if label.is_empty() { 0.0 } else { g.measure(label, PILL_TEXT, 500) };
        let iw = if icon == Ico::None { 0.0 } else { 13.0 };
        let gap = if iw > 0.0 && tw > 0.0 { 5.0 } else { 0.0 };
        // An icon looks lighter than text, so nudge the content toward it a touch.
        let nudge = if iw > 0.0 && tw > 0.0 { -1.0 } else { 0.0 };
        let x = r.x + (r.w - iw - gap - tw) / 2.0 + nudge;
        let cy = r.y + r.h / 2.0;
        self.draw_ico(icon, x + iw / 2.0, cy, label_c);
        if tw > 0.0 {
            g.text(label, x + iw + gap, cy, tw + 4.0, PILL_TEXT, 500, label_c, Align::Start);
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
        if self.pos().vertical {
            return self.closed_side(s);
        }
        let g = &self.gfx;
        let x0 = self.shape_rect(s).x;
        let cy = s.h / 2.0;
        let line = self.closed_lyric();
        if let Some(t) = self.track.as_ref().filter(|t| t.playing) {
            let lv = self.bar_levels();
            // The bass makes the cover glow in its own color and swell a touch.
            let bass = lv[0];
            let accent = self.art().map_or(WHITE, |(_, c)| *c);
            g.radial(x0 + 20.0, cy, 18.0 + 6.0 * bass, 16.0 + 4.0 * bass, [accent[0], accent[1], accent[2], 0.10 + 0.40 * bass]);
            let side = 20.0 * (1.0 + 0.07 * bass);
            let art = Rect::centered(x0 + 20.0, cy, side, side);
            if let Some((img, _)) = self.art() {
                g.draw_image(img, art, 5.0, 1.0);
            } else {
                g.fill_rrect(art, 5.0, white(0.15));
            }
            self.bars(x0 + s.w - 31.0, cy, &lv, accent);
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
        } else if self.nova_idle() && self.eyes_shown() {
            self.eyes_view(s);
        } else if self.nova_idle() {
            self.closed_slides(s);
        } else if !self.minimal() {
            let w = self.time_width(14.5, 600);
            self.time(x0 + (s.w - w) / 2.0, cy, 14.5, 600, white(0.95));
        }
    }

    /// The closed notch standing on a side: the time, the day, the CPU and memory load and
    /// the next prayer down its length; with music, the cover, the bars and the time; with
    /// a timer, its ring and what's left.
    fn closed_side(&self, s: Shape) {
        let g = &self.gfx;
        if s.w < 30.0 {
            return;
        }
        let b = self.shape_rect(s);
        let cx = b.x + b.w / 2.0;
        let w = b.w - 4.0;
        let small = |text: &str, cy: f32, size: f32, weight: u32, c: Color| {
            g.display(text, b.x + 2.0, cy, w, size, weight, c, Align::Center);
        };
        let hm = || {
            let (t, _) = win::clock_parts();
            t
        };
        if let Some(_t) = self.track.as_ref().filter(|t| t.playing) {
            let lv = self.bar_levels();
            let accent = self.art().map_or(WHITE, |(_, c)| *c);
            let art = Rect::centered(cx, 32.0, 28.0, 28.0);
            g.radial(cx, 32.0, 20.0 + 6.0 * lv[0], 20.0 + 6.0 * lv[0], [accent[0], accent[1], accent[2], 0.10 + 0.35 * lv[0]]);
            if let Some((img, _)) = self.art() {
                g.draw_image(img, art, 8.0, 1.0);
            } else {
                g.fill_rrect(art, 8.0, white(0.15));
            }
            self.bars(cx - 10.0, 68.0, &lv, accent);
            small(&hm(), 100.0, 11.5, 600, white(0.95));
        } else if self.pomo.running() {
            let left = self.pomo.remaining();
            let frac = left.as_secs_f32() / Pomo::total(self.pomo.brk).as_secs_f32();
            g.stroke_circle(cx, 40.0, 11.0, 2.4, rgba(0xff, 0x4b, 0x55, 0.3));
            g.arc(cx, 40.0, 11.0, 0.0, frac * 360.0, 2.4, RED);
            small(&mmss(left.as_millis() as i64), 74.0, 11.5, 600, WHITE);
            small(&hm(), 100.0, 10.0, 500, white(0.55));
        } else {
            let day = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][win::weekday() as usize % 7];
            small(&hm(), 30.0, 12.0, 700, WHITE);
            small(day, 45.0, 10.0, 500, white(0.55));
            g.fill_rrect(Rect::new(cx - 9.0, 59.0, 18.0, 1.0), 0.5, white(0.16));
            small("CPU", 75.0, 9.5, 500, white(0.55));
            small(&format!("{}%", (self.stats_now.cpu * 100.0).round() as i32), 89.0, 12.0, 700, WHITE);
            small("RAM", 110.0, 9.5, 500, white(0.55));
            small(&format!("{}%", self.ram_pct()), 124.0, 12.0, 700, WHITE);
            // The next prayer, and how long until it.
            let place = self.prayer_place();
            let calc = self.prayer_calc();
            let (y, m, d, now) = prayer::local_now(&place);
            let t = prayer::day_times(&place, y, m, d, calc);
            let (i, at) = [0usize, 2, 3, 4, 5].iter().find(|&&i| t[i] * 60 > now).map_or((0, (t[0] + 1440) * 60), |&i| (i, t[i] * 60));
            let left = (at - now).max(0) / 60;
            g.fill_rrect(Rect::new(cx - 9.0, 143.0, 18.0, 1.0), 0.5, white(0.16));
            small(prayer::NAMES[i], 160.0, 9.5, 500, rgba(0xff, 0xc4, 0x3d, 0.8));
            small(&if left >= 60 { format!("{}h{:02}", left / 60, left % 60) } else { format!("{left}m") }, 174.0, 11.5, 700, AMBER);
        }
    }

    /// Memory in use, as a percentage.
    fn ram_pct(&self) -> i32 {
        let s = &self.stats_now;
        if s.ram_total == 0 { 0 } else { (s.ram_used as f64 * 100.0 / s.ram_total as f64).round() as i32 }
    }

    /// The bars' levels this frame: they follow the live sound (quick up, eased down), or
    /// breathe gently when no sound comes through (a muted tab, say).
    fn bar_levels(&self) -> [f32; crate::meter::BANDS] {
        let dt = self.last_draw.get().elapsed().as_secs_f32().min(0.1);
        let t = self.t0.elapsed().as_secs_f32();
        let target: [f32; crate::meter::BANDS] = self.meter.levels().unwrap_or_else(|| {
            std::array::from_fn(|i| 0.25 + 0.12 * (t * 2.2 + i as f32 * 0.9).sin())
        });
        let mut shown = self.bar_levels.get();
        for (v, tv) in shown.iter_mut().zip(target) {
            let rate = if tv > *v { 28.0 } else { 7.0 };
            *v += (tv - *v) * (1.0 - (-dt * rate).exp());
        }
        self.bar_levels.set(shown);
        shown
    }

    /// Five bars in the cover's color, bass in the middle and the highs to the sides.
    fn bars(&self, x: f32, cy: f32, lv: &[f32; crate::meter::BANDS], c: Color) {
        let g = &self.gfx;
        const ORDER: [usize; 5] = [3, 1, 0, 2, 4];
        for (slot, &band) in ORDER.iter().enumerate() {
            let k = lv[band].clamp(0.0, 1.0);
            let h = 3.0 + 13.0 * k;
            let r = Rect::new(x + slot as f32 * 4.4, cy - h / 2.0, 2.6, h);
            // Lighter at the tips, full color at the middle; louder bars glow brighter.
            let top = [c[0] + (1.0 - c[0]) * 0.55, c[1] + (1.0 - c[1]) * 0.55, c[2] + (1.0 - c[2]) * 0.55, 0.75 + 0.25 * k];
            let mid = [c[0], c[1], c[2], 0.75 + 0.25 * k];
            g.linear(r, 1.3, &[(0.0, top), (0.5, mid), (1.0, top)], true);
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
        // Only the words (no times): to read, scrolled with the wheel.
        if let lyrics::State::Plain(lines) = &*self.lyrics.state.lock().unwrap() {
            let top = self.plain_scroll.get().min(lines.len().saturating_sub(1));
            g.clip(Rect::new(264.0, 8.0, 234.0, 128.0));
            for (k, text) in lines.iter().skip(top).take(5).enumerate() {
                let y = 30.0 + k as f32 * 21.0;
                g.text(text, 264.0, y, 234.0, 13.0, 600, white(0.85), Align::Start);
            }
            g.unclip();
            let more = if lines.len() > 5 { " \u{00b7} scroll for more" } else { "" };
            g.text(&format!("Not synced{more}"), 264.0, 128.0, 234.0, 9.5, 500, white(0.4), Align::Start);
            return;
        }
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
        g.display(&mmss(pos), l.el_x, 92.0, 50.0, 12.0, 500, white(0.85), Align::Start);
        g.display(&format!("-{}", mmss(end - pos)), l.rem_r - 60.0, 92.0, 60.0, 12.0, 500, white(0.85), Align::End);
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
        if self.nova() && t.is_some() && !std::ptr::eq(l, &SPLIT) {
            // The mini player: a small window with a smaller one in its corner.
            let px = l.bubble + 26.0;
            let r = Rect::centered(px, cy - 1.0, 18.0, 13.0);
            let busy = self.mini_finding.is_some();
            let c = white(if busy { 0.4 } else if self.hovered(r.grow(4.0)) { 1.0 } else { 0.85 });
            g.stroke_rrect(r, 3.0, 1.5, c, false);
            g.fill_rrect(Rect::new(r.x + r.w - 9.0, r.y + r.h - 7.0, 7.0, 5.0), 1.5, c);
            self.hit(r.grow(6.0), Action::MiniPlayer);
        }

        let tri = |cx: f32, dir: f32| {
            for k in [-1.0f32, 0.0] {
                let x = cx + dir * k * 8.0;
                g.fill_poly(&[(x - dir * 4.0, cy - 5.5), (x + dir * 4.5, cy), (x - dir * 4.0, cy + 5.5)], c);
            }
        };
        let s = self.press_disc(Action::Previous, l.prev, cy, 15.0);
        g.scale_at(l.prev, cy, s);
        tri(l.prev + 4.0, -1.0);
        g.unscale();
        let s = self.press_disc(Action::Next, l.next, cy, 15.0);
        g.scale_at(l.next, cy, s);
        tri(l.next - 4.0, 1.0);
        g.unscale();
        let s = self.press_disc(Action::PlayPause, l.play, cy, 16.0);
        g.scale_at(l.play, cy, s);
        let playing = t.is_some_and(|t| t.playing);
        if playing {
            g.fill_rrect(Rect::new(l.play - 6.0, cy - 8.0, 4.4, 16.0), 1.5, c);
            g.fill_rrect(Rect::new(l.play + 1.6, cy - 8.0, 4.4, 16.0), 1.5, c);
        } else {
            g.fill_poly(&[(l.play - 5.0, cy - 8.5), (l.play + 8.0, cy), (l.play - 5.0, cy + 8.5)], c);
        }
        g.unscale();
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

        g.display(&mmss(left.as_millis() as i64 + 999), 200.0, 51.0, 130.0, 30.0, 700, WHITE, Align::Center);
        let reset = Rect::centered(330.0, 50.0, 22.0, 22.0);
        g.icon('\u{E72C}', 330.0, 50.0, 13.0, white(if self.hovered(reset) { 1.0 } else { 0.8 }));
        self.hit(reset, Action::PomoReset);
        let label = if self.pomo.brk { "Break time" } else { "Focus time" };
        g.text(label, 200.0, 75.0, 130.0, 12.0, 400, white(0.55), Align::Center);

        let sel = |on: bool| white(if on { 0.2 } else { 0.12 });
        // A row sized from its labels, ending at the page's right edge.
        let brk_icon = Ico::Glyph('\u{EC32}');
        let (w1, w2, w3) = (self.pill_w(brk_icon, "Short Break"), 46.0, self.pill_w(Ico::Target, "Focus Time"));
        let x = 384.0 - (w1 + w2 + w3 + 12.0);
        self.pill(Rect::new(x, 92.0, w1, 28.0), sel(self.pomo.brk), brk_icon, "Short Break", WHITE, Some(Action::PomoMode(true)));
        let icon = if self.pomo.running() { Ico::Pause } else { Ico::Play };
        self.pill(Rect::new(x + w1 + 6.0, 92.0, w2, 28.0), RED, icon, "", WHITE, Some(Action::PomoToggle));
        self.pill(Rect::new(x + w1 + w2 + 12.0, 92.0, w3, 28.0), sel(!self.pomo.brk), Ico::Target, "Focus Time", WHITE, Some(Action::PomoMode(false)));
    }

    // ----- water ------------------------------------------------------------------------

    fn water_page(&self) {
        let g = &self.gfx;
        let w = &self.store.water;
        self.header("Water ", Some(("Reminder", SKY)));
        let frac = w.drunk_ml as f32 / w.goal_ml.max(1) as f32;

        let x = 44.0 + g.display(&liters(w.drunk_ml), 44.0, 48.0, 100.0, 26.0, 700, WHITE, Align::Start);
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
            " · Reminders off".to_owned()
        } else if frac >= 1.0 {
            " · Goal reached".to_owned()
        } else {
            w.next_reminder().map_or_else(String::new, |t| format!(" · Next {t}"))
        };
        g.text(&rest, tx, 71.0, 150.0, 12.0, 400, white(0.55), Align::Start);

        g.fill_rrect(Rect::new(44.0, 80.0, 152.0, 5.0), 2.5, white(0.15));
        if frac > 0.0 {
            g.linear(Rect::new(44.0, 80.0, 152.0 * frac.min(1.0), 5.0), 2.5, &[(0.0, rgb(0x66, 0xdb, 0xff)), (1.0, rgb(0x2d, 0x6b, 0xff))], false);
        }

        let gear = Ico::Glyph('\u{E713}');
        let (w1, w2) = (self.pill_w(Ico::Drop, "Drink"), self.pill_w(gear, "Settings"));
        self.pill(Rect::new(44.0, 98.0, w1, 26.0), rgb(0x1d, 0x74, 0xd8), Ico::Drop, "Drink", WHITE, Some(Action::Drink));
        self.pill(Rect::new(44.0 + w1 + 6.0, 98.0, w2, 26.0), white(0.12), gear, "Settings", WHITE, Some(Action::WaterSettings(true)));

        g.line(207.0, 34.0, 207.0, 120.0, 1.0, white(0.12));
        for i in 0..2 {
            let r = Rect::centered(360.0 + i as f32 * 8.0, 41.0, 4.0, 4.0);
            g.fill_circle(r.x + 2.0, r.y + 2.0, 2.0, if self.water_sub == i { WHITE } else { white(0.4) });
            self.hit(r.grow(4.0), Action::WaterSub(i));
        }
        if self.water_sub == 0 {
            let (cx, cy) = (293.0, 77.0);
            g.arc(cx, cy, 30.0, 210.0, 300.0, 5.0, white(0.12));
            let ring = if w.enabled { SKY } else { white(0.4) };
            g.arc(cx, cy, 30.0, 210.0, 300.0 * frac.min(1.0), 5.0, ring);
            if frac > 0.0 {
                // A bead at the end of the arc.
                let a = (210.0 + 300.0 * frac.min(1.0) - 90.0f32).to_radians();
                g.fill_circle(cx + 30.0 * a.cos(), cy + 30.0 * a.sin(), 3.6, ring);
            }
            g.display(&format!("{:.0}", (frac * 100.0).min(999.0)), cx - 30.0, cy - 4.0, 60.0, 22.0, 700, WHITE, Align::Center);
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
        let top = (w.goal_ml * 2).max(1) as f32;
        for (i, ml) in [(0.0, w.goal_ml * 2), (0.5, w.goal_ml), (1.0, 0)] {
            let y = y0 + ch * i;
            g.line(x0, y, x0 + cw + 6.0, y, 1.0, white(0.14));
            let label = if ml == 0 { "0 ml".to_owned() } else { ml.to_string() };
            g.text(&label, x0 + cw + 10.0, y, 40.0, 9.5, 400, white(0.5), Align::Start);
        }
        let hours = w.chart_hours();
        let now_h = win::hour() as u32;
        let step = cw / hours.len() as f32;
        let bar_w = 7.0_f32.min(step - 2.0).max(2.0);
        for (k, &hour) in hours.iter().enumerate() {
            let ml = w.ml_in_hour(hour);
            let x = x0 + k as f32 * step + (step - bar_w) / 2.0;
            let h = (ml as f32 / top * ch).clamp(3.0, ch);
            let current = hour == now_h;
            let c = if current { SKY } else if ml > 0 { rgba(0x3f, 0xb4, 0xff, 0.55) } else { white(0.1) };
            g.fill_rrect(Rect::new(x, y0 + ch - h, bar_w, h), bar_w / 2.0, c);
            if current && ml > 0 {
                // A bubble with this hour's amount above its bar.
                let label = format!("{ml} ml");
                let bw = g.measure(&label, 9.5, 700) + 12.0;
                let bx = (x + bar_w / 2.0 - bw / 2.0).clamp(x0 - 4.0, x0 + cw + 6.0 - bw);
                let by = (y0 + ch - h - 20.0).max(y0 - 16.0);
                g.fill_rrect(Rect::new(bx, by, bw, 15.0), 7.5, rgb(0x1d, 0x74, 0xd8));
                g.text(&label, bx, by + 7.5, bw, 9.5, 700, WHITE, Align::Center);
            }
            if hours.len() <= 4 || k == 0 || k == hours.len() - 1 {
                g.text(&format!("{hour:02}"), x + bar_w / 2.0 - 10.0, y0 + ch + 10.0, 20.0, 9.5, 400, white(0.5), Align::Center);
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
        g.text(&format!("{} ml each", w.per_ml()), 180.0, 50.0, 110.0, 9.5, 400, white(0.5), Align::Start);

        for (i, ml) in [1500u32, 2000, 2500, 3000].into_iter().enumerate() {
            let r = Rect::new(78.0 + i as f32 * 54.5, 69.0, 48.0, 22.0);
            let on = w.goal_ml == ml;
            g.fill_rrect(r, 11.0, if on { BLUE } else { white(if self.hovered(r) { 0.2 } else { 0.12 }) });
            g.text(&format!("{} L", liters(ml)), r.x, r.y + 11.0, r.w, 12.0, if on { 700 } else { 400 }, WHITE, Align::Center);
            self.hit(r, Action::Goal(ml));
        }

        let hm = |m: u32| win::hm_text((m / 60) as u16, (m % 60) as u16);
        self.stepper(86.0, 162.0, 110.0, &hm(w.from_min), Action::FromH(-1), Action::FromH(1));
        g.text("to", 174.0, 110.0, 20.0, 12.0, 400, white(0.55), Align::Center);
        self.stepper(205.0, 282.0, 110.0, &hm(w.to_min), Action::ToH(-1), Action::ToH(1));

        let card = Rect::new(306.0, 36.0, 92.0, 64.0);
        g.fill_rrect(card, 10.0, white(0.08));
        let n = w.reminders();
        g.text(&n.to_string(), card.x, 52.0, card.w, 20.0, 700, SKY, Align::Center);
        g.text("reminders", card.x, 67.0, card.w, 9.5, 400, white(0.55), Align::Center);
        let refills = w.refills().to_string();
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

    // ----- prayer times -----------------------------------------------------------------

    /// The choices that change the times.
    fn prayer_calc(&self) -> prayer::Calc {
        if !self.nova() {
            return prayer::Calc::default();
        }
        prayer::Calc { hijri_adjust: self.store.prayer.hijri_adjust, hanafi: self.store.prayer.hanafi }
    }

    /// The cities to choose from: Classic has only Egypt's (the first 33).
    fn prayer_choices(&self) -> usize {
        if self.nova() { prayer::CHOICES } else { EGYPT_CITIES }
    }

    /// Ramadan at the place (HILAL_RAMADAN_TEST makes any day one, for testing).
    fn ramadan(&self, y: i32, m: u32, d: u32) -> bool {
        self.nova()
            && (std::env::var_os("HILAL_RAMADAN_TEST").is_some()
                || prayer::hijri(y, m, d, self.store.prayer.hijri_adjust).is_some_and(|h| h.1 == 9))
    }

    /// The place the prayer times are for.
    fn prayer_place(&self) -> prayer::Place {
        if !self.nova() {
            // Classic: Egypt's cities only, on the PC's own clock.
            let city = if self.store.prayer.city < EGYPT_CITIES { self.store.prayer.city } else { 0 };
            return prayer::Place { tz: String::new(), ..prayer::place(city, None) };
        }
        prayer::place(self.store.prayer.city, self.store.prayer.auto.as_ref())
    }

    /// With "Auto" chosen, looks the PC's place up once a day (in the background) and keeps
    /// the answer; when every service fails, it tries again half an hour later.
    fn keep_located(&mut self) {
        if !self.nova() {
            return;
        }
        if let Some(rx) = &self.locating {
            match rx.try_recv() {
                Ok(found) => {
                    self.locating = None;
                    if let Some(found) = found {
                        trace(&format!("[prayer] located: {} ({})", found.city, found.country));
                        self.store.prayer.auto = Some(found);
                        self.store.save();
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.locating = None,
                Err(_) => {}
            }
            return;
        }
        let stale = self.store.prayer.auto.as_ref().map_or(true, |a| a.day != prayer::today_key());
        if self.store.prayer.city >= prayer::AUTO && stale && self.locate_at.elapsed() > Duration::from_secs(30 * 60) {
            self.locate_at = Instant::now();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(prayer::detect());
            });
            self.locating = Some(rx);
        }
    }

    fn prayer_page(&self) {
        let g = &self.gfx;
        self.header("Prayer ", Some(("Times", AMBER)));
        let place = self.prayer_place();
        let auto = self.nova() && self.store.prayer.city >= prayer::AUTO;
        let city = match (auto, &self.store.prayer.auto) {
            (true, Some(a)) => format!("Auto · {}", a.city),
            (true, None) if place.approx => format!("Auto · ~{}", place.name),
            (true, None) => "Auto · finding\u{2026}".to_owned(),
            (false, _) => place.name.clone(),
        };
        let city = city.as_str();
        // The day and the clock at the place (its own time zone, like a mosque timetable).
        let calc = self.prayer_calc();
        let (y, m, d, mut now) = prayer::local_now(&place);
        let mut t = prayer::day_times(&place, y, m, d, calc);

        // The next of the five prayers (sunrise is listed but isn't one), and the one before
        // it, in seconds since today's midnight. After Isha, the next is tomorrow's Fajr.
        const FIVE: [usize; 5] = [0, 2, 3, 4, 5];
        let (next, next_at, prev_at) = match FIVE.iter().position(|&i| t[i] * 60 > now) {
            Some(k) => {
                let prev = if k == 0 { (t[5] - 1440) * 60 } else { t[FIVE[k - 1]] * 60 };
                (FIVE[k], t[FIVE[k]] * 60, prev)
            }
            None => {
                // Past Isha: show tomorrow's day, counting from a midnight already behind us.
                let (y2, m2, d2) = prayer::next_day(y, m, d);
                let isha = t[5];
                t = prayer::day_times(&place, y2, m2, d2, calc);
                now -= 86400;
                (0, t[0] * 60, (isha - 1440) * 60)
            }
        };

        // Left: the next prayer, when, how long until it, and the city.
        // In Ramadan (as Nova Island has it): "Ramadan Kareem" over the next prayer, the
        // time and countdown to iftar (until Maghrib) or to the next imsak (ten minutes
        // before Fajr), and Maghrib marked as iftar.
        let ramadan = self.ramadan(y, m, d);
        let hijri = self.nova().then(|| prayer::hijri(y, m, d, calc.hijri_adjust)).flatten().map(|h| h.0);
        if ramadan {
            // The whole line: "Ramadan Kareem · the Hijri date".
            let line = format!("\u{0631}\u{0645}\u{0636}\u{0627}\u{0646} \u{0643}\u{0631}\u{064a}\u{0645}{}", hijri.map_or(String::new(), |h| format!(" \u{00b7} {}", h.rsplit_once(' ').map_or(h.as_str(), |(day, _)| day))));
            g.text(&line, 44.0, 41.0, 152.0, 10.5, 600, AMBER, Align::Start);
        } else {
            g.text("Next prayer", 44.0, 41.0, 150.0, 10.5, 500, white(0.55), Align::Start);
            // Today's Hijri date, small, at the other end of the line.
            if let Some(hijri) = hijri {
                g.text(&hijri, 100.0, 41.0, 96.0, 10.0, 500, rgba(0xff, 0xc4, 0x3d, 0.75), Align::End);
            }
        }
        let nw = g.text(prayer::NAMES[next], 44.0, 60.0, 150.0, 21.0, 700, WHITE, Align::Start);
        let at = next_at / 60 % 1440;
        g.display(&win::hm_text((at / 60) as u16, (at % 60) as u16), 44.0 + nw + 8.0, 61.5, 100.0, 12.5, 600, white(0.6), Align::Start);
        let left = (next_at - now).max(0);
        let until = if left >= 3600 { format!("in {}h {}m", left / 3600, left / 60 % 60) } else { format!("in {}m {:02}s", left / 60, left % 60) };
        g.display(&until, 44.0, 78.0, 150.0, 11.5, 600, AMBER, Align::Start);
        if ramadan {
            let today = prayer::day_times(&place, y, m, d, calc);
            let (yy, mm, dd) = prayer::next_day(y, m, d);
            // (Past Isha `now` counts from tomorrow's midnight; this is the clock today.)
            let real_now = now.rem_euclid(86_400);
            let imsak = (today[0] - 10) * 60;
            let iftar = today[4] * 60;
            let (name, at) = if real_now >= imsak && real_now < iftar {
                ("\u{0627}\u{0644}\u{0625}\u{0641}\u{0637}\u{0627}\u{0631}", iftar)
            } else if real_now < imsak {
                ("\u{0627}\u{0644}\u{0625}\u{0645}\u{0633}\u{0627}\u{0643}", imsak)
            } else {
                let tomorrow = prayer::day_times(&place, yy, mm, dd, calc);
                ("\u{0627}\u{0644}\u{0625}\u{0645}\u{0633}\u{0627}\u{0643}", 86_400 + (tomorrow[0] - 10) * 60)
            };
            let hm = at / 60 % 1440;
            // The word, then the time to its left (two pieces, so the time reads left to right).
            let time = win::hm_text((hm / 60) as u16, (hm % 60) as u16);
            let light = rgb(0xff, 0xe0, 0x8a);
            let nw = g.measure(name, 10.5, 600) + 2.0;
            let tw = g.measure(&time, 10.5, 600) + 2.0;
            g.text(name, 196.0 - nw, 78.0, nw, 10.5, 600, light, Align::Start);
            g.text(&time, 196.0 - nw - 4.0 - tw, 78.0, tw, 10.5, 600, light, Align::Start);
        }
        let frac = ((now - prev_at) as f32 / (next_at - prev_at).max(1) as f32).clamp(0.0, 1.0);
        g.fill_rrect(Rect::new(44.0, 88.0, 152.0, 4.0), 2.0, white(0.15));
        if frac > 0.0 {
            g.linear(Rect::new(44.0, 88.0, 152.0 * frac, 4.0), 2.0, &[(0.0, rgb(0xff, 0xe0, 0x8a)), (1.0, AMBER)], false);
        }
        let pin = Ico::Glyph('\u{E81D}');
        let cw = self.pill_w(pin, city).min(152.0 - 34.0 - 6.0);
        self.pill(Rect::new(44.0, 99.0, cw, 24.0), white(0.12), pin, city, WHITE, Some(Action::PrayerCity(1)));
        // The bell: an alert at each prayer time, on or off.
        let on = self.store.prayer.alerts;
        let bell = Ico::Glyph(if on { '\u{EA8F}' } else { '\u{E7ED}' });
        let fill = if on { rgba(0xff, 0xc4, 0x3d, 0.28) } else { white(0.12) };
        self.pill(Rect::new(44.0 + cw + 6.0, 99.0, 34.0, 24.0), fill, bell, "", if on { AMBER } else { white(0.6) }, Some(Action::PrayerAlerts));

        // Right: all six times; the next one lit, the ones gone by dimmed.
        for (i, name) in prayer::NAMES.iter().enumerate() {
            let cell = Rect::new(204.0 + (i % 3) as f32 * 61.0, 34.0 + (i / 3) as f32 * 45.0, 58.0, 41.0);
            let is_next = i == next;
            let past = t[i] * 60 <= now && !is_next;
            if is_next {
                g.fill_rrect(cell, 11.0, rgba(0xff, 0xc4, 0x3d, 0.16));
                g.stroke_rrect(cell, 11.0, 1.2, rgba(0xff, 0xc4, 0x3d, 0.55), false);
            } else {
                g.fill_rrect(cell, 11.0, white(0.07));
            }
            let name_c = if is_next { AMBER } else { white(if past { 0.35 } else { 0.6 }) };
            let name = if ramadan && i == 4 { "Iftar" } else { name };
            g.text(name, cell.x, cell.y + 13.0, cell.w, 10.0, 600, name_c, Align::Center);
            let (hm, ampm) = win::hm((t[i] / 60 % 24) as u16, (t[i] % 60) as u16);
            let c = white(if past { 0.45 } else { 1.0 });
            let tw = g.measure_display(&hm, 13.5, 700);
            let aw = ampm.map_or(0.0, |a| g.measure_display(a, 8.5, 600) + 2.5);
            let x = cell.x + (cell.w - tw - aw) / 2.0;
            let cy = cell.y + 28.0;
            g.display(&hm, x, cy, tw + 2.0, 13.5, 700, c, Align::Start);
            if let Some(a) = ampm {
                g.display(a, x + tw + 2.5, cy + 1.5, aw, 8.5, 600, [c[0], c[1], c[2], c[3] * 0.72], Align::Start);
            }
        }
    }

    // ----- Claude Code ------------------------------------------------------------------

    fn claude_page(&self) {
        let g = &self.gfx;
        self.header("Claude ", Some(("Code", CLAUDE)));
        let list = &self.claude_list;
        let working = list.iter().filter(|s| s.state == claude::State::Working).count();
        if working > 0 {
            let label = format!("{working} working");
            g.text(&label, 300.0, 19.0, 84.0, 10.5, 600, CLAUDE, Align::End);
        }
        if list.is_empty() {
            spark(g, 210.0, 66.0, 13.0, white(0.3), 0.0);
            g.text("No Claude Code sessions in the last 3 days", 44.0, 96.0, 340.0, 11.0, 400, white(0.45), Align::Center);
            return;
        }
        let t = self.t0.elapsed().as_secs_f32();
        let first = self.claude_scroll.min(list.len().saturating_sub(3));
        for (row, (i, s)) in list.iter().enumerate().skip(first).take(3).enumerate() {
            let r = Rect::new(44.0, 33.0 + row as f32 * 31.0, 340.0, 27.0);
            let open = self.popup.session.as_deref() == Some(s.id.as_str());
            let fill = if open { rgba(0xd9, 0x77, 0x57, 0.22) } else if self.hovered(r) { white(0.11) } else { white(0.06) };
            g.fill_rrect(r, 9.0, fill);
            let cy = r.y + r.h / 2.0;
            status_icon(g, s.state, r.x + 14.0, cy, t);
            let (done, total) = s.tasks_done();
            let right = if total > 0 { format!("{done}/{total}  ·  {}", claude::ago(s.age())) } else { claude::ago(s.age()) };
            let rw = g.measure(&right, 10.0, 500);
            g.text(&right, r.x + r.w - 22.0 - rw, cy, rw + 2.0, 10.0, 500, white(0.5), Align::Start);
            g.icon('\u{E76C}', r.x + r.w - 11.0, cy + 0.5, 8.0, white(0.4));
            g.text(&s.title, r.x + 28.0, cy, r.w - 60.0 - rw, 11.5, 600, WHITE, Align::Start);
            self.hit(r, Action::ClaudeOpen(i));
        }
        if list.len() > 3 {
            // Where the three rows sit in the whole list.
            let h = 90.0 * 3.0 / list.len() as f32;
            let y = 33.0 + (90.0 - h) * first as f32 / (list.len() - 3) as f32;
            g.fill_rrect(Rect::new(388.0, y, 2.5, h), 1.25, white(0.25));
        }
    }

    fn open_popup(&mut self, id: String) {
        trace(&format!("[popup] open {id} (was open {})", self.popup.session.is_some()));
        if self.popup.session.is_none() {
            self.popup.shown = 0.0;
            self.popup.speed = 0.0;
            self.popup.last = Instant::now();
        }
        self.popup.showing = true;
        self.popup_opened_at = Some(Instant::now());
        self.popup.away_at = None;
        self.popup.session = Some(id);
        unsafe {
            let _ = ShowWindow(self.popup.hwnd, SW_SHOWNOACTIVATE);
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            let _ = SetWindowPos(self.popup.hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
        }
        self.gfx.move_surface(self.popup.surface, self.popup.pos);
        self.draw_popup();
    }

    /// Folds the popup back into the notch (the frame loop hides it once it's gone).
    pub fn close_popup(&mut self) {
        trace("[popup] close");
        self.popup_focus(false);
        self.popup.away_at = None;
        self.popup.showing = false;
    }

    /// Whether a screen point (physical px) on the popup should drag it: its header,
    /// except the close button.
    pub fn popup_drags(&self, x: i32, y: i32) -> bool {
        let k = self.gfx.scale;
        let (lx, ly) = ((x - self.popup.pos.0) as f32 / k, (y - self.popup.pos.1) as f32 / k);
        ly < 58.0 && !popup_close_rect().grow(4.0).contains(lx, ly) && !popup_pin_rect().grow(4.0).contains(lx, ly)
    }

    pub fn popup_click(&mut self, x: f32, y: f32) {
        trace(&format!("[popup] click at {x:.0},{y:.0}"));
        if popup_close_rect().grow(4.0).contains(x, y) {
            self.close_popup();
        } else if popup_pin_rect().grow(4.0).contains(x, y) {
            self.popup.pinned = !self.popup.pinned;
        } else if popup_send_rect().grow(2.0).contains(x, y) {
            self.popup_send();
        } else if popup_field_rect().contains(x, y) {
            self.popup_focus(true);
        } else {
            self.popup_focus(false);
        }
    }

    pub fn popup_typing(&self) -> bool {
        self.popup.typing
    }

    /// Gives the message box the keyboard (the popup becomes a normal, focusable window
    /// for that), or takes it back.
    pub fn popup_focus(&mut self, on: bool) {
        if self.popup.typing == on {
            return;
        }
        self.popup.typing = on;
        unsafe {
            let ex = GetWindowLongPtrW(self.popup.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            SetWindowLongPtrW(self.popup.hwnd, GWL_EXSTYLE, if on { ex & !flag } else { ex | flag });
            if on {
                let _ = SetForegroundWindow(self.popup.hwnd);
                let _ = SetFocus(self.popup.hwnd);
            }
        }
    }

    pub fn popup_key(&mut self, vk: u16) {
        match vk {
            0x08 => {
                self.popup.draft.pop();
            }
            0x0D => self.popup_send(),
            0x1B => self.popup_focus(false),
            _ => {}
        }
    }

    pub fn popup_char(&mut self, c: char) {
        if c == '\u{16}' {
            // Ctrl+V
            if let Some(t) = crate::claude_app::clipboard_text() {
                self.popup.draft.push_str(&t.replace(['\r', '\n'], " "));
            }
        } else if !c.is_control() && self.popup.draft.chars().count() < 2000 {
            self.popup.draft.push(c);
        }
    }

    /// Hands the message to the Claude app (pasted in its box, for you to send there).
    fn popup_send(&mut self) {
        let text = self.popup.draft.trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.popup_focus(false);
        self.popup.note = Some(match crate::claude_app::paste(&text) {
            Ok(()) => {
                self.popup.draft.clear();
                ("Pasted in the Claude app \u{2014} press Enter there to send".to_owned(), Instant::now(), true)
            }
            Err(e) => (e.to_owned(), Instant::now(), false),
        });
    }

    pub fn popup_moved(&mut self) {
        let mut r = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = GetWindowRect(self.popup.hwnd, &mut r);
        }
        self.popup.pos = (r.left, r.top);
        self.gfx.move_surface(self.popup.surface, self.popup.pos);
    }

    /// A glass card for one session: what it is, what it's doing, its task list, and the
    /// latest exchange.
    fn draw_popup(&mut self) {
        let dt = self.popup.last.elapsed().as_secs_f32();
        self.popup.last = Instant::now();
        self.popup.step(dt);
        self.popup.frames += 1;
        if self.popup.fps_at.elapsed() >= Duration::from_secs(1) {
            if let Some(path) = std::env::var_os("HILAL_FPS") {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let ms = |d: Duration| d.as_secs_f32() * 1000.0;
                    let _ = writeln!(
                        f,
                        "  [popup] {} fps   longest: backdrop {:.1} ms, notch {:.1} ms, popup {:.1} ms",
                        self.popup.frames,
                        ms(self.parts.0),
                        ms(self.parts.1),
                        ms(self.parts.2)
                    );
                    self.parts = Default::default();
                }
            }
            self.popup.frames = 0;
            self.popup.fps_at = Instant::now();
        }
        let Some(s) = self.popup.session.as_ref().and_then(|id| self.claude_list.iter().find(|s| &s.id == id)).cloned() else {
            // Gone from the list (too old): nothing to show.
            if self.claude_list.iter().all(|x| Some(&x.id) != self.popup.session.as_ref()) && !self.claude_list.is_empty() {
                self.close_popup();
            }
            return;
        };
        if self.gfx.begin(self.popup.surface).is_err() {
            return;
        }
        let g = &self.gfx;
        let t = self.t0.elapsed().as_secs_f32();
        // It grows out of the notch above it: from a little smaller, a little higher, and
        // transparent; closing runs the same way back.
        let v = self.popup.shown;
        g.push_layer(v.clamp(0.0, 1.0).powf(0.8), 0.9 + 0.1 * v, PP_W / 2.0, 0.0, -18.0 * (1.0 - v));
        let pane = Rect::new(1.0, 1.0, PP_W - 2.0, PP_H - 2.0);
        if self.glass_live {
            g.glass_rrect(pane, 26.0);
        } else {
            g.fill_rrect(pane, 26.0, rgba(8, 8, 12, 0.95));
            g.stroke_rrect(pane, 26.0, 1.0, white(0.18), false);
        }
        // A soft wash of Claude's color from the top-left while it works.
        if s.state == claude::State::Working {
            let breath = 0.5 + 0.5 * (t * 1.6).sin();
            g.radial(40.0, 30.0, 150.0, 90.0, rgba(0xd9, 0x77, 0x57, 0.10 + 0.08 * breath));
        }

        // Header: the spark, the title, where and how it's doing; the close button.
        let spin = if s.state == claude::State::Working { t * 90.0 } else { 0.0 };
        spark(g, 32.0, 30.0, 11.0, CLAUDE, spin);
        g.text(&s.title, 54.0, 24.0, PP_W - 104.0, 15.0, 700, WHITE, Align::Start);
        let (label, c) = state_label(s.state);
        let lw = g.text(label, 54.0, 43.0, 120.0, 10.5, 600, c, Align::Start);
        let meta = format!("·  {}  ·  {}", s.project, claude::ago(s.age()));
        g.text(&meta, 54.0 + lw + 6.0, 43.0, PP_W - 120.0 - lw, 10.5, 400, white(0.5), Align::Start);
        let over = |r: Rect| self.popup.hover.is_some_and(|(x, y)| r.grow(4.0).contains(x, y));
        let close = popup_close_rect();
        let lit = over(close);
        g.fill_circle(close.x + close.w / 2.0, close.y + close.h / 2.0, 11.0, white(if lit { 0.22 } else { 0.1 }));
        g.icon('\u{E711}', close.x + close.w / 2.0, close.y + close.h / 2.0 + 0.5, 8.5, white(if lit { 1.0 } else { 0.7 }));
        // The pin: filled in Claude's color while pinned.
        let pin = popup_pin_rect();
        let (pcx, pcy) = (pin.x + pin.w / 2.0, pin.y + pin.h / 2.0);
        if self.popup.pinned {
            g.fill_circle(pcx, pcy, 11.0, rgba(0xd9, 0x77, 0x57, if over(pin) { 0.45 } else { 0.32 }));
            g.icon('\u{E842}', pcx, pcy + 0.5, 9.5, CLAUDE);
        } else {
            g.fill_circle(pcx, pcy, 11.0, white(if over(pin) { 0.22 } else { 0.1 }));
            g.icon('\u{E718}', pcx, pcy + 0.5, 9.5, white(if over(pin) { 1.0 } else { 0.7 }));
        }
        g.line(18.0, 60.0, PP_W - 18.0, 60.0, 1.0, white(0.1));

        let mut y = 72.0;
        // What it's doing this moment.
        if s.state == claude::State::Working {
            let now = if s.doing.is_empty() { "Thinking\u{2026}".to_owned() } else { s.doing.clone() };
            g.fill_rrect(Rect::new(18.0, y, PP_W - 36.0, 26.0), 9.0, rgba(0xd9, 0x77, 0x57, 0.14));
            status_icon(g, claude::State::Working, 32.0, y + 13.0, t);
            g.text(&now, 46.0, y + 13.0, PP_W - 82.0, 11.5, 500, WHITE, Align::Start);
            y += 34.0;
        }

        // Tasks, with a bar for how far along they are.
        if !s.tasks.is_empty() {
            let (done, total) = s.tasks_done();
            g.text("TASKS", 20.0, y + 6.0, 80.0, 9.5, 700, white(0.45), Align::Start);
            g.text(&format!("{done} of {total}"), PP_W - 120.0, y + 6.0, 100.0, 9.5, 600, white(0.45), Align::End);
            let bar = Rect::new(66.0, y + 5.0, PP_W - 196.0, 3.0);
            g.fill_rrect(bar, 1.5, white(0.1));
            g.fill_rrect(Rect::new(bar.x, bar.y, bar.w * done as f32 / total.max(1) as f32, 3.0), 1.5, GREEN);
            y += 16.0;
            let room = if s.state == claude::State::Working { 6 } else { 7 };
            // Keep the active task in view when the list is long.
            let active = s.tasks.iter().position(|t| t.1 == claude::Task::Active).unwrap_or(0);
            let first = active.saturating_sub(room / 2).min(s.tasks.len().saturating_sub(room));
            for (name, st) in s.tasks.iter().skip(first).take(room) {
                let cy = y + 9.0;
                match st {
                    claude::Task::Done => {
                        g.fill_circle(28.0, cy, 6.5, rgba(0x2f, 0xd0, 0x6b, 0.9));
                        g.icon('\u{E73E}', 28.0, cy + 0.5, 7.5, BLACK);
                    }
                    claude::Task::Active => {
                        g.stroke_circle(28.0, cy, 6.0, 1.6, rgba(0xd9, 0x77, 0x57, 0.3));
                        g.arc(28.0, cy, 6.0, t * 300.0, 110.0, 1.8, CLAUDE);
                    }
                    claude::Task::Pending => g.stroke_circle(28.0, cy, 6.0, 1.4, white(0.3)),
                }
                let c = match st {
                    claude::Task::Done => white(0.5),
                    claude::Task::Active => WHITE,
                    claude::Task::Pending => white(0.75),
                };
                g.text(name, 42.0, cy, PP_W - 62.0, 11.5, if *st == claude::Task::Active { 600 } else { 400 }, c, Align::Start);
                y += 19.0;
            }
            y += 8.0;
        }

        // The latest exchange: you, then Claude.
        g.line(18.0, y, PP_W - 18.0, y, 1.0, white(0.08));
        y += 10.0;
        let bottom = popup_field_rect().y - 10.0;
        if !s.prompt.is_empty() && bottom - y > 40.0 {
            g.text("YOU", 20.0, y + 6.0, 60.0, 9.5, 700, white(0.45), Align::Start);
            y += 14.0;
            y += g.paragraph(&s.prompt, 20.0, y, PP_W - 40.0, 11.5, 400, white(0.85), 2) + 10.0;
        }
        if !s.reply.is_empty() && bottom - y > 30.0 {
            spark(g, 24.0, y + 6.0, 4.5, CLAUDE, 0.0);
            g.text("CLAUDE", 33.0, y + 6.0, 80.0, 9.5, 700, CLAUDE, Align::Start);
            y += 14.0;
            let lines = (((bottom - y) / (11.5 * 1.42)) as usize).clamp(1, 6);
            g.paragraph(&s.reply, 20.0, y, PP_W - 40.0, 11.5, 400, WHITE, lines);
        }

        // The message box along the bottom, with its send button.
        let field = popup_field_rect();
        let typing = self.popup.typing;
        g.fill_rrect(field, 18.0, white(if typing { 0.14 } else if over(field) { 0.1 } else { 0.07 }));
        if typing {
            g.stroke_rrect(field, 18.0, 1.2, rgba(0xd9, 0x77, 0x57, 0.7), false);
        }
        let text_x = field.x + 16.0;
        let text_w = field.w - 56.0;
        let note = self.popup.note.as_ref().filter(|n| n.1.elapsed() < Duration::from_secs(6));
        if !self.popup.draft.is_empty() {
            // The end of the message stays in view as it grows.
            let d = &self.popup.draft;
            let dw = g.measure(d, 12.0, 400);
            let align = if dw > text_w { Align::End } else { Align::Start };
            let w = g.text(d, text_x, field.y + field.h / 2.0, text_w, 12.0, 400, WHITE, align);
            if typing && (t * 2.0).fract() < 0.55 && !crate::gfx::is_rtl(d) {
                let cx = text_x + w.min(text_w) + 1.5;
                g.fill_rrect(Rect::new(cx, field.y + 10.0, 1.6, field.h - 20.0), 0.8, CLAUDE);
            }
        } else if let Some((msg, _, ok)) = note {
            g.text(msg, text_x, field.y + field.h / 2.0, text_w, 11.5, 500, if *ok { GREEN } else { rgb(0xff, 0x8a, 0x8a) }, Align::Start);
        } else {
            g.text("Message Claude\u{2026}", text_x, field.y + field.h / 2.0, text_w, 12.0, 400, white(0.4), Align::Start);
            if typing && (t * 2.0).fract() < 0.55 {
                g.fill_rrect(Rect::new(text_x - 2.0, field.y + 10.0, 1.6, field.h - 20.0), 0.8, CLAUDE);
            }
        }
        let send = popup_send_rect();
        let ready = !self.popup.draft.trim().is_empty();
        let sc = if ready { if over(send) { rgb(0xe8, 0x88, 0x68) } else { CLAUDE } } else { white(0.12) };
        g.fill_circle(send.x + send.w / 2.0, send.y + send.h / 2.0, 13.0, sc);
        g.icon('\u{E74A}', send.x + send.w / 2.0, send.y + send.h / 2.0 + 0.5, 10.0, if ready { WHITE } else { white(0.4) });

        g.pop_layer();

        if let Some(dir) = std::env::var_os("HILAL_DUMP") {
            // One picture as it opens, one with a message being typed, and one halfway
            // through opening and closing.
            let v = self.popup.shown;
            let mid = if self.popup.showing { "claude-popup-opening.png" } else { "claude-popup-closing.png" };
            let mid = PathBuf::from(&dir).join(mid);
            if self.popup.animating() && (0.35..0.65).contains(&v) && !mid.exists() && self.gfx.dump_to.is_none() {
                self.gfx.dump_to = Some(mid);
            } else if let Some(n) = [1u64, 4].into_iter().find(|&n| !PathBuf::from(&dir).join(format!("claude-popup-{n}s.png")).exists() && self.popup_opened_at.is_some_and(|t| t.elapsed() >= Duration::from_secs(n))) {
                self.gfx.dump_to = Some(PathBuf::from(&dir).join(format!("claude-popup-{n}s.png")));
            } else if !self.popup.dumped && !self.popup.animating() {
                self.popup.dumped = true;
                self.gfx.dump_to = Some(PathBuf::from(dir).join("claude-popup.png"));
            } else if self.popup.typing && self.popup.draft.len() > 8 && self.gfx.dump_to.is_none() {
                self.gfx.dump_to = Some(PathBuf::from(dir).join("claude-popup-typing.png"));
            }
        }
        let _ = self.gfx.present();
    }

    // ----- video download ---------------------------------------------------------------

    fn download_page(&self) {
        let g = &self.gfx;
        self.header("Video ", Some(("Download", VIOLET)));
        let status = self.dl_job.as_ref().map(|j| j.status());
        let running = self.dl_job.as_ref().is_some_and(|j| j.running());

        // The link (or, once known, the video's title) and the paste button.
        let field = Rect::new(44.0, 34.0, 256.0, 28.0);
        g.fill_rrect(field, 9.0, white(0.08));
        let title = status.as_ref().map(|s| s.title.as_str()).filter(|t| !t.is_empty());
        let (text, c, weight) = match (title, &self.dl_url) {
            (Some(t), _) => (t.to_owned(), WHITE, 600),
            (None, Some(u)) => (u.trim_start_matches("https://").trim_start_matches("www.").to_owned(), white(0.8), 400),
            (None, None) if self.dl_no_link_at.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) => {
                ("No link on the clipboard. Copy one first".to_owned(), rgb(0xff, 0x9a, 0x8a), 400)
            }
            (None, None) => ("Copy a video link, then press Paste".to_owned(), white(0.4), 400),
        };
        g.text(&text, field.x + 11.0, field.y + 14.0, field.w - 22.0, 11.5, weight, c, Align::Start);
        let paste = Ico::Glyph('\u{E77F}');
        let pw = self.pill_w(paste, "Paste");
        self.pill(Rect::new(384.0 - pw, 34.0, pw, 28.0), white(0.14), paste, "Paste", WHITE, Some(Action::DlPaste));

        // MP3 or MP4, and the button.
        let mut x = 44.0;
        for (k, icon, label) in [(Kind::Mp3, '\u{E8D6}', "MP3"), (Kind::Mp4, '\u{E714}', "MP4")] {
            let w = self.pill_w(Ico::Glyph(icon), label);
            let on = self.dl_kind == k;
            let fill = if on { rgba(0xa8, 0x7b, 0xff, 0.38) } else { white(0.08) };
            let act = (!running).then_some(Action::DlKind(k));
            self.pill(Rect::new(x, 72.0, w, 28.0), fill, Ico::Glyph(icon), label, if on { WHITE } else { white(0.6) }, act);
            x += w + 6.0;
        }
        let (icon, label, fill, act) = if running {
            (Ico::Glyph('\u{E711}'), "Cancel", white(0.14), Some(Action::DlCancel))
        } else if self.dl_url.is_some() {
            (Ico::Glyph('\u{E896}'), "Download", rgb(0x1d, 0x74, 0xd8), Some(Action::DlStart))
        } else {
            (Ico::Glyph('\u{E896}'), "Download", white(0.06), None)
        };
        let bw = self.pill_w(icon, label);
        let label_c = if act.is_some() { WHITE } else { white(0.35) };
        self.pill(Rect::new(384.0 - bw, 72.0, bw, 28.0), fill, icon, label, label_c, act);

        // How it's going.
        let Some(s) = status else {
            g.text("Saves to Downloads\\Hilal", 44.0, 118.0, 340.0, 10.5, 400, white(0.4), Align::Start);
            return;
        };
        let what = if !self.dl_format.is_empty() { self.dl_format.as_str() } else if self.dl_kind == Kind::Mp3 { "MP3" } else { "MP4" };
        let bar = |frac: f32, c: Color| {
            g.fill_rrect(Rect::new(44.0, 106.0, 340.0, 4.0), 2.0, white(0.12));
            if frac > 0.0 {
                g.fill_rrect(Rect::new(44.0, 106.0, 340.0 * frac, 4.0), 2.0, c);
            }
        };
        match &s.phase {
            Phase::Starting => {
                bar(0.0, VIOLET);
                g.text("Getting the video\u{2026}", 44.0, 120.0, 340.0, 10.5, 500, white(0.6), Align::Start);
            }
            Phase::Downloading => {
                bar(s.progress, VIOLET);
                g.text(&format!("Downloading  {:.0}%", s.progress * 100.0), 44.0, 120.0, 340.0, 10.5, 500, white(0.75), Align::Start);
            }
            Phase::Finishing => {
                bar(1.0, VIOLET);
                let msg = if self.dl_kind == Kind::Mp3 { "Making the MP3\u{2026}" } else { "Joining picture and sound\u{2026}" };
                g.text(msg, 44.0, 120.0, 340.0, 10.5, 500, white(0.75), Align::Start);
            }
            Phase::Done(_) => {
                bar(1.0, GREEN);
                g.text(&format!("Saved as {what} in Downloads\\Hilal"), 44.0, 120.0, 260.0, 10.5, 500, GREEN, Align::Start);
                let show = Rect::new(310.0, 112.0, 74.0, 16.0);
                let c = if self.hovered(show) { WHITE } else { white(0.7) };
                g.text("Show in folder", show.x, 120.0, show.w, 10.5, 600, c, Align::End);
                self.hit(show, Action::DlShow);
            }
            Phase::Failed(e) => {
                bar(0.0, RED);
                g.text(e, 44.0, 120.0, 340.0, 10.5, 500, rgb(0xff, 0x8a, 0x8a), Align::Start);
            }
        }
    }

    // ----- to-do ------------------------------------------------------------------------

    fn todo_page(&self) {
        let g = &self.gfx;
        let tasks = &self.store.tasks;
        self.header("Todo", Some(("List", GREEN)));
        let done = tasks.iter().filter(|t| t.done).count();
        g.display(&format!("{done}/{}", tasks.len()), 44.0, 48.0, 140.0, 28.0, 700, WHITE, Align::Start);
        g.text("Task Done Today", 44.0, 80.0, 150.0, 12.0, 400, white(0.55), Align::Start);

        let adding = self.form.as_ref().is_some_and(|f| f.editing.is_none());
        let w1 = self.pill_w(Ico::Plus, "Add Task");
        let pencil = Ico::Glyph('\u{E70F}');
        // The edit pill keeps its label when both fit beside the task list, else just its pencil.
        let edit_label = if 42.0 + w1 + 6.0 + self.pill_w(pencil, "Edit Task") <= 194.0 { "Edit Task" } else { "" };
        let w2 = if edit_label.is_empty() { 34.0 } else { self.pill_w(pencil, edit_label) };
        self.pill(Rect::new(42.0, 94.0, w1, 26.0), if adding { rgba(0x2f, 0xd0, 0x6b, 0.25) } else { white(0.12) }, Ico::Plus, "Add Task", WHITE, Some(Action::TodoAdd));
        let can_edit = !tasks.is_empty();
        let edit_fill = if self.edit_mode { white(0.25) } else { white(if can_edit { 0.12 } else { 0.06 }) };
        self.pill(
            Rect::new(42.0 + w1 + 6.0, 94.0, w2, 26.0),
            edit_fill,
            pencil,
            edit_label,
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
            g.display(&value, cx - 24.0, cy, 48.0, 12.5, 700, WHITE, Align::Center);
            g.text(label, cx - 30.0, 99.0, 60.0, 10.5, 600, white(0.85), Align::Center);
            g.text(&sub, cx - 30.0, 112.0, 60.0, 9.0, 400, white(0.5), Align::Center);
        }
        // The GPU's fan under its gauge: a little fan turning with it, and its speed.
        let fan = match (s.gpu_fan_rpm, s.gpu_fan_pct) {
            (Some(0), _) | (None, Some(0)) => Some(("fan off".to_owned(), 0.0)),
            (Some(rpm), _) => Some((format!("{rpm} rpm"), rpm as f32)),
            (None, Some(p)) => Some((format!("fan {p}%"), p as f32 * 30.0)),
            _ => None,
        };
        if let Some((label, rpm)) = fan {
            // One turn a second per 1000 RPM: the real speed would only strobe.
            let angle = self.fan_angle.get() + dt * rpm / 1000.0 * 360.0;
            self.fan_angle.set(angle % 360.0);
            let tw = g.measure(&label, 8.5, 500);
            let (fx, fy) = (134.0 - (tw + 11.0) / 2.0 + 4.0, 123.5);
            let c = if rpm > 0.0 { VIOLET } else { white(0.35) };
            for k in 0..3 {
                let a = (angle + k as f32 * 120.0).to_radians();
                g.line(fx, fy, fx + 3.6 * a.cos(), fy + 3.6 * a.sin(), 1.6, c);
            }
            g.fill_circle(fx, fy, 1.1, c);
            g.text(&label, fx + 7.0, fy, tw + 4.0, 8.5, 500, if rpm > 0.0 { white(0.6) } else { white(0.35) }, Align::Start);
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
        let fw = g.measure_display(&fps, 26.0, 700);
        g.display(&fps, x, 57.0, w, 26.0, 700, WHITE, Align::Start);
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

/// With HILAL_FPS=<file> set, notes an event in that file (time-stamped).
pub fn trace(msg: &str) {
    let Some(path) = std::env::var_os("HILAL_FPS") else { return };
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let t = win::seconds_of_day();
        let _ = writeln!(f, "  {:02}:{:02}:{:02} {msg}", t / 3600, t / 60 % 60, t % 60);
    }
}

/// Claude's spark: a burst of rounded rays, turned by `deg`.
fn spark(g: &Gfx, cx: f32, cy: f32, r: f32, c: Color, deg: f32) {
    for i in 0..8 {
        let a = (deg + i as f32 * 22.5).to_radians();
        let len = if i % 2 == 0 { r } else { r * 0.72 };
        let (s, co) = a.sin_cos();
        g.line(cx - co * len, cy - s * len, cx + co * len, cy + s * len, (r * 0.2).max(1.4), c);
    }
}

/// A session's state as a small icon: a turning arc while it works, a pulsing dot when it
/// needs you, a check when done, a pause mark when stopped.
fn status_icon(g: &Gfx, st: claude::State, cx: f32, cy: f32, t: f32) {
    match st {
        claude::State::Working => {
            g.stroke_circle(cx, cy, 5.5, 1.8, rgba(0xd9, 0x77, 0x57, 0.25));
            g.arc(cx, cy, 5.5, t * 320.0, 120.0, 2.0, CLAUDE);
        }
        claude::State::NeedsYou => {
            let p = 0.5 + 0.5 * (t * 4.0).sin();
            g.fill_circle(cx, cy, 6.0 + 2.0 * p, rgba(0xff, 0xc4, 0x3d, 0.25 * (1.0 - p)));
            g.fill_circle(cx, cy, 4.0, AMBER);
        }
        claude::State::Done => {
            g.fill_circle(cx, cy, 6.0, rgba(0x2f, 0xd0, 0x6b, 0.9));
            g.icon('\u{E73E}', cx, cy + 0.5, 7.0, BLACK);
        }
        claude::State::Stopped => {
            g.stroke_circle(cx, cy, 5.5, 1.4, white(0.4));
            g.fill_rrect(Rect::new(cx - 2.6, cy - 2.8, 1.8, 5.6), 0.6, white(0.6));
            g.fill_rrect(Rect::new(cx + 0.8, cy - 2.8, 1.8, 5.6), 0.6, white(0.6));
        }
    }
}

fn state_label(st: claude::State) -> (&'static str, Color) {
    match st {
        claude::State::Working => ("Working", CLAUDE),
        claude::State::NeedsYou => ("Waiting for you", AMBER),
        claude::State::Done => ("Done", GREEN),
        claude::State::Stopped => ("Stopped", white(0.6)),
    }
}

/// A crescent's outline: the disc of radius `r` at (`cx`, `cy`) less a disc of radius
/// `cut` shifted by `off` from it.
/// Each kind's color on its card: a verse gold, a dhikr teal, a du'a violet.
fn kind_color(kind: u8) -> Color {
    match kind {
        0 => rgb(0xff, 0xd4, 0x79),
        1 => rgb(0x5e, 0xea, 0xd4),
        _ => rgb(0xa7, 0x8b, 0xfa),
    }
}

/// A reminder's words as drawn: the Qur'an's between ornate brackets, with each verse
/// number as the verse-end ornament (the Qur'an face draws U+06DD and the number after it
/// as one), both in the kind's color; commas in the UI font (the Qur'an face draws them as
/// verse-end circles).
fn reminder_words(item: &crate::reminders_data::Reminder) -> (String, Vec<gfx::Span>) {
    let accent = kind_color(item.kind);
    let mut out = String::new();
    let mut spans = Vec::new();
    let mut push = |out: &mut String, s: &str, color: Option<Color>, ui_font: Option<f32>| {
        let start = out.encode_utf16().count() as u32;
        out.push_str(s);
        if let Some(color) = color {
            spans.push(gfx::Span { start, len: s.encode_utf16().count() as u32, color, ui_font });
        }
    };
    if item.quran {
        push(&mut out, "\u{FD3F}\u{2009}", Some(accent), None);
    }
    for (k, word) in item.text.split(' ').enumerate() {
        if k > 0 {
            push(&mut out, " ", None, None);
        }
        let number = item.quran && (1..=3).contains(&word.chars().count()) && word.chars().all(|c| ('\u{0660}'..='\u{0669}').contains(&c));
        if number {
            push(&mut out, &format!("\u{06DD}{word}"), Some(accent), None);
            continue;
        }
        let mut rest = word;
        while let Some(at) = rest.find('\u{060C}') {
            push(&mut out, &rest[..at], None, None);
            push(&mut out, "\u{060C}", Some(white(0.6)), Some(0.7));
            rest = &rest[at + '\u{060C}'.len_utf8()..];
        }
        push(&mut out, rest, None, None);
    }
    if item.quran {
        push(&mut out, "\u{2009}\u{FD3E}", Some(accent), None);
    }
    (out, spans)
}

fn crescent(cx: f32, cy: f32, r: f32, off: (f32, f32), cut: f32) -> Vec<(f32, f32)> {
    let inside_cut = |x: f32, y: f32| (x - cx - off.0).hypot(y - cy - off.1) < cut;
    // The middle of the lit part faces away from the cut.
    let mid = (-off.1).atan2(-off.0);
    let mut span = 0.0f32;
    while span < std::f32::consts::PI && !inside_cut(cx + r * (mid + span).cos(), cy + r * (mid + span).sin()) {
        span += 0.01;
    }
    // Where the outer edge meets the cut, seen from the cut's centre.
    let (ex, ey) = (cx + r * (mid + span).cos() - cx - off.0, cy + r * (mid + span).sin() - cy - off.1);
    let psi = (ey.atan2(ex) - mid).rem_euclid(std::f32::consts::TAU);
    let psi = if psi > std::f32::consts::PI { std::f32::consts::TAU - psi } else { psi };
    let n = 40;
    let mut pts = Vec::with_capacity(2 * n + 2);
    for k in 0..=n {
        let a = mid - span + 2.0 * span * k as f32 / n as f32;
        pts.push((cx + r * a.cos(), cy + r * a.sin()));
    }
    for k in 0..=n {
        let a = mid + psi - 2.0 * psi * k as f32 / n as f32;
        pts.push((cx + off.0 + cut * a.cos(), cy + off.1 + cut * a.sin()));
    }
    pts
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
    let mut weight = 0f64;
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for p in rgba.pixels() {
        let [r, g, b, a] = p.0;
        let k = a as f32 / 255.0;
        bgra.extend_from_slice(&[(b as f32 * k) as u8, (g as f32 * k) as u8, (r as f32 * k) as u8, a]);
        // Colorful, not-too-dark pixels count most, so a red cover gives red, not brown.
        let (mx, mn) = (r.max(g).max(b) as f64, r.min(g).min(b) as f64);
        let wgt = 0.02 + (mx - mn) / 255.0 * (mx / 255.0);
        sum[0] += r as f64 * wgt;
        sum[1] += g as f64 * wgt;
        sum[2] += b as f64 * wgt;
        weight += wgt;
    }
    let n = weight.max(1e-6) * 255.0;
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

/// Whether the window in front is maximized on the notch's screen (not one of ours, the
/// desktop or the taskbar).
fn foreground_is_maximized(notch: HWND, ours: &[HWND]) -> bool {
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || ours.contains(&fg) || !IsZoomed(fg).as_bool() {
            return false;
        }
        let mut class = [0u16; 32];
        let n = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
        MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST) == MonitorFromWindow(notch, MONITOR_DEFAULTTONEAREST)
    }
}

/// A free-standing shape's outline as a window region (see `notch_region`): its rounded
/// corners as drawn, square ones where it touches a screen edge (carried past the window's
/// side, where the region is cut anyway).
fn slab_region(b: Rect, radii: [f32; 4], k: f32) -> windows::Win32::Graphics::Gdi::HRGN {
    use windows::Win32::Graphics::Gdi::CreateRoundRectRgn;
    const PAD: f32 = 2.0;
    let r = radii.iter().cloned().fold(0.0, f32::max);
    let (mut l, t, mut rt, btm) = (b.x - PAD, b.y - PAD, b.x + b.w + PAD, b.y + b.h + PAD);
    if radii[0] == 0.0 && radii[3] == 0.0 {
        l -= r + PAD;
    }
    if radii[1] == 0.0 && radii[2] == 0.0 {
        rt += r + PAD;
    }
    let d = ((r + PAD) * 2.0 * k).round() as i32;
    unsafe { CreateRoundRectRgn((l * k).floor() as i32, (t * k).floor() as i32, (rt * k).ceil() as i32 + 1, (btm * k).ceil() as i32 + 1, d, d) }
}

/// The screen nearest a point: its bounds, and its work area (without the taskbar).
fn monitor_at(p: (i32, i32)) -> (windows::Win32::Foundation::RECT, windows::Win32::Foundation::RECT) {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let m = MonitorFromPoint(POINT { x: p.0, y: p.1 }, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(m, &mut info);
    }
    (info.rcMonitor, info.rcWork)
}

/// The notch's outline (ears, sides, rounded bottom) as a window region, in physical px.
/// Regions have no anti-aliasing: they cut on whole pixels. So this one runs `PAD` dp
/// outside the drawn shape, leaving its soft edge intact, and only the transparent
/// margin past it gets cut.
fn notch_region(s: Shape, cx: f32, k: f32) -> windows::Win32::Graphics::Gdi::HRGN {
    use std::f32::consts::{FRAC_PI_2, PI};
    use windows::Win32::Graphics::Gdi::{CreatePolygonRgn, WINDING};
    const PAD: f32 = 2.0;
    let rr0 = s.r.min(s.h / 2.0).min(s.w / 2.0).max(0.0);
    let e0 = s.e.min(s.h - rr0).max(0.0);
    // Offsetting the outline outward: sides and bottom move out, the bottom corners keep
    // their centers (radius grows), the concave ears keep theirs (radius shrinks).
    let (l, r, h) = (cx - s.w / 2.0 - PAD, cx + s.w / 2.0 + PAD, s.h + PAD);
    let rr = rr0 + PAD;
    let e = (e0 - PAD).max(0.0);
    let mut pts = Vec::new();
    let arc = |cx: f32, cy: f32, rad: f32, from: f32, to: f32| {
        (0..=10).map(move |i| {
            let a = from + (to - from) * i as f32 / 10.0;
            POINT { x: ((cx + rad * a.cos()) * k).round() as i32, y: ((cy + rad * a.sin()) * k).round() as i32 }
        })
    };
    let (el, er, ey) = (l + PAD - e0, r - PAD + e0, e0); // the ears' (unchanged) centers
    pts.push(POINT { x: ((el - PAD) * k).floor() as i32, y: 0 });
    pts.extend(arc(el, ey, e, -FRAC_PI_2, 0.0)); // left ear, from the top edge down to the side
    pts.extend(arc(l + rr, h - rr, rr, PI, FRAC_PI_2)); // bottom-left corner
    pts.extend(arc(r - rr, h - rr, rr, FRAC_PI_2, 0.0)); // bottom-right corner
    pts.extend(arc(er, ey, e, PI, PI + FRAC_PI_2)); // right ear, back up to the top edge
    pts.push(POINT { x: ((er + PAD) * k).ceil() as i32, y: 0 });
    unsafe { CreatePolygonRgn(&pts, WINDING) }
}
