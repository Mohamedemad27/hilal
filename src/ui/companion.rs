//! The companion (Nova mode, as Nova Island has it): it notices when you come back, greets
//! the day, and minds your breaks.
//!   - back after 5+ minutes away: "Welcome back", with what happened meanwhile (downloads,
//!     the adhan, meetings, the internet); only when something did, or after a long absence
//!   - the first time at the computer each day: a morning (or evening) card with the date and
//!     Hijri date, the weather, the next prayer and today's meetings
//!   - 50 minutes of use without a break: a gentle card (stand up, rest your eyes, breathe)
//! The cards come out like the dhikr card and wait for a calm moment. Also: a burst of
//! confetti for something done, the mascot's eyes on the closed notch when the mouse lingers
//! beside it, and scenes, where the mascot comes out with a card at the adhan and at the
//! start and end of a focus session.

use std::time::{Duration, Instant};

use super::{App, Shape, AMBER, REMINDER_MAX_H, REMINDER_W, REMINDER_W_SIDE};
use crate::gfx::{rgb, rgba, white, Align, Color, Rect, WHITE};

/// The reminder slot's mark for a companion card.
pub const CARD: usize = usize::MAX;
const AWAY: Duration = Duration::from_secs(5 * 60);
const LONG_AWAY: Duration = Duration::from_secs(30 * 60);
const WORK: Duration = Duration::from_secs(50 * 60);
const CARE: [(&str, &str); 3] = [
    ("Stand up and move", "50 minutes at the screen. Two minutes of moving wakes up body and mind."),
    ("Rest your eyes", "Look at something 20 feet away for 20 seconds: the 20-20-20 rule."),
    ("Take a deep breath", "In for 4, hold for 4, out for 4. Three times."),
];

#[derive(Clone)]
pub struct Card {
    pub badge: String,
    pub color: Color,
    pub title: String,
    /// The Qur'an face for the title (the adhan's verse).
    pub quran_title: bool,
    pub lines: Vec<String>,
    pub ms: u64,
    /// The mascot beside it (a scene).
    pub mascot: bool,
    /// Closing it stops the adhan.
    pub stops_sound: bool,
}

/// Something that happened, for "Welcome back": when (seconds since 1970), and what.
pub struct Happening {
    pub at: i64,
    pub text: String,
}

/// The confetti in the air: each piece's start, velocity and color.
pub struct Confetti {
    pub at: Instant,
    pub pieces: Vec<(f32, f32, f32, f32, Color)>,
}

/// Seconds since the last keyboard or mouse input.
fn idle() -> Duration {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut li).as_bool() {
            return Duration::ZERO;
        }
        Duration::from_millis(GetTickCount().wrapping_sub(li.dwTime) as u64)
    }
}

impl App {
    /// Notes something for the next "Welcome back".
    pub(super) fn happened(&mut self, text: impl Into<String>) {
        self.happenings.push(Happening { at: crate::prayer::utc_now(), text: text.into() });
        if self.happenings.len() > 50 {
            self.happenings.remove(0);
        }
    }

    pub(super) fn tick_companion(&mut self) {
        if !self.nova() {
            self.card_pending = None;
            return;
        }
        let f = &self.store.features;
        let (welcome_on, brief_on, care_on) = (f.welcome_back, f.morning_brief, f.wellbeing);
        let now = Instant::now();
        let idle = idle();
        // The first start of the day: a greeting once things have settled.
        if self.started_at.elapsed() > Duration::from_secs(9) && !self.brief_checked {
            self.brief_checked = true;
            if brief_on && self.brief_due() {
                self.brief();
            }
        }
        if idle >= AWAY {
            if self.away_from.is_none() {
                self.away_from = Some(now - idle);
            }
        } else if let Some(from) = self.away_from.filter(|_| idle < Duration::from_secs(5)) {
            // Back at the computer.
            let away = now - from;
            self.away_from = None;
            self.active_since = now;
            if brief_on && self.brief_due() {
                self.brief();
            } else if welcome_on {
                self.welcome_back(away);
            }
        } else if care_on && now - self.active_since >= WORK && !self.hidden_for_fullscreen && !(self.pomo.running() && !self.pomo.brk) {
            // Minding breaks: long use without one (never during focus or full screen).
            self.active_since = now;
            let (title, text) = CARE[self.care_next % CARE.len()];
            self.care_next += 1;
            self.queue_card(Card { badge: "Break".into(), color: rgb(0x86, 0xef, 0xac), title: title.into(), quran_title: false, lines: vec![text.into()], ms: 15_000, mascot: false, stops_sound: false });
        }
        // A card waits while the notch is open or busy with something else.
        if self.card_pending.is_some() && !self.open && self.reminder.is_none() && !self.toast_shown() && !self.peeking() && !self.hidden_for_fullscreen && !self.welcoming() {
            if let Some(c) = self.card_pending.take() {
                self.show_card(c);
            }
        }
    }

