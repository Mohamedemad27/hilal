//! Prayer times worked out on the device, the way Nova Island does it outside Egypt: the
//! PrayTimes formulas with the method each country follows (Umm al-Qura in Saudi Arabia,
//! Dubai's, Diyanet's, ...), one minute of precaution on Asr, Fajr and Isha kept within a
//! share of the night at high latitudes, and the times shown in the place's own time zone
//! like a mosque timetable. Egypt keeps Hilal's own Egyptian-survey times (Dhuhr rounded
//! down, as Google and the Egyptian tables show it). The place is a city from the list,
//! or "Auto": found once a day from the PC's internet address.

use windows::core::HSTRING;
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};

/// Name, latitude, longitude, country (ISO code, which picks the method), time zone.
/// Egypt's first, as they always were (saved choices are indexes into this), then the
/// world's.
pub const CITIES: [(&str, f64, f64, &str, &str); 77] = [
    ("Cairo", 30.0444, 31.2357, "EG", "Africa/Cairo"),
    ("Maadi", 29.9602, 31.2569, "EG", "Africa/Cairo"),
    ("Nasr City", 30.0566, 31.3301, "EG", "Africa/Cairo"),
    ("New Cairo", 30.0074, 31.4913, "EG", "Africa/Cairo"),
    ("Helwan", 29.8414, 31.3008, "EG", "Africa/Cairo"),
    ("Shubra El Kheima", 30.1286, 31.2422, "EG", "Africa/Cairo"),
    ("6th of October", 29.9285, 30.9188, "EG", "Africa/Cairo"),
    ("Giza", 30.0131, 31.2089, "EG", "Africa/Cairo"),
    ("Alexandria", 31.2001, 29.9187, "EG", "Africa/Cairo"),
    ("Port Said", 31.2653, 32.3019, "EG", "Africa/Cairo"),
    ("Suez", 29.9668, 32.5498, "EG", "Africa/Cairo"),
    ("Ismailia", 30.5965, 32.2715, "EG", "Africa/Cairo"),
    ("Damietta", 31.4165, 31.8133, "EG", "Africa/Cairo"),
    ("Mansoura", 31.0409, 31.3785, "EG", "Africa/Cairo"),
    ("Tanta", 30.7865, 31.0004, "EG", "Africa/Cairo"),
    ("Zagazig", 30.5877, 31.5020, "EG", "Africa/Cairo"),
    ("Banha", 30.4660, 31.1858, "EG", "Africa/Cairo"),
    ("Shibin El Kom", 30.5503, 31.0106, "EG", "Africa/Cairo"),
    ("Kafr El Sheikh", 31.1107, 30.9388, "EG", "Africa/Cairo"),
    ("Damanhur", 31.0341, 30.4682, "EG", "Africa/Cairo"),
    ("Marsa Matruh", 31.3543, 27.2373, "EG", "Africa/Cairo"),
    ("Arish", 31.1316, 33.7984, "EG", "Africa/Cairo"),
    ("Faiyum", 29.3084, 30.8428, "EG", "Africa/Cairo"),
    ("Beni Suef", 29.0661, 31.0994, "EG", "Africa/Cairo"),
    ("Minya", 28.0871, 30.7618, "EG", "Africa/Cairo"),
    ("Asyut", 27.1809, 31.1837, "EG", "Africa/Cairo"),
    ("Sohag", 26.5591, 31.6957, "EG", "Africa/Cairo"),
    ("Qena", 26.1551, 32.7160, "EG", "Africa/Cairo"),
    ("Luxor", 25.6872, 32.6396, "EG", "Africa/Cairo"),
    ("Aswan", 24.0889, 32.8998, "EG", "Africa/Cairo"),
    ("Hurghada", 27.2579, 33.8116, "EG", "Africa/Cairo"),
    ("Sharm El Sheikh", 27.9158, 34.3300, "EG", "Africa/Cairo"),
    ("El Kharga", 25.4390, 30.5586, "EG", "Africa/Cairo"),
    ("Mecca", 21.4225, 39.8262, "SA", "Asia/Riyadh"),
    ("Medina", 24.4672, 39.6111, "SA", "Asia/Riyadh"),
    ("Riyadh", 24.7136, 46.6753, "SA", "Asia/Riyadh"),
    ("Jeddah", 21.4858, 39.1925, "SA", "Asia/Riyadh"),
    ("Dammam", 26.4207, 50.0888, "SA", "Asia/Riyadh"),
    ("Dubai", 25.2048, 55.2708, "AE", "Asia/Dubai"),
    ("Abu Dhabi", 24.4539, 54.3773, "AE", "Asia/Dubai"),
    ("Sharjah", 25.3463, 55.4209, "AE", "Asia/Dubai"),
    ("Doha", 25.2854, 51.5310, "QA", "Asia/Qatar"),
    ("Kuwait City", 29.3759, 47.9774, "KW", "Asia/Kuwait"),
    ("Manama", 26.2285, 50.5860, "BH", "Asia/Bahrain"),
    ("Muscat", 23.5880, 58.3829, "OM", "Asia/Muscat"),
    ("Amman", 31.9539, 35.9106, "JO", "Asia/Amman"),
    ("Jerusalem", 31.7683, 35.2137, "PS", "Asia/Hebron"),
    ("Gaza", 31.5017, 34.4668, "PS", "Asia/Gaza"),
    ("Beirut", 33.8938, 35.5018, "LB", "Asia/Beirut"),
    ("Damascus", 33.5138, 36.2765, "SY", "Asia/Damascus"),
    ("Baghdad", 33.3152, 44.3661, "IQ", "Asia/Baghdad"),
    ("Khartoum", 15.5007, 32.5599, "SD", "Africa/Khartoum"),
    ("Tripoli", 32.8872, 13.1913, "LY", "Africa/Tripoli"),
    ("Tunis", 36.8065, 10.1815, "TN", "Africa/Tunis"),
    ("Algiers", 36.7538, 3.0588, "DZ", "Africa/Algiers"),
    ("Casablanca", 33.5731, -7.5898, "MA", "Africa/Casablanca"),
    ("Rabat", 34.0209, -6.8416, "MA", "Africa/Casablanca"),
    ("Istanbul", 41.0082, 28.9784, "TR", "Europe/Istanbul"),
    ("Ankara", 39.9334, 32.8597, "TR", "Europe/Istanbul"),
    ("Tehran", 35.6892, 51.3890, "IR", "Asia/Tehran"),
    ("Karachi", 24.8607, 67.0011, "PK", "Asia/Karachi"),
    ("Lahore", 31.5204, 74.3587, "PK", "Asia/Karachi"),
    ("Delhi", 28.6139, 77.2090, "IN", "Asia/Kolkata"),
    ("Dhaka", 23.8103, 90.4125, "BD", "Asia/Dhaka"),
    ("Kuala Lumpur", 3.1390, 101.6869, "MY", "Asia/Kuala_Lumpur"),
    ("Jakarta", -6.2088, 106.8456, "ID", "Asia/Jakarta"),
    ("Singapore", 1.3521, 103.8198, "SG", "Asia/Singapore"),
    ("London", 51.5072, -0.1276, "GB", "Europe/London"),
    ("Paris", 48.8566, 2.3522, "FR", "Europe/Paris"),
    ("Berlin", 52.5200, 13.4050, "DE", "Europe/Berlin"),
    ("Amsterdam", 52.3676, 4.9041, "NL", "Europe/Amsterdam"),
    ("Stockholm", 59.3293, 18.0686, "SE", "Europe/Stockholm"),
    ("New York", 40.7128, -74.0060, "US", "America/New_York"),
    ("Chicago", 41.8781, -87.6298, "US", "America/Chicago"),
    ("Los Angeles", 34.0522, -118.2437, "US", "America/Los_Angeles"),
    ("Toronto", 43.6532, -79.3832, "CA", "America/Toronto"),
    ("Sydney", -33.8688, 151.2093, "AU", "Australia/Sydney"),
];

