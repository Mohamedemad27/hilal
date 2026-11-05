//! The prayer city's drop-down (Hilal's issue #2): a click on the city pill opens the list
//! of cities just below it, in Hilal's own glass, grouped (Egypt, then the world, then Auto
//! in Nova mode), the chosen one lit. The wheel moves through it (no scrollbar), a letter
//! jumps to the cities starting with it, a click picks one, and a click anywhere else or Esc
//! closes it.

use std::cell::RefCell;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{prayer, App, AMBER, EGYPT_CITIES};
use crate::gfx::{rgba, white, Align, Rect, WHITE};

pub const CP_W: f32 = 230.0;
pub const CP_H: f32 = 330.0;
const ROW: f32 = 26.0;
const PAD: f32 = 8.0;

/// One line of the list: a group's title, or a city (its choice index).
#[derive(Clone)]
enum Item {
    Group(&'static str),
    City(usize, String),
}

pub struct CityPicker {
    pub hwnd: HWND,
    surface: Option<usize>,
    pub shown: bool,
    /// The first line shown.
    top: usize,
    cursor: (f32, f32),
    regions: RefCell<Vec<(Rect, usize)>>,
    /// Drawn again until then after a change (a side window may drop a frame).
    redraw_until: Option<Instant>,
}

impl CityPicker {
    pub fn new() -> Self {
        Self { hwnd: HWND::default(), surface: None, shown: false, top: 0, cursor: (-1.0, -1.0), regions: RefCell::new(Vec::new()), redraw_until: None }
    }
}

impl App {
    /// The list's lines.
    fn city_items(&self) -> Vec<Item> {
        let mut out = vec![Item::Group("Egypt")];
        out.extend((0..EGYPT_CITIES).map(|i| Item::City(i, prayer::CITIES[i].0.to_owned())));
        if self.nova() {
            out.push(Item::Group("Around the world"));
            out.extend((EGYPT_CITIES..prayer::CITIES.len()).map(|i| Item::City(i, prayer::CITIES[i].0.to_owned())));
            out.push(Item::Group("Automatic"));
            out.push(Item::City(prayer::AUTO, "Auto (where you are)".to_owned()));
        }
        out
    }

    fn city_rows(&self) -> usize {
        ((CP_H - 2.0 * PAD) / ROW) as usize
    }

    /// Opens the list under the city pill (`pill`: its place in the notch, dp).
    pub(super) fn open_city_picker(&mut self, pill: Rect) {
        let k = self.gfx.scale;
        if self.city_picker.hwnd.is_invalid() {
            let Some(h) = crate::create_city_window() else { return };
            self.city_picker.hwnd = h;
        }
        // With the chosen city a few lines from the top.
        let items = self.city_items();
        let cur = self.store.prayer.city.min(if self.nova() { prayer::AUTO } else { EGYPT_CITIES - 1 });
        let at = items.iter().position(|i| matches!(i, Item::City(c, _) if *c == cur)).unwrap_or(0);
        let max_top = items.len().saturating_sub(self.city_rows());
        self.city_picker.top = at.saturating_sub(3).min(max_top);
        // Under the pill: the page's own place in the window (its content starts at the open
        // notch's left edge), just below the open notch so the notch never covers its top.
        let page = self.shape_rect(super::PAGE);
        let x = self.origin.x + ((page.x + pill.x - 4.0) * k) as i32;
        let y = self.origin.y + ((page.y + super::PAGE.h.max(pill.y + pill.h) + 6.0) * k) as i32;
        let (w, h) = ((CP_W * k).round() as i32, (CP_H * k).round() as i32);
        let hwnd = self.city_picker.hwnd;
        unsafe {
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        }
        match self.city_picker.surface {
            None => self.city_picker.surface = self.gfx.add_surface(hwnd, CP_W, CP_H, (x, y), false).ok(),
            Some(s) => self.gfx.move_surface(s, (x, y)),
        }
        // (Glass reads what's behind it, so it mustn't be in that picture itself.)
        // (Not while a screenshot is being taken: then it shows, with the notch.)
        crate::win::exclude_from_capture(hwnd, self.glass_live && self.capture_until.is_none());
        self.city_picker.shown = true;
        self.city_picker.redraw_until = Some(Instant::now() + std::time::Duration::from_millis(600));
        self.draw_city_picker();
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }

    pub fn close_city_picker(&mut self) {
        if self.city_picker.shown {
            self.city_picker.shown = false;
            unsafe {
                let _ = ShowWindow(self.city_picker.hwnd, SW_HIDE);
            }
        }
    }

    pub(super) fn tick_city_picker(&mut self) {
        let p = &self.city_picker;
        if p.shown && p.redraw_until.is_some_and(|t| Instant::now() < t) {
            self.draw_city_picker();
        }
    }

