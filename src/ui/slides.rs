//! Nova mode's closed notch: a little wider, it turns every six seconds between the clock
//! with the weather, the CPU and memory load, and the next prayer (in Ramadan, iftar or
//! imsak). A new song shows its cover and title for a moment. The weather also sits in the
//! pages' header.

use std::time::{Duration, Instant};

use super::{prayer, App, Shape, AMBER, PEEK};
use crate::gfx::{rgb, white, Align, Color, Rect, WHITE};
use crate::weather::Sky;
use crate::win;

/// Nova's closed notch.
pub const N_IDLE: Shape = Shape { w: 236.0, h: 32.0, r: 16.0, e: 11.0 };
/// How long each slide shows, and how long a new song's card does.
const SLIDE: Duration = Duration::from_secs(6);
pub const TRACK_PEEK: Duration = Duration::from_millis(3200);

impl App {
    /// The slides that are on (all off: the clock stays).
    fn slides(&self) -> Vec<u8> {
        let f = &self.store.features;
        let mut s = Vec::new();
        if f.slide_clock {
            s.push(0);
        }
        if f.slide_stats {
            s.push(1);
        }
        if self.meeting_soon().is_some() {
            s.push(3);
        }
        if f.slide_prayer {
            s.push(2);
        }
        if s.is_empty() {
            s.push(0);
        }
        s
    }

    /// Which slide shows now (0 clock, 1 CPU and memory, 2 prayer).
    pub(super) fn slide_now(&self) -> u8 {
        let slides = self.slides();
        slides[(self.slides_at.elapsed().as_secs_f32() / SLIDE.as_secs_f32()) as usize % slides.len()]
    }

    /// Nova mode's closed notch is the wider one (unless it's the thin line).
    pub(super) fn nova_idle(&self) -> bool {
        self.nova() && !self.minimal() && !self.pos().vertical
    }

