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

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Water {
    pub enabled: bool,
    pub bottle_ml: u32,
    pub goal_ml: u32,
    pub from_h: u32,
    pub to_h: u32,
    pub day: String,
    pub drunk_ml: u32,
    /// Today's drinks as (hour, ml), for the chart and for undo.
    pub sips: Vec<(u32, u32)>,
    pub last_reminder: u64,
}

impl Default for Water {
    fn default() -> Self {
        Self {
            enabled: true,
            bottle_ml: 500,
            goal_ml: 2000,
            from_h: 8,
            to_h: 22,
            day: String::new(),
            drunk_ml: 0,
            sips: Vec::new(),
            last_reminder: now_secs(),
        }
    }
}

impl Water {
    /// Starts a new day's count when the date changes.
    pub fn roll_day(&mut self) -> bool {
        let today = win::today();
        if self.day == today {
            return false;
        }
        self.day = today;
        self.drunk_ml = 0;
        self.sips.clear();
        self.last_reminder = now_secs();
        true
    }

    pub fn drink(&mut self) {
        self.drunk_ml += self.bottle_ml;
        self.sips.push((win::hour() as u32, self.bottle_ml));
        self.last_reminder = now_secs();
    }

    pub fn reset(&mut self) {
        self.drunk_ml = 0;
        self.sips.clear();
        self.last_reminder = now_secs();
    }

    /// One reminder per bottle needed to reach the goal.
    pub fn reminders(&self) -> u32 {
        self.goal_ml.div_ceil(self.bottle_ml.max(1)).max(1)
    }

    fn span_h(&self) -> u32 {
        let span = (self.to_h + 24 - self.from_h) % 24;
        if span == 0 { 24 } else { span }
    }

    fn interval_secs(&self) -> u64 {
        (self.span_h() as u64 * 3600 / self.reminders() as u64).max(15 * 60)
    }

    pub fn in_hours(&self, hour: u32) -> bool {
        (hour + 24 - self.from_h) % 24 < self.span_h()
    }

    pub fn reminder_due(&self) -> bool {
        self.enabled
            && self.in_hours(win::hour() as u32)
            && self.drunk_ml < self.goal_ml
            && now_secs().saturating_sub(self.last_reminder) >= self.interval_secs()
    }

    /// Local "HH:MM" of the next reminder.
    pub fn next_reminder(&self) -> String {
        let due_in = self.interval_secs().saturating_sub(now_secs().saturating_sub(self.last_reminder));
        let now_min = win::hour() as u64 * 60 + win::minute() as u64;
        let at = (now_min + due_in / 60) % (24 * 60);
        format!("{:02}:{:02}", at / 60, at % 60)
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
}

impl Default for Settings {
    fn default() -> Self {
        Self { minimal: false, glass: true, overlay: false, overlay_pos: None }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Store {
    pub tasks: Vec<Task>,
    pub water: Water,
    pub shelf: Vec<PathBuf>,
    pub settings: Settings,
}

impl Store {
    fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
        base.join("Hilal").join("state.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
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
    fn water_hours_wrap_past_midnight() {
        let w = Water { from_h: 22, to_h: 2, ..Default::default() };
        assert!(w.in_hours(23) && w.in_hours(1) && !w.in_hours(2) && !w.in_hours(12));
        assert_eq!(Water { goal_ml: 2000, bottle_ml: 500, ..Default::default() }.reminders(), 4);
    }

    #[test]
    fn task_start_time() {
        let t = Task { time: "09:30-10:00".into(), ..Default::default() };
        assert_eq!(t.start_min(), Some(570));
        assert_eq!(Task::default().start_min(), None);
    }
}
