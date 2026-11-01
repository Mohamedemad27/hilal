//! Everything the user creates, saved as one JSON file in %APPDATA%\Hilal.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::win;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Task {
    pub text: String,
    pub done: bool,
    /// "HH:MM-HH:MM", "HH:MM", or empty.
    pub time: String,
    /// Day ("YYYY-MM-DD") the due reminder last fired, so it fires once a day.
    pub alerted: String,
}

impl Task {
    /// Start time in minutes after midnight, when one was given.
    pub fn start_min(&self) -> Option<u32> {
        let (h, m) = self.time.get(..5)?.split_once(':')?;
        let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    }
}

/// Not set yet (older saves stored whole hours in `from_h`/`to_h`).
const UNSET: u32 = u32::MAX;
/// One reminder per quarter hour inside the reminder hours.
const SLOT_SECS: u32 = 15 * 60;
/// "Snooze" asks again this much later.
pub const SNOOZE_SECS: u64 = 10 * 60;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Water {
    pub enabled: bool,
    pub bottle_ml: u32,
    pub goal_ml: u32,
    /// Reminder hours, minutes after midnight (30-minute steps, at least an hour long).
    pub from_min: u32,
    pub to_min: u32,
    pub day: String,
    pub drunk_ml: u32,
    /// Today's drinks as (hour, ml), for the chart.
    pub sips: Vec<(u32, u32)>,
    /// What's left in the bottle; an empty bottle asks for a refill first.
    pub in_bottle_ml: u32,
    /// The last reminder slot today that was shown or skipped (-1: none yet).
    pub handled_slot: i32,
    /// When a snoozed reminder comes back (unix secs; 0 = not snoozed).
    pub snooze_until: u64,
    // Older saves.
    #[serde(skip_serializing)]
    pub from_h: u32,
    #[serde(skip_serializing)]
    pub to_h: u32,
}

impl Default for Water {
    fn default() -> Self {
        Self {
            enabled: true,
            bottle_ml: 500,
            goal_ml: 2000,
            from_min: UNSET,
            to_min: UNSET,
            day: String::new(),
            drunk_ml: 0,
            sips: Vec::new(),
            in_bottle_ml: 500,
            handled_slot: -1,
            snooze_until: 0,
            from_h: 8,
            to_h: 22,
        }
    }
}

/// What a due reminder asks for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ask {
    /// The bottle is (probably) empty.
    Refill,
    Drink,
}

impl Water {
    /// Fills in fields older saves didn't have.
    pub fn migrate(&mut self) {
        if self.from_min == UNSET || self.to_min == UNSET {
            self.from_min = self.from_h % 24 * 60;
            self.to_min = self.to_h % 24 * 60;
        }
        if self.window_mins() < 60 {
            self.to_min = (self.from_min + 60) % 1440;
        }
        self.bottle_ml = self.bottle_ml.clamp(250, 3000);
        self.in_bottle_ml = self.in_bottle_ml.min(self.bottle_ml);
    }

    /// Starts a new day's count when the date changes.
    pub fn roll_day(&mut self) -> bool {
        let today = win::today();
        if self.day == today {
            return false;
        }
        self.day = today;
        self.drunk_ml = 0;
        self.sips.clear();
        self.handled_slot = -1;
        self.snooze_until = 0;
        self.skip_past();
        true
    }

    /// Length of the reminder hours in minutes (a window may run past midnight).
    pub fn window_mins(&self) -> u32 {
        match (self.to_min + 1440 - self.from_min) % 1440 {
            0 => 1440,
            m => m,
        }
    }

    /// Reminders a day: one per quarter hour in the window.
    pub fn reminders(&self) -> u32 {
        (self.window_mins() * 60 / SLOT_SECS).max(1)
    }

    /// How much each reminder (and each "Drink") stands for, rounded to 10 ml.
    pub fn per_ml(&self) -> u32 {
        ((self.goal_ml / self.reminders() + 5) / 10 * 10).max(10)
    }

    /// Bottle refills needed to reach the goal.
    pub fn refills(&self) -> u32 {
        (self.goal_ml / self.bottle_ml.max(1)).saturating_sub(1)
    }

    /// Seconds since the window opened at `sod` (seconds of day), if it's open then.
    fn into_window_at(&self, sod: u32) -> Option<u32> {
        let into = (sod + 86_400 - self.from_min * 60) % 86_400;
        (into < self.window_mins() * 60).then_some(into)
    }