/// The choice after the list: the place found from the internet address.
pub const AUTO: usize = CITIES.len();

/// How many places there are to choose from (the list, then Auto).
pub const CHOICES: usize = CITIES.len() + 1;

/// A place to work the times out for.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub country: String,
    /// IANA time zone ("Asia/Riyadh"); empty means the PC's own.
    pub tz: String,
    /// A guess (the capital of the PC's time zone) until the real place is found.
    pub approx: bool,
}

/// The chosen place: a city from the list, or the detected one. Before Auto has found
/// anything, the capital of the PC's time zone (marked approximate), else Cairo.
pub fn place(choice: usize, auto: Option<&crate::store::AutoPlace>) -> Place {
    let city = |i: usize| {
        let (name, lat, lon, country, tz) = CITIES[i];
        Place { name: name.to_owned(), lat, lon, country: country.to_owned(), tz: tz.to_owned(), approx: false }
    };
    if choice < AUTO {
        return city(choice);
    }
    match auto {
        Some(a) => Place { name: a.city.clone(), lat: a.lat, lon: a.lon, country: a.country.clone(), tz: a.tz.clone(), approx: false },
        None => capital_of_pc_zone().unwrap_or_else(|| city(0)),
    }
}

/// The capital of the PC's time zone, as a stand-in while there's no internet yet.
fn capital_of_pc_zone() -> Option<Place> {
    const CAPITALS: [(&str, &str); 26] = [
        ("Africa/Cairo", "Cairo"), ("Asia/Riyadh", "Riyadh"), ("Asia/Dubai", "Dubai"), ("Asia/Kuwait", "Kuwait City"),
        ("Asia/Qatar", "Doha"), ("Asia/Bahrain", "Manama"), ("Asia/Muscat", "Muscat"), ("Asia/Amman", "Amman"),
        ("Asia/Beirut", "Beirut"), ("Asia/Damascus", "Damascus"), ("Asia/Baghdad", "Baghdad"), ("Asia/Gaza", "Gaza"),
        ("Asia/Hebron", "Jerusalem"), ("Africa/Casablanca", "Rabat"), ("Africa/Algiers", "Algiers"), ("Africa/Tunis", "Tunis"),
        ("Africa/Tripoli", "Tripoli"), ("Africa/Khartoum", "Khartoum"), ("Europe/Istanbul", "Istanbul"), ("Asia/Karachi", "Karachi"),
        ("Asia/Jakarta", "Jakarta"), ("Asia/Kuala_Lumpur", "Kuala Lumpur"), ("Europe/London", "London"), ("Europe/Paris", "Paris"),
        ("America/New_York", "New York"), ("Asia/Tehran", "Tehran"),
    ];
    let zone = pc_zone()?;
    let name = CAPITALS.iter().find(|c| c.0 == zone)?.1;
    let i = CITIES.iter().position(|c| c.0 == name)?;
    let (name, lat, lon, country, tz) = CITIES[i];
    Some(Place { name: name.to_owned(), lat, lon, country: country.to_owned(), tz: tz.to_owned(), approx: true })
}

