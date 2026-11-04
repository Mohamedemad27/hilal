//! Upcoming meetings from a private iCal link (Google Calendar, Outlook.com, iCloud: the
//! "secret address in iCal format"), as Nova Island has them. No sign-in: the link itself is
//! the key, so it's kept only in Hilal's settings and never logged. Read every 10 minutes
//! (2 after a failure), for the next 24 hours: repeating meetings are expanded, moved and
//! cancelled ones honoured, all-day events left out.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use crate::prayer::{days_from_civil, offset_on};

#[derive(Clone, Debug, PartialEq)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// Seconds since 1970 (UTC).
    pub start: i64,
    pub end: i64,
    /// A Meet, Teams, Zoom or Webex link, for the Join button.
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub meetings: Vec<Meeting>,
    /// What went wrong with the last read ("" when it worked).
    pub error: String,
    /// When it last worked (seconds since 1970; 0: never).
    pub updated: i64,
}

pub struct Feed {
    pub updates: Receiver<State>,
    url: Sender<String>,
}

const AHEAD: i64 = 24 * 3600;
const MAX_EVENTS: usize = 12;

impl Feed {
    pub fn start(url: String) -> Self {
        let (tx, rx) = channel();
        let (url_tx, url_rx) = channel::<String>();
        std::thread::spawn(move || {
            let mut url = url;
            let mut last = State::default();
            loop {
                let wait = if url.is_empty() {
                    let _ = tx.send(State::default());
                    None
                } else {
                    match read(&url) {
                        Ok(meetings) => {
                            last = State { meetings, error: String::new(), updated: crate::prayer::utc_now() };
                            let _ = tx.send(last.clone());
                            Some(Duration::from_secs(10 * 60))
                        }
                        Err(e) => {
                            last.error = e;
                            let _ = tx.send(last.clone());
                            Some(Duration::from_secs(2 * 60))
                        }
                    }
                };
                // Sleeps until it's time again, or the link changes.
                let got = match wait {
                    Some(d) => url_rx.recv_timeout(d).ok(),
                    None => url_rx.recv().ok(),
                };
                if let Some(new) = got {
                    url = new;
                    last = State::default();
                }
            }
        });
        Self { updates: rx, url: url_tx }
    }

    /// A new link (empty: none), read at once.
    pub fn set_url(&self, url: String) {
        let _ = self.url.send(url);
    }
}

/// A link as given: "webcal://" reads as "https://". None if it isn't an https link.
pub fn normalize(url: &str) -> Option<String> {
    let u = url.trim();
    let u = if u.len() > 9 && u[..9].eq_ignore_ascii_case("webcal://") { format!("https://{}", &u[9..]) } else { u.to_owned() };
    (u.starts_with("https://") && !u.contains(char::is_whitespace) && u.len() > 12).then_some(u)
}

fn read(url: &str) -> Result<Vec<Meeting>, String> {
    // HILAL_CALENDAR_TEST=<file.ics> reads that file instead (for testing).
    if let Some(path) = url.strip_prefix("test:") {
        let body = std::fs::read_to_string(path).map_err(|_| "no test file".to_owned())?;
        let now = crate::prayer::utc_now();
        return Ok(occurrences(&body, now, now + AHEAD));
    }
    let r = ureq::get(url).set("accept", "text/calendar").timeout(Duration::from_secs(15)).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        _ => "no connection".to_owned(),
    })?;
    let body = r.into_string().map_err(|_| "unreadable".to_owned())?;
    if !body.contains("BEGIN:VCALENDAR") {
        return Err("not an iCal file".to_owned());
    }
    let now = crate::prayer::utc_now();
    Ok(occurrences(&body, now, now + AHEAD))
}

/// One VEVENT's fields (unfolded lines), by name, with their parameters.
#[derive(Default, Debug)]
struct Event {
    props: Vec<(String, String, String)>,
}

impl Event {
    fn get(&self, name: &str) -> Option<(&str, &str)> {
        self.props.iter().find(|p| p.0 == name).map(|p| (p.1.as_str(), p.2.as_str()))
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        self.props.iter().filter(move |p| p.0 == name).map(|p| (p.1.as_str(), p.2.as_str()))
    }
}

