//! The weather, as Nova Island gets it: Open-Meteo for where the PC is (found from its
//! internet address, the same way as the prayer times' Auto), refreshed every 20 minutes;
//! after a failure it tries again after 2, 5, then every 15 minutes. In the background, and
//! only while it's switched on (Nova mode).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Weather {
    pub city: String,
    pub temp: i32,
    pub code: u32,
    pub is_day: bool,
    pub hi: i32,
    pub lo: i32,
}

/// What kind of sky a WMO weather code is (Nova's grouping).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sky {
    Sun,
    Moon,
    Partly,
    Cloud,
    Fog,
    Rain,
    Snow,
    Storm,
}

impl Weather {
    pub fn sky(&self) -> Sky {
        match self.code {
            0 => if self.is_day { Sky::Sun } else { Sky::Moon },
            1 | 2 => if self.is_day { Sky::Partly } else { Sky::Cloud },
            3 => Sky::Cloud,
            45 | 48 => Sky::Fog,
            71..=77 | 85 | 86 => Sky::Snow,
            95.. => Sky::Storm,
            _ => Sky::Rain,
        }
    }

    pub fn label(&self) -> &'static str {
        match self.sky() {
            Sky::Sun | Sky::Moon => "Clear",
            Sky::Partly => "Partly Cloudy",
            Sky::Cloud => "Cloudy",
            Sky::Fog => "Fog",
            Sky::Rain => "Rain",
            Sky::Snow => "Snow",
            Sky::Storm => "Thunderstorm",
        }
    }
}

pub struct Feed {
    pub updates: Receiver<Weather>,
    on: Arc<AtomicBool>,
}

impl Feed {
    /// Starts the feed; `place` is a known place to use (lat, lon, city) instead of looking
    /// the PC up.
    pub fn start(on: bool) -> Self {
        let (tx, rx) = channel();
        let flag = Arc::new(AtomicBool::new(on));
        let f = flag.clone();
        std::thread::spawn(move || {
            let mut place: Option<(f64, f64, String, u32)> = None;
            let mut next = Instant::now();
            let mut fails = 0usize;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if !f.load(Ordering::Relaxed) || Instant::now() < next {
                    continue;
                }
                // Where the PC is, once a day.
                let today = crate::prayer::today_key();
                if place.as_ref().map_or(true, |p| p.3 != today) {
                    if let Some(a) = crate::prayer::detect() {
                        place = Some((a.lat, a.lon, a.city, today));
                    }
                }
                let got = place.as_ref().and_then(|(lat, lon, city, _)| fetch(*lat, *lon, city));
                match got {
                    Some(w) => {
                        fails = 0;
                        let _ = tx.send(w);
                        next = Instant::now() + Duration::from_secs(20 * 60);
                    }
                    None => {
                        const RETRY_MIN: [u64; 3] = [2, 5, 15];
                        next = Instant::now() + Duration::from_secs(RETRY_MIN[fails.min(2)] * 60);
                        fails += 1;
                    }
                }
            }
        });
        Self { updates: rx, on: flag }
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }
}

fn fetch(lat: f64, lon: f64, city: &str) -> Option<Weather> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1"
    );
    let j: serde_json::Value = ureq::get(&url).timeout(Duration::from_secs(10)).call().ok()?.into_json().ok()?;
    let c = &j["current"];
    Some(Weather {
        city: city.to_owned(),
        temp: c["temperature_2m"].as_f64()?.round() as i32,
        code: c["weather_code"].as_u64()? as u32,
        is_day: c["is_day"].as_u64() == Some(1),
        hi: j["daily"]["temperature_2m_max"][0].as_f64()?.round() as i32,
        lo: j["daily"]["temperature_2m_min"][0].as_f64()?.round() as i32,
    })
}