/// Finds where this PC is from its internet address, trying three free services in turn
/// (they rate-limit often): city, position, country and time zone.
pub fn detect() -> Option<crate::store::AutoPlace> {
    let get = |url: &str| -> Option<serde_json::Value> {
        ureq::get(url).timeout(std::time::Duration::from_secs(10)).call().ok()?.into_json().ok()
    };
    let num = |v: &serde_json::Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()));
    let text = |v: &serde_json::Value| v.as_str().unwrap_or("").to_owned();
    let services: [(&str, fn(&serde_json::Value) -> &serde_json::Value); 3] = [
        ("https://ipwho.is/", |j| &j["timezone"]["id"]),
        ("https://get.geojs.io/v1/ip/geo.json", |j| &j["timezone"]),
        ("https://ipapi.co/json/", |j| &j["timezone"]),
    ];
    for (url, zone) in services {
        let Some(j) = get(url) else { continue };
        if j["success"] == false {
            continue;
        }
        if let (Some(lat), Some(lon)) = (num(&j["latitude"]), num(&j["longitude"])) {
            return Some(crate::store::AutoPlace {
                city: j["city"].as_str().filter(|c| !c.is_empty()).unwrap_or("Here").to_owned(),
                lat,
                lon,
                country: text(&j["country_code"]).to_ascii_uppercase(),
                tz: text(zone(&j)),
                day: today_key(),
            });
        }
    }
    None
}

