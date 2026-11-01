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

/// Local time as "HH:MM".
pub fn clock() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

/// Local date as "Sat 31 Oct 2026".
pub fn date_long() -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let t = unsafe { GetLocalTime() };
    format!("{} {} {} {}", DAYS[t.wDayOfWeek as usize % 7], t.wDay, MONTHS[(t.wMonth as usize + 11) % 12], t.wYear)
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

/// Hides a window from screenshots, recordings and desktop duplication (it still shows
/// on the monitor). Glass windows need this so they don't capture themselves.
pub fn exclude_from_capture(hwnd: windows::Win32::Foundation::HWND, on: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
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