    pub fn city_hover(&mut self, x: f32, y: f32) {
        if self.city_picker.cursor != (x, y) {
            self.city_picker.cursor = (x, y);
            self.draw_city_picker();
        }
    }

    pub fn city_wheel(&mut self, delta: i32) {
        let rows = self.city_rows();
        let max_top = self.city_items().len().saturating_sub(rows);
        let steps = (delta.abs() / 40).max(1) as usize;
        let p = &mut self.city_picker;
        p.top = if delta < 0 { (p.top + steps).min(max_top) } else { p.top.saturating_sub(steps) };
        p.redraw_until = Some(Instant::now() + std::time::Duration::from_millis(300));
        self.draw_city_picker();
    }

    pub fn city_click(&mut self, x: f32, y: f32) {
        let hit = self.city_picker.regions.borrow().iter().find(|(r, _)| r.contains(x, y)).map(|(_, c)| *c);
        if let Some(c) = hit {
            self.store.prayer.city = c;
            self.store.save();
            super::trace(&format!("[prayer] city {}", prayer::place(c, self.store.prayer.auto.as_ref()).name));
            self.close_city_picker();
        }
    }

    /// A letter jumps to the first city starting with it; Esc closes.
    pub fn city_char(&mut self, c: char) {
        if c == '\u{1b}' {
            self.close_city_picker();
            return;
        }
        let want = c.to_lowercase().next().unwrap_or(c);
        let items = self.city_items();
        if let Some(at) = items.iter().position(|i| matches!(i, Item::City(_, n) if n.to_lowercase().starts_with(want))) {
            let max_top = items.len().saturating_sub(self.city_rows());
            self.city_picker.top = at.saturating_sub(1).min(max_top);
            self.city_picker.redraw_until = Some(Instant::now() + std::time::Duration::from_millis(300));
            self.draw_city_picker();
        }
    }

    pub fn draw_city_picker(&mut self) {
        let Some(surface) = self.city_picker.surface else { return };
        if !self.city_picker.shown || self.gfx.begin(surface).is_err() {
            return;
        }
        self.city_picker.regions.borrow_mut().clear();
        let items = self.city_items();
        let rows = self.city_rows();
        let p = &self.city_picker;
        let g = &self.gfx;
        let pane = Rect::new(1.0, 1.0, CP_W - 2.0, CP_H - 2.0);
        if self.glass_live {
            g.glass_rrect(pane, 16.0);
        } else {
            g.fill_rrect(pane, 16.0, rgba(0x12, 0x12, 0x18, 0.98));
        }
        g.stroke_rrect(pane, 16.0, 1.0, white(0.1), false);
        let cur = self.store.prayer.city;
        g.clip(Rect::new(1.0, PAD - 2.0, CP_W - 2.0, CP_H - 2.0 * PAD + 4.0));
        for (slot, item) in items.iter().skip(p.top).take(rows).enumerate() {
            let r = Rect::new(PAD, PAD + slot as f32 * ROW, CP_W - 2.0 * PAD, ROW - 2.0);
            match item {
                Item::Group(t) => {
                    g.text(t, r.x + 8.0, r.y + r.h / 2.0 + 3.0, r.w - 16.0, 10.0, 600, white(0.45), Align::Start);
                }
                Item::City(c, name) => {
                    let on = *c == cur;
                    let lit = r.contains(p.cursor.0, p.cursor.1);
                    if on {
                        g.fill_rrect(r, 8.0, rgba(0xff, 0xc4, 0x3d, 0.2));
                    } else if lit {
                        g.fill_rrect(r, 8.0, white(0.1));
                    }
                    if on {
                        g.icon('\u{E73E}', r.x + r.w - 14.0, r.y + r.h / 2.0, 10.0, AMBER);
                    }
                    g.text(name, r.x + 10.0, r.y + r.h / 2.0, r.w - 34.0, 12.5, if on { 700 } else { 500 }, if on { AMBER } else { WHITE }, Align::Start);
                    p.regions.borrow_mut().push((r, *c));
                }
            }
        }
        g.unclip();
        // (A soft fade at an edge says there's more that way.)
        if p.top > 0 {
            g.linear(Rect::new(1.0, 1.0, CP_W - 2.0, 18.0), 16.0, &[(0.0, rgba(0x12, 0x12, 0x18, 0.9)), (1.0, rgba(0x12, 0x12, 0x18, 0.0))], true);
        }
        if p.top + rows < items.len() {
            g.linear(Rect::new(1.0, CP_H - 19.0, CP_W - 2.0, 18.0), 16.0, &[(0.0, rgba(0x12, 0x12, 0x18, 0.0)), (1.0, rgba(0x12, 0x12, 0x18, 0.9))], true);
        }
        // A skipped frame (the screen wasn't ready) is drawn again next time round.
        if self.gfx.present().is_err() {
            self.city_picker.redraw_until = Some(Instant::now() + std::time::Duration::from_millis(100));
        }
    }
}