/// Today as yyyymmdd (the PC's date).
pub fn today_key() -> u32 {
    let (y, m, d, _) = today();
    y as u32 * 10000 + m * 100 + d
}

pub const NAMES: [&str; 6] = ["Fajr", "Sunrise", "Dhuhr", "Asr", "Maghrib", "Isha"];

/// The choices that change the times: the Hijri date moved by a day or two (which moves
/// Ramadan, and so Umm al-Qura's Isha), and Asr by the Hanafi shadow rule.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Calc {
    pub hijri_adjust: i32,
    pub hanafi: bool,
}

// ---- time zones (Windows' own calendar knows the IANA names and their summer times) ----

/// The PC's IANA time zone ("Africa/Cairo").
fn pc_zone() -> Option<String> {
    let cal = windows::Globalization::Calendar::new().ok()?;
    cal.GetTimeZone().ok().map(|z| z.to_string()).filter(|z| !z.is_empty())
}

/// Seconds since 1970 (UTC), now.
fn utc_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The wall clock in `zone` at a moment (seconds since 1970, UTC): date, and seconds since
/// that day's midnight. None if the zone isn't known.
fn wall_clock(zone: &str, unix: i64) -> Option<(i32, u32, u32, i32)> {
    use windows::Foundation::DateTime;
    let cal = windows::Globalization::Calendar::new().ok()?;
    cal.ChangeClock(&HSTRING::from("24HourClock")).ok()?;
    cal.ChangeCalendarSystem(&HSTRING::from("GregorianCalendar")).ok()?;
    cal.ChangeTimeZone(&HSTRING::from(zone)).ok()?;
    cal.SetDateTime(DateTime { UniversalTime: (unix + 11_644_473_600) * 10_000_000 }).ok()?;
    let secs = cal.Hour().ok()? * 3600 + cal.Minute().ok()? * 60 + cal.Second().ok()?;
    Some((cal.Year().ok()?, cal.Month().ok()? as u32, cal.Day().ok()? as u32, secs))
}

/// Days since 1970 for a date.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// How many hours `zone`'s clocks run ahead of UTC around noon on a day (the PC's offset if
/// the zone is empty or unknown).
fn offset_on(zone: &str, y: i32, m: u32, d: u32) -> f64 {
    let noon = days_from_civil(y, m, d) * 86_400 + 12 * 3600;
    match (!zone.is_empty()).then(|| wall_clock(zone, noon)).flatten() {
        Some((wy, wm, wd, secs)) => {
            let shown = days_from_civil(wy, wm, wd) * 86_400 + secs as i64;
            (shown - noon) as f64 / 3600.0
        }
        None => today().3,
    }
}

/// The place's date now and the seconds since its midnight (its own clock, which may not
/// be the PC's).
pub fn local_now(p: &Place) -> (i32, u32, u32, i32) {
    if !p.tz.is_empty() {
        if let Some(now) = wall_clock(&p.tz, utc_now()) {
            return now;
        }
    }
    let (y, m, d, _) = today();
    (y, m, d, crate::win::seconds_of_day() as i32)
}

// ---- methods (Nova Island's table) ----

/// Isha (and Maghrib) by the sun's depth, or as minutes after Maghrib (after sunset).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    Angle(f64),
    Minutes(f64),
}