fn events(body: &str) -> Vec<Event> {
    // Long lines continue on the next one, starting with a space or tab.
    let mut lines: Vec<String> = Vec::new();
    for raw in body.split('\n') {
        let raw = raw.trim_end_matches('\r');
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&raw[1..]);
        } else {
            lines.push(raw.to_owned());
        }
    }
    let mut out = Vec::new();
    let mut cur: Option<Event> = None;
    let mut depth = 0;
    for l in lines {
        if l == "BEGIN:VEVENT" {
            cur = Some(Event::default());
            depth = 0;
            continue;
        }
        if l == "END:VEVENT" {
            if let Some(e) = cur.take() {
                out.push(e);
            }
            continue;
        }
        let Some(e) = cur.as_mut() else { continue };
        // (Alarms inside an event aren't the event's own fields.)
        if l.starts_with("BEGIN:") {
            depth += 1;
            continue;
        }
        if l.starts_with("END:") {
            depth -= 1;
            continue;
        }
        if depth > 0 {
            continue;
        }
        let Some((head, value)) = l.split_once(':') else { continue };
        let (name, params) = head.split_once(';').unwrap_or((head, ""));
        e.props.push((name.to_ascii_uppercase(), params.to_owned(), value.to_owned()));
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\N", "\n").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// A DATE-TIME (or DATE) as seconds since 1970, and whether it was a whole day.
fn when(params: &str, value: &str) -> Option<(i64, bool)> {
    let v = value.trim();
    let num = |a: usize, b: usize| v.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(4, 6)?, num(6, 8)?);
    let day = days_from_civil(y as i32, m as u32, d as u32);
    if v.len() == 8 || params.contains("VALUE=DATE") && !v.contains('T') {
        return Some((day * 86_400, true));
    }
    let (hh, mm, ss) = (num(9, 11)?, num(11, 13)?, num(13, 15).unwrap_or(0));
    let local = day * 86_400 + hh * 3600 + mm * 60 + ss;
    if v.ends_with('Z') {
        return Some((local, false));
    }
    // In a time zone (an IANA name as Google and iCloud give them; anything else, and a
    // floating time, reads as the PC's own).
    let zone = params.split(';').find_map(|p| p.strip_prefix("TZID=")).unwrap_or("").trim_matches('"');
    let off = offset_on(zone, y as i32, m as u32, d as u32);
    Some((local - (off * 3600.0).round() as i64, false))
}

/// A duration like "PT1H30M" in seconds.
fn duration(v: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut num = 0i64;
    let mut sign = 1;
    for c in v.chars() {
        match c {
            '-' => sign = -1,
            '0'..='9' => num = num * 10 + c.to_digit(10)? as i64,
            'W' => (total, num) = (total + num * 7 * 86_400, 0),
            'D' => (total, num) = (total + num * 86_400, 0),
            'H' => (total, num) = (total + num * 3600, 0),
            'M' => (total, num) = (total + num * 60, 0),
            'S' => (total, num) = (total + num, 0),
            _ => {}
        }
    }
    Some(sign * total)
}

/// A meeting link worth a Join button, in any of the event's text.
fn join_link(texts: &[&str]) -> Option<String> {
    const HOSTS: [&str; 5] = ["https://meet.google.com/", "https://teams.microsoft.com/", "https://teams.live.com/", ".zoom.us/", ".webex.com/"];
    for t in texts {
        let t = unescape(t);
        for word in t.split(|c: char| c.is_whitespace() || c == '"' || c == '<' || c == '>' || c == '\'') {
            let w = word.trim_end_matches([')', '.', ',', ';']);
            let zoom = w.starts_with("https://zoom.us/") || (w.starts_with("https://") && w.contains(".zoom.us/"));
            if w.starts_with("https://") && (zoom || HOSTS.iter().any(|h| w.contains(h))) {
                return Some(w.to_owned());
            }
        }
    }
    None
}

