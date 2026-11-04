//! Small Win32 helpers.

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTimeAsFileTime};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Global cursor position in physical pixels.
pub fn cursor_px() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x, p.y))
}

/// Local date as "YYYY-MM-DD", used for daily resets.
pub fn today() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// 0 = not read yet, 1 = 12-hour, 2 = 24-hour.
static HOUR_FORMAT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Whether Windows is set to a 12-hour clock (Settings > Time & language > Region).
pub fn uses_12h() -> bool {
    use std::sync::atomic::Ordering;
    match HOUR_FORMAT.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_ITIME};
            let mut buf = [0u16; 4];
            let n = unsafe { GetLocaleInfoEx(windows::core::PCWSTR::null(), LOCALE_ITIME, Some(&mut buf)) };
            let twelve = n > 0 && buf[0] == '0' as u16;
            HOUR_FORMAT.store(if twelve { 1 } else { 2 }, Ordering::Relaxed);
            twelve
        }
    }
}

/// The user changed regional settings: read the clock format again.
pub fn refresh_time_format() {
    HOUR_FORMAT.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// A time of day in the user's format, split so the AM/PM can be drawn smaller:
/// ("3:31", Some("PM")) on a 12-hour clock, ("15:31", None) on a 24-hour one.
pub fn hm(hour: u16, minute: u16) -> (String, Option<&'static str>) {
    if uses_12h() {
        let h = match hour % 12 {
            0 => 12,
            h => h,
        };
        (format!("{h}:{minute:02}"), Some(if hour < 12 { "AM" } else { "PM" }))
    } else {
        (format!("{hour:02}:{minute:02}"), None)
    }
}

/// `hm` as one string: "3:31 PM" or "15:31".
pub fn hm_text(hour: u16, minute: u16) -> String {
    match hm(hour, minute) {
        (t, Some(ampm)) => format!("{t} {ampm}"),
        (t, None) => t,
    }
}

/// The current local time, split like `hm`.
pub fn clock_parts() -> (String, Option<&'static str>) {
    let t = unsafe { GetLocalTime() };
    hm(t.wHour, t.wMinute)
}

/// The current local time in the user's format, e.g. "3:31 PM".
pub fn clock() -> String {
    let t = unsafe { GetLocalTime() };
    hm_text(t.wHour, t.wMinute)
}

/// Local date as "Sun, Nov 1".
pub fn date_long() -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let t = unsafe { GetLocalTime() };
    format!("{}, {} {}", DAYS[t.wDayOfWeek as usize % 7], MONTHS[(t.wMonth as usize + 11) % 12], t.wDay)
}

/// Seconds since local midnight.
pub fn seconds_of_day() -> u32 {
    let t = unsafe { GetLocalTime() };
    t.wHour as u32 * 3600 + t.wMinute as u32 * 60 + t.wSecond as u32
}

/// Today's day of the week (0 Sunday).
pub fn weekday() -> u16 {
    unsafe { GetLocalTime() }.wDayOfWeek
}

pub fn hour() -> u16 {
    unsafe { GetLocalTime() }.wHour
}

pub fn minute() -> u16 {
    unsafe { GetLocalTime() }.wMinute
}

/// Now as a Windows FILETIME (100 ns ticks since 1601), the unit WinRT timestamps use.
pub fn filetime_now() -> i64 {
    let ft = unsafe { GetSystemTimeAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

/// Battery percent and whether it's on mains power. A desktop (no battery) reads as
/// full and plugged in.
pub fn battery() -> Option<(u8, bool)> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    let plugged = s.ACLineStatus == 1;
    if s.BatteryFlag == 128 || s.BatteryLifePercent > 100 {
        return Some((100, plugged));
    }
    Some((s.BatteryLifePercent, plugged))
}

/// Whether our windows are currently hidden from capture; the keyboard hook only steps
/// in on screenshot keys while they are.
pub static CAPTURE_HIDDEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Hides a window from screenshots, recordings and desktop duplication (it still shows
/// on the monitor). Glass windows need this so they don't capture themselves.
pub fn exclude_from_capture(hwnd: windows::Win32::Foundation::HWND, on: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};
    CAPTURE_HIDDEN.store(on, std::sync::atomic::Ordering::Relaxed);
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
    }
}

/// A screen recorder or streaming app is running (OBS, Streamlabs). Those are only open to
/// record, unlike Discord or NVIDIA's overlay, which run all the time and are left out.
pub fn recorder_running() -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    const RECORDERS: [&str; 5] = ["obs64.exe", "obs32.exe", "obs.exe", "streamlabs obs.exe", "streamlabs desktop.exe"];
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return false };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut found = false;
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more && !found {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..len]).to_ascii_lowercase();
            found = RECORDERS.contains(&name.as_str());
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
        found
    }
}

/// Marks key events we inject ourselves, so our keyboard hook lets them through.
pub const INJECTED: usize = 0x4849_4C41; // "HILA"

/// Modifier bits for `replay_keys`.
pub const MOD_SHIFT: u8 = 1;
pub const MOD_CTRL: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_WIN: u8 = 8;

/// Presses `vk` with the given modifiers, as if typed: modifiers that are still held are
/// left alone, the others are pressed around the key.
pub fn replay_keys(vk: u16, mods: u8) {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let held = |k: VIRTUAL_KEY| unsafe { GetAsyncKeyState(k.0 as i32) } < 0;
    // (Spelled out: this module's glob import has its own MOD_* hotkey constants.)
    let wanted = [(crate::win::MOD_SHIFT, VK_SHIFT), (crate::win::MOD_CTRL, VK_CONTROL), (crate::win::MOD_ALT, VK_MENU), (crate::win::MOD_WIN, VK_LWIN)];
    let press: Vec<VIRTUAL_KEY> = wanted
        .iter()
        .filter(|(bit, k)| mods & bit != 0 && !(held(*k) || (*k == VK_LWIN && held(VK_RWIN))))
        .map(|(_, k)| *k)
        .collect();
    let input = |k: VIRTUAL_KEY, up: bool| {
        let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        if k == VK_SNAPSHOT || k == VK_LWIN {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: k, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: INJECTED } },
        }
    };
    let key = VIRTUAL_KEY(vk);
    let mut seq: Vec<INPUT> = press.iter().map(|k| input(*k, false)).collect();
    seq.push(input(key, false));
    seq.push(input(key, true));
    seq.extend(press.iter().rev().map(|k| input(*k, true)));
    unsafe {
        SendInput(&seq, std::mem::size_of::<INPUT>() as i32);
    }
}

/// The primary display's refresh rate in Hz.
pub fn refresh_hz() -> u32 {
    use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
    let mut mode = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
    unsafe {
        if EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut mode).as_bool() {
            return mode.dmDisplayFrequency;
        }
    }
    60
}

const RUN_KEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

pub fn starts_with_windows() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Hilal"), RRF_RT_REG_SZ, None, None, None).is_ok() }
}

pub fn set_start_with_windows(on: bool) {
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let value: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                &HSTRING::from("Hilal"),
                REG_SZ.0,
                Some(value.as_ptr() as _),
                (value.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Hilal"));
        }
    }
}

/// Pictures (the user's), where screenshots are saved.
pub fn known_folder_pictures() -> Option<std::path::PathBuf> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let p = unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, HANDLE::default()) }.ok()?;
    unsafe { p.to_string() }.ok().map(std::path::PathBuf::from)
}

/// Opens a web link in the default browser.
pub fn open_url(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    if url.starts_with("https://") || url.starts_with("http://") {
        unsafe {
            ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
        }
    }
}