/// How a country works out its times.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Method {
    pub fajr: f64,
    pub isha: Rule,
    /// None: at sunset.
    pub maghrib: Option<Rule>,
    /// The authority's own minutes added to Dhuhr and Maghrib.
    pub tune_dhuhr: f64,
    pub tune_maghrib: f64,
}

/// The method a country follows (the Muslim World League's where it has none of its own).
pub fn method(country: &str) -> Method {
    use Rule::*;
    let m = |fajr, isha| Method { fajr, isha, maghrib: None, tune_dhuhr: 0.0, tune_maghrib: 0.0 };
    match country {
        "EG" | "SD" | "LY" | "SY" | "LB" => m(19.5, Angle(17.5)),
        "SA" | "YE" => m(18.5, Minutes(90.0)),
        "AE" => Method { tune_dhuhr: 3.0, tune_maghrib: 3.0, ..m(18.2, Angle(18.2)) },
        "KW" => m(18.0, Angle(17.5)),
        "QA" => m(18.0, Minutes(90.0)),
        "BH" | "OM" => m(19.5, Minutes(90.0)),
        "JO" | "PS" => Method { maghrib: Some(Minutes(5.0)), ..m(18.0, Angle(18.0)) },
        "MA" => m(19.0, Angle(17.0)),
        "DZ" => m(18.0, Angle(17.0)),
        "TN" => m(18.0, Angle(18.0)),
        "TR" => m(18.0, Angle(17.0)),
        "IR" => Method { maghrib: Some(Angle(4.5)), ..m(17.7, Angle(14.0)) },
        "PK" | "IN" | "BD" | "AF" => m(18.0, Angle(18.0)),
        "SG" | "BN" => m(20.0, Angle(18.0)),
        "MY" => m(20.0, Angle(18.0)),
        "ID" => m(20.0, Angle(18.0)),
        "US" | "CA" => m(15.0, Angle(15.0)),
        "FR" => m(12.0, Angle(12.0)),
        "RU" => m(16.0, Angle(15.0)),
        _ => m(18.0, Angle(17.0)),
    }
}

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

fn acot(x: f64) -> f64 {
    (1.0 / x).atan().to_degrees()
}

/// Egypt's six times (Hilal's own: two refinement passes), in local hours for a place
/// whose clocks run `tz` hours ahead of UTC.
fn egypt_times(y: i32, m: u32, d: u32, lat: f64, lon: f64, tz: f64, shadow: f64) -> [f64; 6] {
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
        let angle = -(1.0 / (shadow + tan((lat - decl).abs()))).atan().to_degrees();
        at_angle(angle, t, false)
    };
    // Two passes: the sun's position at the rough time, then at the better one.
    let mut t = [5.0, 6.0, 12.0, 13.0, 18.0, 18.0];
    for _ in 0..2 {
        t = [
            at_angle(19.5, t[0], true),
            at_angle(RISE_ANGLE, t[1], true),
            noon(t[2]),
            asr(t[3]),
            at_angle(RISE_ANGLE, t[4], false),
            at_angle(17.5, t[5], false),
        ];
    }
    t.map(|h| h + tz - lon / 15.0)
}