/// Every meeting (one per occurrence) that runs into [from, to).
pub fn occurrences(body: &str, from: i64, to: i64) -> Vec<Meeting> {
    let all = events(body);
    // Moved or cancelled occurrences of a series: by its UID and the original start.
    let mut moved: Vec<(String, i64, &Event)> = Vec::new();
    for e in &all {
        if let (Some((_, uid)), Some((p, v))) = (e.get("UID"), e.get("RECURRENCE-ID")) {
            if let Some((t, _)) = when(p, v) {
                moved.push((uid.to_owned(), t, e));
            }
        }
    }
    let mut out = Vec::new();
    for e in &all {
        if e.get("RECURRENCE-ID").is_some() {
            continue; // (handled with its series)
        }
        let cancelled = |ev: &Event| ev.get("STATUS").is_some_and(|s| s.1.eq_ignore_ascii_case("CANCELLED"));
        if cancelled(e) {
            continue;
        }
        let Some((sp, sv)) = e.get("DTSTART") else { continue };
        let Some((start, all_day)) = when(sp, sv) else { continue };
        if all_day {
            continue;
        }
        let end = e.get("DTEND").and_then(|(p, v)| when(p, v)).map(|t| t.0).or_else(|| e.get("DURATION").and_then(|d| duration(d.1)).map(|d| start + d)).unwrap_or(start);
        let dur = (end - start).max(0);
        let uid = e.get("UID").map_or(String::new(), |u| u.1.to_owned());
        let push = |out: &mut Vec<Meeting>, s: i64, ev: &Event| {
            let en = s + dur;
            if en <= from || s >= to {
                return;
            }
            let texts: Vec<&str> = ["LOCATION", "URL", "DESCRIPTION", "X-GOOGLE-CONFERENCE"].iter().filter_map(|k| ev.get(k).map(|p| p.1)).collect();
            let title = ev.get("SUMMARY").map(|s| unescape(s.1).trim().to_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "(No title)".to_owned());
            out.push(Meeting { id: format!("{uid}|{s}"), title, start: s, end: en, link: join_link(&texts) });
        };
        let Some((_, rule)) = e.get("RRULE") else {
            push(&mut out, start, e);
            continue;
        };
        let exdates: Vec<i64> = e.all("EXDATE").flat_map(|(p, v)| v.split(',').filter_map(move |x| when(p, x).map(|t| t.0))).collect();
        for s in expand(rule, start, sp, from - dur, to) {
            if let Some((_, _, m)) = moved.iter().find(|(u, t, _)| *u == uid && *t == s) {
                if !cancelled(m) {
                    if let Some((p, v)) = m.get("DTSTART") {
                        if let Some((ms, _)) = when(p, v) {
                            push(&mut out, ms, m);
                        }
                    }
                }
            } else if !exdates.contains(&s) {
                push(&mut out, s, e);
            }
        }
    }
    out.sort_by_key(|m| m.start);
    out.truncate(MAX_EVENTS);
    out
}