    /// The latest slot whose time has come by `sod` (reminders fall mid-slot: :07:30, :22:30, ...).
    fn slot_at(&self, sod: u32) -> Option<i32> {
        let into = self.into_window_at(sod)?;
        (into >= SLOT_SECS / 2).then(|| ((into - SLOT_SECS / 2) / SLOT_SECS) as i32)
    }

    fn current_slot(&self) -> Option<i32> {
        self.slot_at(win::seconds_of_day())
    }

    /// Slots already past (e.g. after changing the hours) don't fire.
    pub fn skip_past(&mut self) {
        self.handled_slot = self.current_slot().unwrap_or(-1);
    }

    fn quiet(&self) -> bool {
        !self.enabled || self.drunk_ml >= self.goal_ml
    }

    /// A reminder that should show now, and marks it shown. A snoozed one comes back
    /// once its snooze is over; otherwise each slot fires once.
    pub fn take_due(&mut self) -> Option<Ask> {
        if self.quiet() {
            return None;
        }
        if self.snooze_until > 0 {
            if now_secs() < self.snooze_until {
                return None;
            }
            self.snooze_until = 0;
            self.handled_slot = self.handled_slot.max(self.current_slot().unwrap_or(-1));
            return Some(self.ask());
        }
        let slot = self.current_slot()?;
        if slot <= self.handled_slot {
            return None;
        }
        self.handled_slot = slot;
        Some(self.ask())
    }

    /// Refill first when the bottle can't cover one reminder's worth.
    pub fn ask(&self) -> Ask {
        if self.in_bottle_ml < self.per_ml().min(self.bottle_ml) { Ask::Refill } else { Ask::Drink }
    }

    pub fn snooze(&mut self) {
        self.snooze_until = now_secs() + SNOOZE_SECS;
    }

    pub fn refill(&mut self) {
        self.in_bottle_ml = self.bottle_ml;
    }

    /// One reminder's worth, from the bottle.
    pub fn drink(&mut self) {
        let ml = self.per_ml();
        self.drunk_ml += ml;
        self.in_bottle_ml = self.in_bottle_ml.saturating_sub(ml);
        self.sips.push((win::hour() as u32, ml));
    }

    pub fn reset(&mut self) {
        self.drunk_ml = 0;
        self.sips.clear();
        self.in_bottle_ml = self.bottle_ml;
        self.snooze_until = 0;
        self.skip_past();
    }

    /// Moves the reminder hours by `mins`, keeping their length.
    pub fn shift_start(&mut self, mins: i32) {
        let len = self.window_mins();
        self.from_min = (self.from_min as i32 + 1440 + mins) as u32 % 1440;
        self.to_min = (self.from_min + len) % 1440;
        self.skip_past();
    }

    /// Moves the end of the reminder hours; the window stays at least an hour long.
    pub fn shift_end(&mut self, mins: i32) {
        let len = (self.window_mins() as i32 + mins).clamp(60, 1440) as u32;
        self.to_min = (self.from_min + len) % 1440;
        self.skip_past();
    }

    /// Local time of the next reminder, e.g. "5:22 PM" (None when off or done).
    pub fn next_reminder(&self) -> Option<String> {
        if self.quiet() {
            return None;
        }
        let at = if self.snooze_until > 0 {
            let left = self.snooze_until.saturating_sub(now_secs()) as u32;
            (win::seconds_of_day() + left) % 86_400
        } else {
            let next = (self.handled_slot.max(self.current_slot().unwrap_or(-1)) + 1) as u32;
            let next = if next < self.reminders() && self.into_window_at(win::seconds_of_day()).is_some() { next } else { 0 };
            (self.from_min * 60 + next * SLOT_SECS + SLOT_SECS / 2) % 86_400
        };
        Some(win::hm_text((at / 3600) as u16, (at / 60 % 60) as u16))
    }

    /// Hours to chart: the ones the reminder window touches.
    pub fn chart_hours(&self) -> Vec<u32> {
        let first = self.from_min / 60;
        let count = (self.from_min % 60 + self.window_mins()).div_ceil(60).clamp(1, 24);
        (0..count).map(|i| (first + i) % 24).collect()
    }