    /// The closed notch's slides: which shows, and how far into its fade.
    pub(super) fn closed_slides(&self, s: Shape) {
        let g = &self.gfx;
        let b = self.shape_rect(s);
        let slides = self.slides();
        let t = self.slides_at.elapsed().as_secs_f32();
        let n = (t / SLIDE.as_secs_f32()) as usize;
        let into = t % SLIDE.as_secs_f32();
        let cur = slides[n % slides.len()];
        // Each fades in over the first third of a second.
        let a = (into / 0.3).min(1.0);
        let fade = g.fade.get();
        g.fade.set(fade * a);
        let cy = s.h / 2.0;
        let (l, r) = (b.x + 16.0, b.x + s.w - 16.0);
        match cur {
            0 => {
                let w = self.time_width(13.5, 700);
                self.time(l, cy, 13.5, 700, WHITE);
                let day = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][win::weekday() as usize % 7];
                g.text(day, l + w + 6.0, cy, 40.0, 11.0, 500, white(0.5), Align::Start);
                if let Some(wx) = &self.weather {
                    let temp = format!("{}\u{00b0}", wx.temp);
                    let tw = g.measure_display(&temp, 13.0, 700);
                    g.display(&temp, r - tw, cy, tw + 2.0, 13.0, 700, WHITE, Align::Start);
                    weather_icon(g, wx.sky(), r - tw - 12.0, cy, 8.0);
                }
            }
            1 => {
                let st = &self.stats_now;
                let ram = if st.ram_total > 0 { st.ram_used as f32 / st.ram_total as f32 } else { 0.0 };
                let half = (s.w - 32.0) / 2.0;
                for (k, (label, v)) in [("CPU", st.cpu), ("RAM", ram)].into_iter().enumerate() {
                    let x = l + k as f32 * (half + 8.0);
                    g.text(label, x, cy, 30.0, 10.5, 500, white(0.5), Align::Start);
                    let bar = Rect::new(x + 30.0, cy - 2.0, half - 30.0 - 38.0, 4.0);
                    g.fill_rrect(bar, 2.0, white(0.15));
                    let c = if v >= 0.9 { rgb(0xff, 0x45, 0x3a) } else if v >= 0.7 { rgb(0xff, 0xc4, 0x3d) } else { WHITE };
                    g.fill_rrect(Rect::new(bar.x, bar.y, bar.w * v.clamp(0.0, 1.0), 4.0), 2.0, c);
                    g.display(&format!("{}%", (v * 100.0).round()), bar.x + bar.w + 4.0, cy, 34.0, 12.0, 700, WHITE, Align::Start);
                }
            }
            3 => {
                if let Some(m) = self.meeting_soon() {
                    let when = Self::meeting_when(m);
                    let ww = g.measure(&when, 12.0, 600) + 2.0;
                    g.icon('\u{E787}', l + 6.0, cy, 12.0, rgb(0x7d, 0xd3, 0xfc));
                    g.text(&m.title, l + 20.0, cy, r - ww - 10.0 - (l + 20.0), 12.5, 700, WHITE, Align::Start);
                    g.text(&when, r - ww, cy, ww, 12.0, 600, rgb(0x7d, 0xd3, 0xfc), Align::Start);
                }
            }
            _ => {
                // The next prayer (in Ramadan, iftar or imsak), and how long until it.
                let place = self.prayer_place();
                let calc = self.prayer_calc();
                let (y, m, d, now) = prayer::local_now(&place);
                let today = prayer::day_times(&place, y, m, d, calc);
                let (yy, mm, dd) = prayer::next_day(y, m, d);
                let tomorrow = || prayer::day_times(&place, yy, mm, dd, calc);
                let (name, at): (String, i32) = if self.ramadan(y, m, d) {
                    let (imsak, iftar) = ((today[0] - 10) * 60, today[4] * 60);
                    if now >= imsak && now < iftar {
                        ("Iftar".to_owned(), iftar)
                    } else if now < imsak {
                        ("Imsak".to_owned(), imsak)
                    } else {
                        ("Imsak".to_owned(), 86_400 + (tomorrow()[0] - 10) * 60)
                    }
                } else {
                    match [0usize, 2, 3, 4, 5].iter().find(|&&i| today[i] * 60 > now) {
                        Some(&i) => (prayer::NAMES[i].to_owned(), today[i] * 60),
                        None => (prayer::NAMES[0].to_owned(), 86_400 + tomorrow()[0] * 60),
                    }
                };
                let hm = at / 60 % 1440;
                g.fill_poly(&super::crescent(l + 6.0, cy, 6.0, (2.8, -1.9), 5.0), AMBER);
                let nw = g.text(&name, l + 18.0, cy, 90.0, 13.0, 700, WHITE, Align::Start);
                g.display(&win::hm_text((hm / 60) as u16, (hm % 60) as u16), l + 18.0 + nw + 6.0, cy, 70.0, 11.0, 500, white(0.5), Align::Start);
                let left = (at - now).max(0) / 60;
                let left = if left >= 60 { format!("{}h {:02}m", left / 60, left % 60) } else { format!("{left}m") };
                let lw = g.measure_display(&left, 12.5, 700);
                g.display(&left, r - lw, cy, lw + 2.0, 12.5, 700, AMBER, Align::Start);
                g.text("in", r - lw - 18.0, cy, 16.0, 10.5, 500, white(0.5), Align::Start);
                // With Quran Radio on, little waves beside the countdown.
                if self.radio_live() {
                    let x = r - lw - 34.0;
                    for (k, h) in [5.0f32, 9.0, 6.0].iter().enumerate() {
                        g.fill_rrect(Rect::new(x + k as f32 * 3.6, cy - h / 2.0, 2.2, *h), 1.1, rgb(0x30, 0xd1, 0x58));
                    }
                }
            }
        }
        g.fade.set(fade);
    }

    /// A new song's card: the cover, the title and the artist.
    pub(super) fn track_peek_view(&self) {
        let g = &self.gfx;
        let Some(t) = &self.track else { return };
        let art = Rect::new(12.0, 10.0, 40.0, 40.0);
        match self.art() {
            Some((img, _)) => g.draw_image(img, art, 9.0, 1.0),
            None => {
                g.fill_rrect(art, 9.0, white(0.15));
                g.icon('\u{E8D6}', art.x + 20.0, art.y + 20.0, 15.0, white(0.7));
            }
        }
        let w = PEEK.w - 64.0 - 14.0;
        g.text("New track", 64.0, 17.0, w, 9.5, 600, white(0.5), Align::Start);
        g.text(&t.title, 64.0, 32.0, w, 13.0, 700, WHITE, Align::Start);
        if !t.artist.is_empty() {
            g.text(&t.artist, 64.0, 47.0, w, 10.5, 500, white(0.6), Align::Start);
        }
    }

    /// A new song showing its card (Nova mode, while the notch is closed).
    pub(super) fn track_peeking(&self) -> bool {
        !self.open && self.track_peek_until.is_some_and(|t| Instant::now() < t)
    }

    /// Notes a new song, for its card (not the first one seen).
    pub(super) fn follow_track(&mut self) {
        let key = self.track.as_ref().map(|t| format!("{}|{}", t.title, t.artist));
        if key != self.last_track {
            if key.is_some() && self.last_track.is_some() && self.nova() && self.store.features.track_peek && !self.open {
                self.track_peek_until = Some(Instant::now() + TRACK_PEEK);
            }
            self.last_track = key;
        }
        // The weather, as it comes.
        self.weather_feed.set_on(self.nova());
        while let Ok(w) = self.weather_feed.updates.try_recv() {
            self.store.weather = Some((w.clone(), crate::store::unix_now()));
            self.store.save();
            self.weather = Some(w);
            self.closed_sig.clear();
        }
    }

    /// The weather in a page's header, at its right end (Nova mode).
    pub(super) fn header_weather(&self) {
        let Some(w) = self.weather.as_ref().filter(|_| self.nova()) else { return };
        let g = &self.gfx;
        let temp = format!("{}\u{00b0}", w.temp);
        let tw = g.measure_display(&temp, 12.5, 600);
        let x = 396.0 - tw;
        g.display(&temp, x, 19.0, tw + 2.0, 12.5, 600, white(0.9), Align::Start);
        weather_icon(g, w.sky(), x - 11.0, 19.0, 7.0);
    }
}