    fn brief_due(&self) -> bool {
        crate::win::hour() >= 4 && self.store.brief_day != crate::prayer::today_key()
    }

    /// The morning (or evening) card.
    fn brief(&mut self) {
        self.store.brief_day = crate::prayer::today_key();
        self.store.save();
        let h = crate::win::hour();
        let greeting = if h < 12 { "Good morning" } else if h < 17 { "Good afternoon" } else { "Good evening" };
        let mut lines = Vec::new();
        let place = self.prayer_place();
        let (y, m, d, now) = crate::prayer::local_now(&place);
        let hijri = crate::prayer::hijri(y, m, d, self.store.prayer.hijri_adjust).map(|h| h.0);
        lines.push(format!("{}{}", crate::win::date_long(), hijri.map_or(String::new(), |h| format!(" \u{00b7} {h}"))));
        if let Some(w) = &self.weather {
            lines.push(format!("{}\u{00b0} {} in {} \u{00b7} H {}\u{00b0} L {}\u{00b0}", w.temp, w.label(), if w.city.is_empty() { "your city" } else { &w.city }, w.hi, w.lo));
        }
        let t = crate::prayer::day_times(&place, y, m, d, self.prayer_calc());
        // (Past Isha, the next is tomorrow's Fajr.)
        let next = match [0usize, 2, 3, 4, 5].iter().find(|&&i| t[i] * 60 > now) {
            Some(&i) => (i, t[i]),
            None => {
                let (yy, mm, dd) = crate::prayer::next_day(y, m, d);
                (0, crate::prayer::day_times(&place, yy, mm, dd, self.prayer_calc())[0])
            }
        };
        lines.push(format!("Next prayer: {} {}", crate::prayer::NAMES[next.0], crate::win::hm_text((next.1 / 60) as u16, (next.1 % 60) as u16)));
        let today: Vec<&crate::calendar::Meeting> = self.calendar.meetings.iter().filter(|m| m.end > crate::prayer::utc_now() && m.start < crate::prayer::utc_now() + 18 * 3600).collect();
        if let Some(first) = today.first() {
            let local = first.start + (crate::prayer::offset_on("", y, m, d) * 3600.0) as i64;
            let hm = local.rem_euclid(86_400) / 60;
            lines.push(format!("{} today \u{00b7} first: {} {}", if today.len() == 1 { "1 meeting".to_owned() } else { format!("{} meetings", today.len()) }, first.title, crate::win::hm_text((hm / 60) as u16, (hm % 60) as u16)));
        }
        self.queue_card(Card { badge: greeting.into(), color: rgb(0xff, 0xc4, 0x5c), title: format!("{greeting} \u{1f44b}"), quran_title: false, lines, ms: 14_000, mascot: false, stops_sound: false });
    }

    fn welcome_back(&mut self, away: Duration) {
        let since = crate::prayer::utc_now() - away.as_secs() as i64;
        let news: Vec<String> = self.happenings.iter().filter(|h| h.at >= since).map(|h| h.text.clone()).collect();
        if news.is_empty() && away < LONG_AWAY {
            return;
        }
        let m = (away.as_secs_f32() / 60.0).round() as u64;
        let mut lines = vec![format!("Away for {}", if m < 60 { format!("{m} min") } else { format!("{} h{}", m / 60, if m % 60 > 0 { format!(" {} min", m % 60) } else { String::new() }) })];
        let n = news.len();
        lines.extend(news.into_iter().rev().take(4).collect::<Vec<_>>().into_iter().rev());
        if n > 4 {
            lines.push(format!("and {} more", n - 4));
        }
        if n == 0 {
            lines.push("You missed nothing. All quiet.".into());
        }
        self.queue_card(Card { badge: "Welcome back".into(), color: rgb(0x7d, 0xd3, 0xfc), title: "Welcome back \u{1f44b}".into(), quran_title: false, lines, ms: 12_000, mascot: false, stops_sound: false });
    }

    pub(super) fn queue_card(&mut self, c: Card) {
        self.card_pending = Some(c);
    }

