//! Prayer times for Egyptian cities, worked out on the device (no internet): the Egyptian
//! General Authority of Survey method (Fajr at 19.5°, Isha at 17.5° below the horizon,
//! Asr when a shadow equals its object plus its noon shadow), the sun's position from the
//! usual low-precision formulas. Times round to the nearest minute, except Dhuhr, which
//! rounds down (as Google and the Egyptian tables show it).

use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};

/// Name, latitude, longitude.
pub const CITIES: [(&str, f64, f64); 33] = [
    ("Cairo", 30.0444, 31.2357),
    ("Maadi", 29.9602, 31.2569),
    ("Nasr City", 30.0566, 31.3301),
    ("New Cairo", 30.0074, 31.4913),
    ("Helwan", 29.8414, 31.3008),
    ("Shubra El Kheima", 30.1286, 31.2422),
    ("6th of October", 29.9285, 30.9188),
    ("Giza", 30.0131, 31.2089),
    ("Alexandria", 31.2001, 29.9187),
    ("Port Said", 31.2653, 32.3019),
    ("Suez", 29.9668, 32.5498),
    ("Ismailia", 30.5965, 32.2715),
    ("Damietta", 31.4165, 31.8133),
    ("Mansoura", 31.0409, 31.3785),
    ("Tanta", 30.7865, 31.0004),
    ("Zagazig", 30.5877, 31.5020),
    ("Banha", 30.4660, 31.1858),
    ("Shibin El Kom", 30.5503, 31.0106),
    ("Kafr El Sheikh", 31.1107, 30.9388),
    ("Damanhur", 31.0341, 30.4682),
    ("Marsa Matruh", 31.3543, 27.2373),
    ("Arish", 31.1316, 33.7984),
    ("Faiyum", 29.3084, 30.8428),
    ("Beni Suef", 29.0661, 31.0994),
    ("Minya", 28.0871, 30.7618),
    ("Asyut", 27.1809, 31.1837),
    ("Sohag", 26.5591, 31.6957),
    ("Qena", 26.1551, 32.7160),
    ("Luxor", 25.6872, 32.6396),
    ("Aswan", 24.0889, 32.8998),
    ("Hurghada", 27.2579, 33.8116),
    ("Sharm El Sheikh", 27.9158, 34.3300),
    ("El Kharga", 25.4390, 30.5586),
];

pub const NAMES: [&str; 6] = ["Fajr", "Sunrise", "Dhuhr", "Asr", "Maghrib", "Isha"];

const FAJR_ANGLE: f64 = 19.5;
const ISHA_ANGLE: f64 = 17.5;
/// The sun's upper edge on the horizon, with refraction.
const RISE_ANGLE: f64 = 0.833;

fn sin(d: f64) -> f64 {
    d.to_radians().sin()
}
fn cos(d: f64) -> f64 {
    d.to_radians().cos()
}
fn tan(d: f64) -> f64 {
    d.to_radians().tan()
}
fn fix(a: f64, b: f64) -> f64 {
    a.rem_euclid(b)
}

fn julian(y: i32, m: u32, d: u32) -> f64 {
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let a = (y as f64 / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y as f64 + 4716.0)).floor() + (30.6001 * (m as f64 + 1.0)).floor() + d as f64 + b - 1524.5
}

/// The sun's declination (degrees) and the equation of time (hours) at Julian day `jd`.
fn sun(jd: f64) -> (f64, f64) {
    let d = jd - 2451545.0;
    let g = fix(357.529 + 0.98560028 * d, 360.0);
    let q = fix(280.459 + 0.98564736 * d, 360.0);
    let l = fix(q + 1.915 * sin(g) + 0.020 * sin(2.0 * g), 360.0);
    let e = 23.439 - 0.00000036 * d;
    let ra = fix((cos(e) * sin(l)).atan2(cos(l)).to_degrees() / 15.0, 24.0);
    let eqt = q / 15.0 - ra;
    let decl = (sin(e) * sin(l)).asin().to_degrees();
    (decl, eqt)
}

/// The six times for a day, in local hours (0..24, may run past for a late Isha), for a
/// place at `lat`/`lon` whose clocks run `tz` hours ahead of UTC.
pub fn times(y: i32, m: u32, d: u32, lat: f64, lon: f64, tz: f64) -> [f64; 6] {
    let jd = julian(y, m, d) - lon / (15.0 * 24.0);
    let noon = |t: f64| fix(12.0 - sun(jd + t / 24.0).1, 24.0);
    // The time (hours) the sun is `angle` below the horizon, before noon if `morning`.
    let at_angle = |angle: f64, t: f64, morning: bool| {
        let decl = sun(jd + t / 24.0).0;
        let c = ((-sin(angle) - sin(decl) * sin(lat)) / (cos(decl) * cos(lat))).clamp(-1.0, 1.0);
        let h = c.acos().to_degrees() / 15.0;
        noon(t) + if morning { -h } else { h }
    };
    let asr = |t: f64| {
        let decl = sun(jd + t / 24.0).0;
        let angle = -(1.0 / (1.0 + tan((lat - decl).abs()))).atan().to_degrees();
        at_angle(angle, t, false)
    };
    // Two passes: the sun's position at the rough time, then at the better one.
    let mut t = [5.0, 6.0, 12.0, 13.0, 18.0, 18.0];
    for _ in 0..2 {
        t = [
            at_angle(FAJR_ANGLE, t[0], true),
            at_angle(RISE_ANGLE, t[1], true),
            noon(t[2]),
            asr(t[3]),
            at_angle(RISE_ANGLE, t[4], false),
            at_angle(ISHA_ANGLE, t[5], false),
        ];
    }
    t.map(|h| h + tz - lon / 15.0)
}