/// A little weather picture (`r` is about its radius).
pub fn weather_icon(g: &crate::gfx::Gfx, sky: Sky, cx: f32, cy: f32, r: f32) {
    let sun = rgb(0xff, 0xd0, 0x4a);
    let cloud_c: Color = white(0.9);
    let cloud = |x: f32, y: f32, k: f32, c: Color| {
        g.fill_circle(x - 0.35 * k, y + 0.1 * k, 0.45 * k, c);
        g.fill_circle(x + 0.15 * k, y - 0.15 * k, 0.6 * k, c);
        g.fill_circle(x + 0.6 * k, y + 0.2 * k, 0.38 * k, c);
        g.fill_rrect(Rect::new(x - 0.75 * k, y + 0.1 * k, 1.7 * k, 0.5 * k), 0.25 * k, c);
    };
    match sky {
        Sky::Sun => {
            g.fill_circle(cx, cy, r * 0.5, sun);
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                g.line(cx + a.cos() * r * 0.75, cy + a.sin() * r * 0.75, cx + a.cos() * r, cy + a.sin() * r, 1.4, sun);
            }
        }
        Sky::Moon => g.fill_poly(&super::crescent(cx, cy, r * 0.8, (r * 0.37, -r * 0.26), r * 0.68), rgb(0xe8, 0xe4, 0xff)),
        Sky::Partly => {
            g.fill_circle(cx + r * 0.3, cy - r * 0.3, r * 0.45, sun);
            cloud(cx - r * 0.1, cy + r * 0.1, r, cloud_c);
        }
        Sky::Cloud => cloud(cx, cy, r, cloud_c),
        Sky::Fog => {
            cloud(cx, cy - r * 0.25, r, white(0.7));
            g.line(cx - r * 0.8, cy + r * 0.7, cx + r * 0.6, cy + r * 0.7, 1.3, white(0.7));
            g.line(cx - r * 0.5, cy + r * 1.0, cx + r * 0.8, cy + r * 1.0, 1.3, white(0.7));
        }
        Sky::Rain => {
            cloud(cx, cy - r * 0.3, r, cloud_c);
            for k in [-0.5f32, 0.0, 0.5] {
                g.line(cx + k * r, cy + r * 0.55, cx + k * r - r * 0.15, cy + r * 1.0, 1.3, rgb(0x6c, 0xb8, 0xff));
            }
        }
        Sky::Snow => {
            cloud(cx, cy - r * 0.3, r, cloud_c);
            for k in [-0.5f32, 0.0, 0.5] {
                g.fill_circle(cx + k * r, cy + r * 0.8, 1.2, WHITE);
            }
        }
        Sky::Storm => {
            cloud(cx, cy - r * 0.3, r, white(0.75));
            g.fill_poly(&[(cx + 0.1 * r, cy + 0.3 * r), (cx - 0.3 * r, cy + 0.85 * r), (cx, cy + 0.85 * r), (cx - 0.2 * r, cy + 1.3 * r), (cx + 0.35 * r, cy + 0.7 * r), (cx + 0.05 * r, cy + 0.7 * r)], sun);
        }
    }
}