/// The six times in UTC hours, as Nova Island works them out (PrayTimes: one refinement
/// pass from rough guesses; Fajr and Isha kept within a share of the night).
fn nova_times(y: i32, m: u32, d: u32, lat: f64, lon: f64, method: Method, ramadan: bool, shadow: f64) -> [f64; 6] {
    let jdate = julian(y, m, d) - lon / (15.0 * 24.0);
    let mid_day = |t: f64| fix(12.0 - sun(jdate + t).1, 24.0);
    let sun_angle_time = |angle: f64, t: f64, ccw: bool| {
        let decl = sun(jdate + t).0;
        let noon = mid_day(t);
        let h = ((-sin(angle) - sin(decl) * sin(lat)) / (cos(decl) * cos(lat))).acos().to_degrees() / 15.0;
        noon + if ccw { -h } else { h }
    };
    let asr_time = |factor: f64, t: f64| {
        let decl = sun(jdate + t).0;
        sun_angle_time(-acot(factor + tan((lat - decl).abs())), t, false)
    };
    let g = |h: f64| h / 24.0;
    let shift = lon / 15.0;
    let mut fajr = sun_angle_time(method.fajr, g(5.0), true) - shift;
    let sunrise = sun_angle_time(RISE_ANGLE, g(6.0), true) - shift;
    let dhuhr = mid_day(g(12.0)) - shift;
    let asr = asr_time(shadow, g(13.0)) - shift;
    let sunset = sun_angle_time(RISE_ANGLE, g(18.0), false) - shift;
    let night = fix(sunrise - sunset, 24.0);
    let portion = |angle: f64| angle / 60.0 * night;
    let p = portion(method.fajr);
    if fajr.is_nan() || fix(sunrise - fajr, 24.0) > p {
        fajr = sunrise - p;
    }
    let maghrib = match method.maghrib {
        Some(Rule::Angle(a)) => sun_angle_time(a, g(18.0), false) - shift,
        Some(Rule::Minutes(min)) => sunset + min / 60.0,
        None => sunset,
    };
    let isha = match method.isha {
        // Umm al-Qura: two hours after Maghrib in Ramadan.
        Rule::Minutes(min) => maghrib + if ramadan && method.fajr == 18.5 { 120.0 } else { min } / 60.0,
        Rule::Angle(a) => {
            let t = sun_angle_time(a, g(18.0), false) - shift;
            let p = portion(a);
            if t.is_nan() || fix(t - sunset, 24.0) > p { sunset + p } else { t }
        }
    };
    // The authority's own offsets, and a minute of precaution on Asr.
    [fajr, sunrise, dhuhr + method.tune_dhuhr / 60.0, asr + 1.0 / 60.0, maghrib + method.tune_maghrib / 60.0, isha]
}

