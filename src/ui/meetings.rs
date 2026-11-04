//! Upcoming meetings (Nova mode, with an iCal link in Settings): a toast 10 minutes and 1
//! minute before, and at the start a toast whose Join opens the meeting link (with a chime);
//! and while one is under three hours away, a slide on the closed notch.

use std::time::Instant;

use super::toasts::ToastAct;
use super::App;
use crate::calendar::{self, Meeting};
use crate::gfx::rgb;

const CAL: crate::gfx::Color = rgb(0x7d, 0xd3, 0xfc);

impl App {
    /// Keeps the feed on the right link, takes its news, and gives the toasts their turn.
    pub(super) fn tick_meetings(&mut self) {
        let on = self.nova() && self.store.features.meetings;
        let url = if on { self.store.features.calendar_url.clone() } else { String::new() };
        let url = match std::env::var("HILAL_CALENDAR_TEST") {
            Ok(path) if on => format!("test:{path}"),
            _ => url,
        };
        if url != self.calendar_url {
            self.calendar_url = url.clone();
            self.calendar_feed.set_url(url);
        }
        while let Ok(s) = self.calendar_feed.updates.try_recv() {
            self.calendar = s;
            self.closed_sig.clear();
        }
        if !on || self.last_meeting_check.elapsed().as_secs() < 1 {
            return;
        }
        self.last_meeting_check = Instant::now();
        let now = crate::prayer::utc_now();
        let meetings = self.calendar.meetings.clone();
        for m in &meetings {
            let left = m.start - now;
            for min in [10i64, 1] {
                let id = format!("{}|{min}", m.id);
                if left <= min * 60 && left > (min - 1) * 60 && !self.meetings_fired.contains(&id) {
                    self.meetings_fired.push(id);
                    self.toast('\u{E787}', CAL, m.title.clone(), format!("in {min} min"), None, 5000);
                }
            }
            let id = format!("{}|start", m.id);
            if left <= 0 && left > -60 && !self.meetings_fired.contains(&id) {
                self.meetings_fired.push(id);
                self.toast('\u{E787}', CAL, m.title.clone(), if m.link.is_some() { "Join" } else { "Now" }, None, 12_000);
                self.happened(format!("Meeting started: {}", m.title));
                if let (Some(link), Some(t)) = (m.link.clone(), self.toast.as_mut()) {
                    t.action = Some(ToastAct::OpenUrl(link));
                }
                crate::sound::play_chime();
            }
        }
        if self.meetings_fired.len() > 200 {
            self.meetings_fired.drain(..100);
        }
    }

    /// The next meeting still to come or running now, if it's under three hours away (for
    /// the closed notch's slide).
    pub(super) fn meeting_soon(&self) -> Option<&Meeting> {
        if !self.nova() || !self.store.features.meetings {
            return None;
        }
        let now = crate::prayer::utc_now();
        self.calendar.meetings.iter().find(|m| m.end > now).filter(|m| m.start - now < 3 * 3600)
    }

    /// "in 25 min", "in 1h 10m" or "now".
    pub(super) fn meeting_when(m: &Meeting) -> String {
        let now = crate::prayer::utc_now();
        if m.start <= now {
            return "now".to_owned();
        }
        let min = ((m.start - now) as f64 / 60.0).round() as i64;
        if min >= 60 {
            if min % 60 == 0 { format!("in {}h", min / 60) } else { format!("in {}h {}m", min / 60, min % 60) }
        } else {
            format!("in {min} min")
        }
    }

    /// The Connections page's status line.
    pub(super) fn calendar_status(&self) -> String {
        if self.store.features.calendar_url.is_empty() {
            return "Not linked".to_owned();
        }
        let c = &self.calendar;
        if !c.error.is_empty() && c.updated == 0 {
            return format!("Couldn't read it ({})", c.error);
        }
        if c.updated == 0 {
            return "Reading\u{2026}".to_owned();
        }
        let n = c.meetings.len();
        let what = match n {
            0 => "No meetings in the next 24 hours".to_owned(),
            1 => "1 meeting in the next 24 hours".to_owned(),
            n => format!("{n} meetings in the next 24 hours"),
        };
        if c.error.is_empty() { what } else { format!("{what} (last read failed: {})", c.error) }
    }

    /// A link typed or pasted into Settings: kept if it's an https (or webcal) link.
    pub(super) fn set_calendar_url(&mut self, url: &str) -> bool {
        let clean = if url.trim().is_empty() { Some(String::new()) } else { calendar::normalize(url) };
        match clean {
            Some(u) => {
                self.store.features.calendar_url = u;
                self.store.save();
                true
            }
            None => false,
        }
    }
}