    pub fn ml_in_hour(&self, hour: u32) -> u32 {
        self.sips.iter().filter(|(h, _)| *h == hour).map(|(_, ml)| ml).sum()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Closed notch becomes a thin line, for laptops with a thick top bezel.
    pub minimal: bool,
    /// Liquid Glass (blurred desktop behind the notch) instead of solid black.
    pub glass: bool,
    /// The FPS overlay is pinned on screen, and where (top-left, physical px).
    pub overlay: bool,
    pub overlay_pos: Option<(i32, i32)>,
    /// Step the notch aside while a full-screen app is in front.
    pub hide_in_fullscreen: bool,
    /// Shrink the closed notch to a thin line while a maximized window (a browser full
    /// of tabs, say) is in front, so it doesn't cover what's at the top of it.
    pub shrink_over_maximized: bool,
    /// Or hide it altogether there (as in full screen).
    pub hide_over_maximized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { minimal: false, glass: true, overlay: false, overlay_pos: None, hide_in_fullscreen: true, shrink_over_maximized: true, hide_over_maximized: false }
    }
}

/// The prayer times page: which city (an index into `prayer::CITIES`), whether each
/// prayer time shows an alert, and the last one that did (yyyymmdd, prayer index).
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Prayer {
    pub city: usize,
    pub alerts: bool,
    pub alerted: (u32, usize),
}

impl Default for Prayer {
    fn default() -> Self {
        Self { city: 0, alerts: true, alerted: (0, 0) }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Store {
    pub tasks: Vec<Task>,
    pub water: Water,
    pub prayer: Prayer,
    pub shelf: Vec<PathBuf>,
    pub settings: Settings,
}

impl Store {
    fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
        base.join("Hilal").join("state.json")
    }

    /// Reads the saved state. A UTF-8 byte-order mark (what Notepad and PowerShell add) is
    /// skipped. If the file exists but can't be read, it is copied aside as
    /// state.unreadable-<time>.json before starting fresh, so nothing the user made is
    /// overwritten and lost.
    pub fn load() -> Self {
        let path = Self::path();
        let Ok(bytes) = std::fs::read(&path) else { return Self::default() };
        let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        match serde_json::from_slice::<Store>(json) {
            Ok(mut store) => {
                store.water.migrate();
                store
            }
            Err(_) => {
                let aside = path.with_file_name(format!("state.unreadable-{}.json", now_secs()));
                let _ = std::fs::copy(&path, aside);
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            // Write then rename, so a crash mid-save never leaves a half-written file.
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_state_with_a_byte_order_mark() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"tasks":[{"text":"keep me","done":false}]}"#);
        let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
        let store: Store = serde_json::from_slice(json).unwrap();
        assert_eq!(store.tasks[0].text, "keep me");
    }

    #[test]
    fn water_slots_follow_the_window() {
        let w = Water { from_min: 16 * 60 + 30, to_min: 17 * 60 + 30, goal_ml: 3000, bottle_ml: 250, ..Default::default() };
        assert_eq!((w.reminders(), w.per_ml(), w.refills()), (4, 750, 11));
        let at = |h: u32, m: u32, s: u32| h * 3600 + m * 60 + s;
        assert_eq!(w.slot_at(at(16, 37, 29)), None);
        assert_eq!(w.slot_at(at(16, 37, 30)), Some(0));
        assert_eq!(w.slot_at(at(17, 7, 30)), Some(2));
        assert_eq!(w.slot_at(at(17, 31, 0)), None);
        let long = Water { from_min: 16 * 60 + 30, to_min: 19 * 60, goal_ml: 3000, ..Default::default() };
        assert_eq!((long.reminders(), long.per_ml()), (10, 300));
    }

    #[test]
    fn water_window_runs_past_midnight_and_stays_an_hour() {
        let mut w = Water { from_min: 23 * 60, to_min: 60, ..Default::default() };
        assert_eq!(w.window_mins(), 120);
        assert!(w.slot_at(30 * 60).is_some());
        w.shift_end(-90);
        assert_eq!(w.window_mins(), 60);
        w.shift_start(30);
        assert_eq!((w.from_min, w.to_min), (23 * 60 + 30, 30));
    }

    #[test]
    fn empty_bottle_asks_for_a_refill_first() {
        let mut w = Water { bottle_ml: 250, goal_ml: 3000, from_min: 0, to_min: 60, in_bottle_ml: 0, ..Default::default() };
        assert_eq!(w.ask(), Ask::Refill);
        w.refill();
        assert_eq!(w.ask(), Ask::Drink);
        w.drink();
        assert_eq!((w.drunk_ml, w.in_bottle_ml), (750, 0));
    }

    #[test]
    fn task_start_time() {
        let t = Task { time: "09:30-10:00".into(), ..Default::default() };
        assert_eq!(t.start_min(), Some(570));
        assert_eq!(Task::default().start_min(), None);
    }
}