/// Egypt's times as minutes since midnight (Dhuhr rounded down).
fn minutes(t: [f64; 6]) -> [i32; 6] {
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

/// A day's six times at a place, in minutes since that place's midnight (its own clock).
pub fn day_times(p: &Place, y: i32, m: u32, d: u32, calc: Calc) -> [i32; 6] {
    let tz = offset_on(&p.tz, y, m, d);
    let shadow = if calc.hanafi { 2.0 } else { 1.0 };
    if p.country == "EG" {
        return minutes(egypt_times(y, m, d, p.lat, p.lon, tz, shadow));
    }
    let ramadan = hijri(y, m, d, calc.hijri_adjust).is_some_and(|h| h.1 == 9);
    nova_times(y, m, d, p.lat, p.lon, method(&p.country), ramadan, shadow).map(|h| ((h + tz) * 60.0 + 0.5).floor() as i32)
}

/// The prayer (index into `NAMES`, never Sunrise) whose time came within the last ten
/// minutes at the place, with its date as yyyymmdd and its time in minutes.
pub fn started(p: &Place, calc: Calc) -> Option<(u32, usize, i32)> {
    let (y, m, d, now) = local_now(p);
    let t = day_times(p, y, m, d, calc);
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

/// The prayer (never Sunrise) due in `lead` minutes or a little less (within the last minute
/// of that window), for a heads-up before it.
pub fn coming(p: &Place, lead: i32, calc: Calc) -> Option<(u32, usize, i32)> {
    let (y, m, d, now) = local_now(p);
    let t = day_times(p, y, m, d, calc);
    // HILAL_HEADSUP_TEST=<index> makes that prayer's heads-up show now (for testing).
    if let Some(i) = std::env::var("HILAL_HEADSUP_TEST").ok().and_then(|v| v.parse::<usize>().ok()).filter(|&i| i < 6) {
        return Some((now as u32, i, t[i]));
    }
    [0, 2, 3, 4, 5]
        .into_iter()
        .find(|&i| {
            let left = t[i] * 60 - now;
            left > 0 && left <= lead * 60 && left > (lead - 1) * 60
        })
        .map(|i| (y as u32 * 10000 + m * 100 + d, i, t[i]))
}

/// The Hijri (Umm al-Qura) date for a day, moved by `adjust` days (a local moon sighting),
/// from Windows' own calendar: the text to show ("14 جمادى الأولى 1448") and the month.
pub fn hijri(y: i32, m: u32, d: u32, adjust: i32) -> Option<(String, u32)> {
    use windows::core::w;
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::Globalization::{GetDateFormatEx, ENUM_DATE_FORMATS_FLAGS};
    let mut day = (y, m, d);
    for _ in 0..adjust.max(0) {
        day = next_day(day.0, day.1, day.2);
    }
    for _ in 0..(-adjust).max(0) {
        day = prev_day(day.0, day.1, day.2);
    }
    let st = SYSTEMTIME { wYear: day.0 as u16, wMonth: day.1 as u16, wDay: day.2 as u16, ..Default::default() };
    let fmt = |f: windows::core::PCWSTR| -> Option<String> {
        let mut buf = [0u16; 64];
        let n = unsafe { GetDateFormatEx(w!("ar-SA"), ENUM_DATE_FORMATS_FLAGS(0), Some(&st), f, Some(&mut buf), None) };
        (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
    };
    let text = fmt(w!("d MMMM yyyy"))?;
    let month = fmt(w!("M"))?.trim().parse().ok()?;
    Some((text, month))
}

/// The day before (y, m, d).
pub fn prev_day(y: i32, m: u32, d: u32) -> (i32, u32, u32) {
    if d > 1 {
        return (y, m, d - 1);
    }
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    let leap = (py % 4 == 0 && py % 100 != 0) || py % 400 == 0;
    (py, pm, [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][pm as usize - 1])
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

    fn got(name: &str) -> Vec<String> {
        let i = CITIES.iter().position(|c| c.0 == name).unwrap();
        day_times(&place(i, None), 2026, 11, 1, Calc::default()).iter().map(|m| format!("{:02}:{:02}", m / 60, m % 60)).collect()
    }

    /// Other countries, Nova Island's way, against aladhan.com for 1 Nov 2026 (each city in
    /// its own time zone): within a minute, Asr's minute of precaution included.
    #[test]
    fn matches_other_countries() {
        let want = [
            ("Mecca", ["05:06", "06:24", "12:04", "15:20", "17:45", "19:15"]),
            ("Dubai", ["05:08", "06:26", "12:05", "15:15", "17:41", "18:57"]),
            ("London", ["05:00", "06:54", "11:44", "14:08", "16:33", "18:20"]),
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
    fn hijri_date_is_umm_al_qura() {
        let (text, month) = hijri(2026, 11, 1, 0).expect("no Hijri date");
        assert_eq!(month, 5, "{text}");
        assert!(text.contains("1448"), "{text}");
        // a day later with the moon-sighting adjustment
        assert_ne!(hijri(2026, 11, 1, 1).unwrap().0, text);
    }

    #[test]
    fn time_zones_follow_the_place() {
        assert_eq!(offset_on("Asia/Riyadh", 2026, 11, 1), 3.0);
        assert_eq!(offset_on("Europe/London", 2026, 7, 1), 1.0);
        assert_eq!(offset_on("Europe/London", 2026, 12, 1), 0.0);
    }

    #[test]
    fn saved_egypt_choices_keep_their_city() {
        assert_eq!(CITIES[1].0, "Maadi");
        assert!(!place(AUTO, None).name.is_empty());
    }

    #[test]
    fn days_step_both_ways() {
        assert_eq!(prev_day(2026, 3, 1), (2026, 2, 28));
        assert_eq!(prev_day(2027, 1, 1), (2026, 12, 31));
        assert_eq!(next_day(2026, 12, 31), (2027, 1, 1));
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