/// The starts of a repeating event (RRULE: DAILY, WEEKLY with BYDAY, MONTHLY, YEARLY; with
/// INTERVAL, COUNT, UNTIL) that fall in [from, to). Weekly days follow the event's own wall
/// clock, so a summer time change keeps the meeting at its hour.
fn expand(rule: &str, start: i64, params: &str, from: i64, to: i64) -> Vec<i64> {
    let get = |k: &str| rule.split(';').find_map(|p| p.strip_prefix(k).and_then(|r| r.strip_prefix('=')));
    let freq = get("FREQ").unwrap_or("");
    let interval = get("INTERVAL").and_then(|v| v.parse::<i64>().ok()).unwrap_or(1).max(1);
    let count = get("COUNT").and_then(|v| v.parse::<usize>().ok());
    let until = get("UNTIL").and_then(|v| when("", v)).map(|t| if t.1 { t.0 + 86_399 } else { t.0 });
    let zone = params.split(';').find_map(|p| p.strip_prefix("TZID=")).unwrap_or("").trim_matches('"').to_owned();
    // The start in the event's own wall clock.
    let off0 = |t: i64| {
        let d = t.div_euclid(86_400);
        let (y, m, dd) = civil(d);
        (offset_on(&zone, y, m, dd) * 3600.0).round() as i64
    };
    let wall0 = start + if params.contains("TZID=") || !rule.is_empty() { off0(start) } else { 0 };
    let tod = wall0.rem_euclid(86_400);
    let day0 = wall0.div_euclid(86_400);
    let to_utc = |day: i64| {
        let local = day * 86_400 + tod;
        local - off0(local)
    };
    const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
    let byday: Vec<i64> = get("BYDAY").map(|v| v.split(',').filter_map(|d| DAYS.iter().position(|x| d.ends_with(x)).map(|i| i as i64)).collect()).unwrap_or_default();
    let weekday = |day: i64| (day + 3).rem_euclid(7); // 0 Monday (1970-01-01 was a Thursday)
    let mut out = Vec::new();
    let mut n = 0usize;
    let mut take = |t: i64, out: &mut Vec<i64>| -> bool {
        if until.is_some_and(|u| t > u) || count.is_some_and(|c| n >= c) || t >= to {
            return false;
        }
        n += 1;
        if t >= from {
            out.push(t);
        }
        true
    };
    match freq {
        "DAILY" => {
            let mut d = day0;
            for _ in 0..20_000 {
                if !take(to_utc(d), &mut out) {
                    break;
                }
                d += interval;
            }
        }
        "WEEKLY" => {
            let days = if byday.is_empty() { vec![weekday(day0)] } else { byday };
            let week0 = day0 - weekday(day0);
            let mut w = 0;
            'weeks: for _ in 0..5_000 {
                for &k in &days {
                    let d = week0 + w * 7 * interval + k;
                    if d < day0 {
                        continue;
                    }
                    if !take(to_utc(d), &mut out) {
                        break 'weeks;
                    }
                }
                w += 1;
            }
        }
        "MONTHLY" | "YEARLY" => {
            let (y0, m0, d0) = civil(day0);
            let step = if freq == "YEARLY" { 12 * interval } else { interval };
            for k in 0..2_000 {
                let months = m0 as i64 - 1 + k * step;
                let (y, m) = (y0 as i64 + months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
                // (A 31st in a shorter month doesn't happen that month.)
                if civil(days_from_civil(y as i32, m, d0)).2 != d0 {
                    continue;
                }
                if !take(to_utc(days_from_civil(y as i32, m, d0)), &mut out) {
                    break;
                }
            }
        }
        _ => {
            take(start, &mut out);
        }
    }
    out
}

/// The date of a day number (days since 1970).
fn civil(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261104T100000Z\r\nDTEND:20261104T103000Z\r\nSUMMARY:Design review\r\nLOCATION:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nDTSTART;TZID=Africa/Cairo:20261102T090000\r\nDTEND;TZID=Africa/Cairo:20261102T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE\r\nSUMMARY:Stand \r\n up\r\nEXDATE;TZID=Africa/Cairo:20261109T090000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:c\r\nDTSTART;VALUE=DATE:20261104\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nRECURRENCE-ID;TZID=Africa/Cairo:20261111T090000\r\nDTSTART;TZID=Africa/Cairo:20261111T110000\r\nDTEND;TZID=Africa/Cairo:20261111T111500\r\nSUMMARY:Stand up (moved)\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn at(y: i32, m: u32, d: u32, h: i64) -> i64 {
        days_from_civil(y, m, d) * 86_400 + h * 3600
    }

    #[test]
    fn reads_meetings_repeats_and_links() {
        // 3 to 12 November: the review, and the stand-ups on Mondays and Wednesdays (Cairo,
        // UTC+2), less the one taken off on the 9th and with the 11th moved to 11:00.
        let got = occurrences(ICS, at(2026, 11, 3, 0), at(2026, 11, 12, 0));
        let list: Vec<(String, i64)> = got.iter().map(|m| (m.title.clone(), m.start)).collect();
        assert_eq!(
            list,
            vec![
                ("Stand up".to_owned(), at(2026, 11, 4, 7)),
                ("Design review".to_owned(), at(2026, 11, 4, 10)),
                ("Stand up (moved)".to_owned(), at(2026, 11, 11, 9)),
            ],
            "{got:#?}"
        );
        assert_eq!(got[1].link.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert!(got[0].link.is_none());
    }

    #[test]
    fn links_and_durations() {
        assert_eq!(normalize("webcal://p01.icloud.com/x.ics").as_deref(), Some("https://p01.icloud.com/x.ics"));
        assert_eq!(normalize("http://x"), None);
        assert_eq!(duration("PT1H30M"), Some(5400));
        assert_eq!(join_link(&["Join: https://us02web.zoom.us/j/123?pwd=x."]).as_deref(), Some("https://us02web.zoom.us/j/123?pwd=x"));
        assert_eq!(civil(days_from_civil(2028, 2, 29)), (2028, 2, 29));
    }
}