/// The six times as minutes since midnight.
pub fn minutes(t: [f64; 6]) -> [i32; 6] {
    let mut m = t.map(|h| (h * 60.0).round() as i32);
    m[2] = (t[2] * 60.0).floor() as i32;
    m
}

/// Today's local date and the local clock's offset from UTC (hours, so summer time counts).
pub fn today() -> (i32, u32, u32, f64) {
    let (l, u) = unsafe { (GetLocalTime(), GetSystemTime()) };
    let mins = |t: &windows::Win32::Foundation::SYSTEMTIME| t.wHour as i32 * 60 + t.wMinute as i32;
    // Which way the dates differ tells how to read a wrap past midnight.
    let day = |t: &windows::Win32::Foundation::SYSTEMTIME| (t.wYear as i32, t.wMonth, t.wDay);
    let mut off = mins(&l) - mins(&u);
    if day(&l) > day(&u) {
        off += 24 * 60;
    } else if day(&l) < day(&u) {
        off -= 24 * 60;
    }
    (l.wYear as i32, l.wMonth as u32, l.wDay as u32, off as f64 / 60.0)
}

/// The prayer (index into `NAMES`, never Sunrise) whose time came within the last ten
/// minutes at the city, with today's date as yyyymmdd and its time in minutes.
pub fn started(city: usize) -> Option<(u32, usize, i32)> {
    let (_, lat, lon) = CITIES[city.min(CITIES.len() - 1)];
    let (y, m, d, tz) = today();
    let t = minutes(times(y, m, d, lat, lon, tz));
    let now = crate::win::seconds_of_day() as i32;
    // HILAL_PRAYER_TEST=<index> makes that prayer's alert show now (for testing).
    if let Some(i) = std::env::var("HILAL_PRAYER_TEST").ok().and_then(|v| v.parse::<usize>().ok()).filter(|&i| i < 6) {
        return Some((now as u32, i, t[i]));
    }
    [0, 2, 3, 4, 5]
        .into_iter()
        .rev()
        .find(|&i| now >= t[i] * 60 && now < t[i] * 60 + 600)
        .map(|i| (y as u32 * 10000 + m * 100 + d, i, t[i]))
}

/// The day after (y, m, d).
pub fn next_day(y: i32, m: u32, d: u32) -> (i32, u32, u32) {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let len = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m as usize - 1];
    match (d < len, m < 12) {
        (true, _) => (y, m, d + 1),
        (false, true) => (y, m + 1, 1),
        (false, false) => (y + 1, 1, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn city(name: &str) -> (f64, f64) {
        let (_, lat, lon) = CITIES.iter().find(|c| c.0 == name).unwrap();
        (*lat, *lon)
    }

    fn got(name: &str) -> Vec<String> {
        let (lat, lon) = city(name);
        minutes(times(2026, 11, 1, lat, lon, 2.0)).iter().map(|m| format!("{:02}:{:02}", m / 60, m % 60)).collect()
    }

    /// Maadi, 1 Nov 2026 (UTC+2), exactly as Google shows it for the Egyptian survey.
    #[test]
    fn matches_google_for_maadi() {
        assert_eq!(got("Maadi"), ["04:41", "06:09", "11:38", "14:45", "17:08", "18:27"]);
    }

    /// The same day against aladhan.com's Egyptian-survey times, within a minute (the two
    /// round differently).
    #[test]
    fn matches_published_times() {
        let want = [
            ("Cairo", ["04:41", "06:09", "11:39", "14:46", "17:08", "18:27"]),
            ("Alexandria", ["04:47", "06:16", "11:44", "14:49", "17:12", "18:31"]),
            ("Aswan", ["04:31", "05:55", "11:32", "14:46", "17:09", "18:24"]),
        ];
        let mins = |s: &str| s[..2].parse::<i32>().unwrap() * 60 + s[3..].parse::<i32>().unwrap();
        for (name, want) in want {
            let got = got(name);
            for (g, w) in got.iter().zip(want) {
                assert!((mins(g) - mins(w)).abs() <= 1, "{name}: got {got:?}, want {want:?}");
            }
        }
    }

    #[test]
    fn days_roll_over() {
        assert_eq!(next_day(2026, 2, 28), (2026, 3, 1));
        assert_eq!(next_day(2028, 2, 28), (2028, 2, 29));
        assert_eq!(next_day(2026, 12, 31), (2027, 1, 1));
        assert_eq!(next_day(2026, 11, 1), (2026, 11, 2));
    }
}