    /// A card out now, in the dhikr card's place (hovering holds it; it's independent of the
    /// dhikr switch).
    pub(super) fn show_card(&mut self, c: Card) {
        let w = if self.pos().vertical { REMINDER_W_SIDE } else { REMINDER_W };
        let text_h = 21.0 * 1.6 + c.lines.len() as f32 * 15.0 * 1.6 + 2.0 * c.lines.len() as f32;
        let h = (text_h + 82.0).min(REMINDER_MAX_H).ceil();
        self.reminder_card = (Shape { w, h, r: 30.0, e: 20.0 }, 0.0);
        let now = Instant::now();
        self.reminder = Some(CARD);
        self.reminder_preview = true;
        self.reminder_at = now;
        self.reminder_total = Duration::from_millis(c.ms);
        self.reminder_until = now + self.reminder_total;
        super::trace(&format!("[card] {}", c.badge));
        self.companion = Some(c);
    }

    /// A companion card: its badge, a big title, and its lines; a scene has the mascot on the
    /// left.
    pub(super) fn card_view(&self) {
        let Some(c) = &self.companion else { return };
        let g = &self.gfx;
        let (card, _) = self.reminder_card;
        let (w, h) = (card.w, card.h);
        let t = self.reminder_at.elapsed().as_secs_f32();
        let breath = 0.5 + 0.5 * (t * std::f32::consts::TAU / 4.0).sin();
        g.radial(30.0, h, 220.0, 150.0, [c.color[0], c.color[1], c.color[2], 0.14 + 0.05 * breath]);
        let bw = g.measure(&c.badge, 12.0, 600) + 22.0;
        let badge = Rect::new(22.0, 13.0, bw, 22.0);
        g.fill_rrect(badge, 11.0, [c.color[0], c.color[1], c.color[2], 0.14]);
        g.text(&c.badge, badge.x, 24.0, badge.w, 12.0, 600, c.color, Align::Center);
        let close = Rect::centered(w - 34.0, 24.0, 24.0, 24.0);
        let lit = self.hovered(close);
        if lit {
            g.fill_circle(close.x + 12.0, 24.0, 12.0, white(0.14));
        }
        g.icon('\u{E711}', close.x + 12.0, 24.5, 9.0, white(if lit { 1.0 } else { 0.55 }));
        self.hit(close, super::Action::ReminderDismiss);
        let mut x = 22.0;
        if c.mascot {
            self.mascot(58.0, h / 2.0 + 8.0, t, c.color);
            x = 104.0;
        }
        let tw = w - x - 22.0;
        let mut y = 50.0;
        if c.quran_title {
            g.quran_text(&c.title, Rect::new(x, y - 6.0, tw, 40.0), 24.0, WHITE, &[]);
            y += 40.0;
        } else {
            g.text(&c.title, x, y + 8.0, tw, 19.0, 700, WHITE, Align::Start);
            y += 34.0;
        }
        for line in &c.lines {
            g.text(line, x, y + 6.0, tw, 12.5, 500, white(0.72), Align::Start);
            y += 25.0;
        }
        // The time left, as on the dhikr card.
        let left = self.reminder_until.saturating_duration_since(Instant::now()).as_secs_f32();
        let frac = (left / self.reminder_total.as_secs_f32().max(0.001)).clamp(0.0, 1.0);
        let track = Rect::new(30.0, h - 9.0, w - 60.0, 2.0);
        g.fill_rrect(track, 1.0, white(0.08));
        g.fill_rrect(Rect::new(track.x, track.y, track.w * frac, 2.0), 1.0, [c.color[0], c.color[1], c.color[2], if self.inside { 0.45 } else { 1.0 }]);
    }

    /// The mascot: two glowing eyes that blink now and then, in a soft halo.
    fn mascot(&self, cx: f32, cy: f32, t: f32, c: Color) {
        let g = &self.gfx;
        g.radial(cx, cy, 42.0, 42.0, [c[0], c[1], c[2], 0.25]);
        g.fill_circle(cx, cy, 30.0, rgba(0x14, 0x14, 0x1c, 0.9));
        g.stroke_circle(cx, cy, 30.0, 1.2, [c[0], c[1], c[2], 0.6]);
        let blink = if t % 3.6 < 0.16 { 0.15 } else { 1.0 };
        for dx in [-9.0, 9.0] {
            g.radial(cx + dx, cy - 2.0, 9.0, 9.0, rgba(80, 140, 255, 0.45));
            g.fill_ellipse(cx + dx, cy - 2.0, 3.6, 5.6 * blink, WHITE);
        }
    }

