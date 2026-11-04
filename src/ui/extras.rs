//! Nova mode's Quran Radio page, its updates from GitHub, and news from Hilal's repository.

use std::time::Instant;

use super::toasts::ToastAct;
use super::{Action, App, AMBER, GREEN};
use crate::gfx::{rgb, white, Align, Rect, WHITE};
use crate::radio::State;

impl App {
    pub(super) fn tick_extras(&mut self) {
        let nova = self.nova();
        // The radio: its stations once a run (when the page is first seen), and its stream.
        if let Some(rx) = &self.radio_loading {
            if let Ok(list) = rx.try_recv() {
                self.radio_loading = None;
                if !list.is_empty() {
                    self.store.radio_stations = list;
                    self.store.save();
                }
            }
        }
        if !nova && self.radio.want {
            self.radio.stop();
        }
        self.radio.tick();
        // Updates.
        self.update_feed.set_on(nova && self.store.features.auto_update);
        while let Ok(r) = self.update_feed.updates.try_recv() {
            self.toast('\u{E777}', rgb(0x9c, 0xc3, 0xff), format!("Update ready \u{00b7} {}", r.version), "Update now", None, 20_000);
            if let Some(t) = &mut self.toast {
                t.action = Some(ToastAct::Update(r.installer.clone()));
            }
        }
        if let Some((at, path)) = self.updating.clone() {
            let left = 10i64 - at.elapsed().as_secs() as i64;
            if left <= 0 {
                self.updating = None;
                if let Ok(mut p) = crate::updater::PENDING.lock() {
                    *p = None;
                }
                crate::updater::install(&path, true);
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
                }
                return;
            }
            self.toast('\u{E777}', rgb(0x9c, 0xc3, 0xff), "Updating \u{00b7} back in a few seconds", left.to_string(), None, 1500);
        }
        // News: a new item once, one a day at most.
        self.news_feed.set_on(nova && self.store.features.news);
        while let Ok(items) = self.news_feed.items.try_recv() {
            let today = crate::prayer::today_key();
            if self.store.news_day == today {
                continue;
            }
            if let Some(item) = items.into_iter().find(|i| !self.store.news_seen.contains(&i.id)) {
                self.store.news_seen.push(item.id.clone());
                self.store.news_day = today;
                self.store.save();
                self.toast('\u{E789}', AMBER, item.title, if item.link.is_some() { "Open" } else { "" }, None, 30_000);
                if let (Some(link), Some(t)) = (item.link, self.toast.as_mut()) {
                    t.action = Some(ToastAct::OpenUrl(link));
                }
            }
        }
    }

    /// "Update now": ten seconds' notice, then the installer, and Hilal back.
    pub(super) fn start_update(&mut self, installer: std::path::PathBuf) {
        self.updating = Some((Instant::now(), installer));
    }

    /// The station playing (or chosen), and where it is in the list.
    fn radio_station(&self) -> Option<(usize, &crate::radio::Station)> {
        let list = &self.store.radio_stations;
        let i = self.store.radio_station.as_ref().and_then(|u| list.iter().position(|s| &s.url == u)).unwrap_or(0);
        list.get(i).map(|s| (i, s))
    }

    pub(super) fn radio_toggle(&mut self) {
        if self.radio.want {
            self.radio.stop();
        } else if let Some((_, s)) = self.radio_station() {
            let url = s.url.clone();
            self.radio.play(&url);
        }
    }

    pub(super) fn radio_pick(&mut self, i: usize) {
        if let Some(s) = self.store.radio_stations.get(i) {
            let url = s.url.clone();
            self.store.radio_station = Some(url.clone());
            self.store.save();
            self.radio.play(&url);
        }
    }

    pub(super) fn radio_volume(&mut self, d: f64) {
        self.radio.set_volume(self.radio.volume + d);
        self.store.radio_volume = self.radio.volume;
    }

    pub(super) fn radio_live(&self) -> bool {
        self.radio.state() == State::Live
    }

    /// The Radio page: a play button with the station and what it's doing, the volume, and
    /// the stations (the official radio first) to pick from.
    pub(super) fn radio_page(&self) {
        let g = &self.gfx;
        self.header("Quran ", Some(("Radio", AMBER)));
        if self.store.radio_stations.is_empty() {
            let msg = if self.radio_loading.is_some() { "Getting the stations\u{2026}" } else { "No stations (no internet?)" };
            g.text(msg, 44.0, 78.0, 336.0, 12.0, 500, white(0.6), Align::Center);
            return;
        }
        let state = self.radio.state();
        let (_, st) = self.radio_station().expect("a station");
        // Play / stop.
        let (bx, by) = (68.0, 74.0);
        let b = Rect::centered(bx, by, 44.0, 44.0);
        let lit = self.hovered(b);
        g.fill_circle(bx, by, 22.0, if self.radio.want { AMBER } else { white(if lit { 0.22 } else { 0.14 }) });
        if self.radio.want {
            g.fill_rrect(Rect::centered(bx, by, 12.0, 12.0), 2.0, rgb(0x2a, 0x1c, 0x00));
        } else {
            g.fill_poly(&[(bx - 5.0, by - 8.0), (bx + 8.0, by), (bx - 5.0, by + 8.0)], WHITE);
        }
        self.hit(b, Action::RadioToggle);
        // What's on.
        let eyebrow = if state == State::Live { "\u{0628}\u{062b} \u{0645}\u{0628}\u{0627}\u{0634}\u{0631}" } else if st.official { "\u{0625}\u{0630}\u{0627}\u{0639}\u{0629} \u{0631}\u{0633}\u{0645}\u{064a}\u{0629}" } else { "\u{0627}\u{0644}\u{0642}\u{0631}\u{0651}\u{0627}\u{0621}" };
        let (ew, nw) = (g.measure(eyebrow, 10.0, 600) + 2.0, 104.0);
        g.text(eyebrow, 200.0 - ew, 47.0, ew, 10.0, 600, if state == State::Live { GREEN } else { white(0.5) }, Align::Start);
        let name_w = g.measure(&st.name, 13.5, 700).min(nw) + 2.0;
        g.text(&st.name, 200.0 - name_w, 65.0, name_w, 13.5, 700, WHITE, Align::Start);
        let status = match state {
            State::Off => "Stopped",
            State::Connecting => "Connecting\u{2026}",
            State::Retrying => "Reconnecting\u{2026}",
            State::Live => if st.official { "Official radio" } else { "Recitations, all day" },
        };
        g.text(status, 96.0, 83.0, 104.0, 10.5, 500, white(0.55), Align::End);
        // Volume.
        let vy = 106.0;
        for (label, x, d) in [("\u{2212}", 100.0, -0.1), ("+", 186.0, 0.1)] {
            let r = Rect::centered(x, vy, 18.0, 18.0);
            g.fill_circle(x, vy, 9.0, white(if self.hovered(r) { 0.2 } else { 0.1 }));
            g.text(label, r.x, vy, r.w, 12.0, 600, WHITE, Align::Center);
            self.hit(r, Action::RadioVolume(d as f32));
        }
        let bar = Rect::new(114.0, vy - 2.0, 58.0, 4.0);
        g.fill_rrect(bar, 2.0, white(0.15));
        g.fill_rrect(Rect::new(bar.x, bar.y, bar.w * self.radio.volume as f32, 4.0), 2.0, WHITE);
        // The stations.
        g.line(208.0, 34.0, 208.0, 120.0, 1.0, white(0.12));
        let cur = self.radio_station().map(|s| s.0);
        for (slot, (i, s)) in self.store.radio_stations.iter().enumerate().skip(self.radio_scroll).take(4).enumerate() {
            let r = Rect::new(216.0, 34.0 + slot as f32 * 22.0, 164.0, 20.0);
            let on = Some(i) == cur;
            g.fill_rrect(r, 7.0, if on { super::rgba(0xff, 0xc4, 0x3d, 0.2) } else { white(if self.hovered(r) { 0.12 } else { 0.06 }) });
            g.text(&s.name, r.x + 8.0, r.y + 10.0, r.w - 16.0, 10.5, 600, if on { AMBER } else { white(0.88) }, Align::End);
            self.hit(r, Action::RadioPick(i));
        }
    }

    /// Scrolling the station list.
    pub(super) fn radio_scroll_by(&mut self, down: bool) {
        let max = self.store.radio_stations.len().saturating_sub(4);
        self.radio_scroll = if down { (self.radio_scroll + 1).min(max) } else { self.radio_scroll.saturating_sub(1) };
    }

    /// The stations are read the first time the page shows.
    pub(super) fn radio_seen(&mut self) {
        if self.radio_loading.is_none() && !self.radio_fetched {
            self.radio_fetched = true;
            self.radio_loading = Some(crate::radio::fetch());
        }
    }
}