    /// Bursts confetti out of the notch (something done).
    pub(super) fn celebrate(&mut self, colors: &[Color]) {
        if !self.nova() || !self.store.features.celebrate {
            return;
        }
        let mut seed = crate::prayer::utc_now() as u32 | 1;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 10_000) as f32 / 10_000.0
        };
        let pieces = (0..36)
            .map(|i| {
                let a = std::f32::consts::PI * (0.1 + 0.8 * rnd());
                let v = 160.0 + 160.0 * rnd();
                (super::CX + (rnd() - 0.5) * 120.0, 20.0, a.cos() * v, a.sin() * v * 0.6, colors[i % colors.len()])
            })
            .collect();
        self.confetti = Some(Confetti { at: Instant::now(), pieces });
    }

    /// The confetti, falling and fading for 1.4 s (in the window's own space).
    pub(super) fn confetti_view(&self) {
        let Some(c) = &self.confetti else { return };
        let t = c.at.elapsed().as_secs_f32();
        if t > 1.4 {
            return;
        }
        let g = &self.gfx;
        g.fade.set((1.4 - t).min(0.5) * 2.0);
        for &(x, y, vx, vy, col) in &c.pieces {
            let px = x + vx * t;
            let py = y + vy * t + 260.0 * t * t;
            g.fill_rrect(Rect::centered(px, py, 4.0, 6.5), 1.0, col);
        }
        g.fade.set(1.0);
    }

    pub(super) fn confetti_up(&self) -> bool {
        self.confetti.as_ref().is_some_and(|c| c.at.elapsed().as_secs_f32() < 1.4)
    }

    /// The mascot's eyes on the closed notch, looking toward the cursor, when it has lingered
    /// beside the notch (not on it) for a few seconds.
    pub(super) fn eyes_shown(&self) -> bool {
        self.nova() && self.store.features.nova_eyes && !self.open && self.linger_since.is_some_and(|t| t.elapsed() > Duration::from_secs(3))
    }

    /// Keeps track of the cursor lingering near the notch.
    pub(super) fn follow_linger(&mut self, near: bool) {
        if near && !self.inside {
            self.linger_since.get_or_insert(Instant::now());
        } else {
            self.linger_since = None;
        }
    }

    pub(super) fn eyes_view(&self, s: Shape) {
        let g = &self.gfx;
        let b = self.shape_rect(s);
        let (cx, cy) = (b.x + b.w / 2.0, s.h / 2.0);
        let (dx, dy) = (self.cursor.0 - cx, self.cursor.1 - (b.y + cy));
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let (lx, ly) = (dx / len * 2.2, dy / len * 1.6);
        let t = self.t0.elapsed().as_secs_f32();
        let blink = if t % 4.2 < 0.14 { 0.15 } else { 1.0 };
        for ex in [-11.0, 11.0] {
            g.radial(cx + ex, cy, 10.0, 10.0, rgba(80, 140, 255, 0.4));
            g.fill_ellipse(cx + ex + lx, cy + ly, 3.6, 5.4 * blink, WHITE);
        }
    }

    /// The adhan as a scene: the mascot, the verse and the prayer (Nova mode).
    pub(super) fn adhan_scene(&mut self, i: usize, at: i32) {
        let time = crate::win::hm_text((at / 60 % 24) as u16, (at % 60) as u16);
        let (y, m, d, _) = crate::prayer::local_now(&self.prayer_place());
        let iftar = if i == 4 && self.ramadan(y, m, d) { " \u{00b7} \u{062a}\u{0642}\u{0628}\u{0651}\u{0644} \u{0627}\u{0644}\u{0644}\u{0647} \u{0635}\u{064a}\u{0627}\u{0645}\u{0643}" } else { "" };
        self.happened(format!("Adhan: {} {}", crate::prayer::NAMES[i], time));
        self.show_card(Card {
            badge: "\u{263e} Adhan".into(),
            color: AMBER,
            title: "\u{0623}\u{064e}\u{0642}\u{0650}\u{0645}\u{0652} \u{0635}\u{064e}\u{0644}\u{064e}\u{0627}\u{062a}\u{064e}\u{0643}\u{064e}".into(),
            quran_title: true,
            lines: vec![format!("It's time for {} \u{00b7} {time}{iftar}", crate::prayer::NAMES[i])],
            ms: 45_000,
            mascot: true,
            stops_sound: true,
        });
    }

    /// A focus session's start or end as a scene.
    pub(super) fn focus_scene(&mut self, starting: bool) {
        if !self.nova() || !self.store.features.nova_focus {
            return;
        }
        let red = rgb(0xff, 0x6b, 0x6b);
        let (title, line) = if starting { ("Focus time", "Phone away, one thing at a time. You've got this.") } else { ("Focus done", "Nice work. Take your break: stand up, have some water.") };
        if !starting {
            self.celebrate(&[red, rgb(0xff, 0xd4, 0x79), WHITE]);
        }
        self.show_card(Card { badge: "Focus".into(), color: red, title: title.into(), quran_title: false, lines: vec![line.into()], ms: 6000, mascot: true, stops_sound: false });
    }
}
