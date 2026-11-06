#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gfx;
mod lyrics;
mod media;
mod meter;
mod mini;
mod claude;
mod claude_app;
mod addrbar;
mod apps;
mod calendar;
mod download;
mod news;
mod prayer;
mod radio;
mod radio_art;
mod reminders_data;
mod shell;
mod sound;
mod store;
mod sysmon;
mod themes;
mod ui;
mod updater;
mod watch;
mod weather;
mod win;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};

use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Shell::*;
use windows::Win32::System::Threading::{
    CreateWaitableTimerExW, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, INFINITE, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use ui::{App, CP_H, CP_W, DW_H, DW_W, OV_W, SW_H, SW_W, WAKE_H, WIN_H, WIN_W};

const WM_TRAY: u32 = WM_APP + 1;
/// Posted by the mouse hook when the cursor nears the notch; it only wakes the loop.
const WM_WAKE: u32 = WM_APP + 2;
/// Posted by the keyboard hook when a screenshot key was pressed and held back.
const WM_SHOT: u32 = WM_APP + 3;
const MENU_START: usize = 1;
const MENU_MINIMAL: usize = 2;
const MENU_EXIT: usize = 3;
const MENU_GLASS: usize = 4;
const MENU_OVERLAY: usize = 5;
const MENU_FULLSCREEN: usize = 6;
const MENU_ADHAN: usize = 7;
const MENU_BELLS: usize = 8;
const MENU_MAX_SHRINK: usize = 9;
const MENU_MAX_KEEP: usize = 10;
const MENU_MAX_HIDE: usize = 11;
const MENU_SHARE: usize = 12;
const MENU_REM_ON: usize = 13;
const MENU_REM_NOW: usize = 14;
const MENU_REM_AYAH: usize = 15;
const MENU_REM_DHIKR: usize = 16;
const MENU_REM_QUIET: usize = 17;
const MENU_REM_DUA: usize = 21;
/// The reminders' interval (one per step of `store::REMIND_EVERY`), time on screen (per
/// `REMIND_FOR`), and quiet hours' start and end (per hour).
const MENU_REM_EVERY: usize = 40;
const MENU_REM_FOR: usize = 60;
const MENU_REM_FROM: usize = 80;
const MENU_REM_TO: usize = 110;
const MENU_VERTICAL: usize = 18;
const MENU_RESET_POS: usize = 19;
/// Hijri date adjustment, -2 to +2 days (MENU_HIJRI + 2 is "none").
const MENU_HIJRI: usize = 30;
const MENU_HANAFI: usize = 35;
const MENU_SETTINGS: usize = 36;
/// The id of the Ctrl+Alt+H hotkey.
const HOTKEY_OPEN: i32 = 1;

static NOTCH_HWND: AtomicIsize = AtomicIsize::new(0);
/// Bottom edge of the area (physical px) where cursor moves should wake the loop.
/// Where the mouse wakes the notch (screen px: left, top, right, bottom), around its window.
static WAKE_ZONE: [AtomicI32; 4] = [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];

pub fn set_wake_zone(r: [i32; 4]) {
    for (z, v) in WAKE_ZONE.iter().zip(r) {
        z.store(v, Ordering::Relaxed);
    }
}
static WAKE_PENDING: AtomicBool = AtomicBool::new(false);
/// The screenshot key currently held back, so its key-up is held back too.
static HELD_KEY: AtomicU32 = AtomicU32::new(0);

/// A low-level mouse hook on its own thread, so the notch reacts the instant the cursor
/// reaches the top of the screen, and the system's mouse never waits on our rendering.
fn start_mouse_hook() {
    std::thread::spawn(|| unsafe {
        let Ok(_hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0) else { return };
        let _keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
    });
}

/// Screenshot keys (PrintScreen in any combination, Win+Shift+S): while our windows are
/// hidden from capture, Windows would paint them black in the picture. Hold the key back
/// and let the app make itself capturable first; it sends the key on a moment later.
unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SNAPSHOT};
    if code >= 0 {
        let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        if k.dwExtraInfo != win::INJECTED {
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            if !down && HELD_KEY.load(Ordering::Relaxed) == k.vkCode {
                HELD_KEY.store(0, Ordering::Relaxed);
                return LRESULT(1);
            }
            if down && win::CAPTURE_HIDDEN.load(Ordering::Relaxed) {
                let held = |v: VIRTUAL_KEY| GetAsyncKeyState(v.0 as i32) < 0;
                let (shift, ctrl, alt, winkey) = (held(VK_SHIFT), held(VK_CONTROL), held(VK_MENU), held(VK_LWIN) || held(VK_RWIN));
                let shot = k.vkCode == VK_SNAPSHOT.0 as u32 || (k.vkCode == 'S' as u32 && winkey && shift);
                if shot {
                    let mods = (shift as u8 * win::MOD_SHIFT) | (ctrl as u8 * win::MOD_CTRL) | (alt as u8 * win::MOD_ALT) | (winkey as u8 * win::MOD_WIN);
                    HELD_KEY.store(k.vkCode, Ordering::Relaxed);
                    let hwnd = HWND(NOTCH_HWND.load(Ordering::Relaxed) as _);
                    let _ = PostMessageW(hwnd, WM_SHOT, WPARAM(k.vkCode as usize), LPARAM(mods as isize));
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let z = WAKE_ZONE.each_ref().map(|v| v.load(Ordering::Relaxed));
        let near = info.pt.x >= z[0] && info.pt.y >= z[1] && info.pt.x <= z[2] && info.pt.y <= z[3];
        if near && !WAKE_PENDING.swap(true, Ordering::AcqRel) {
            let hwnd = HWND(NOTCH_HWND.load(Ordering::Relaxed) as _);
            let _ = PostMessageW(hwnd, WM_WAKE, WPARAM(0), LPARAM(0));
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app unless it's already in use (a modal loop such as a drag or the
/// tray menu re-entering the window procedure), in which case the message falls through.
/// The display's refresh rate (60 if Windows reports none).
fn refresh_rate() -> u32 {
    match win::refresh_hz() {
        hz if hz >= 24 => hz,
        _ => 60,
    }
}

/// Sleeps until `due` (to well under a millisecond with the high-resolution timer), or
/// until input arrives, whichever is first.
unsafe fn wait_until(timer: Option<HANDLE>, due: std::time::Instant) {
    let left = due.saturating_duration_since(std::time::Instant::now());
    if left.is_zero() {
        return;
    }
    match timer {
        Some(t) => {
            // Negative: relative, in 100 ns units.
            let when = -((left.as_nanos() / 100) as i64).max(1);
            if SetWaitableTimer(t, &when, 0, None, None, false).is_ok() {
                MsgWaitForMultipleObjects(Some(&[t]), false, INFINITE, QS_ALLINPUT);
                return;
            }
            MsgWaitForMultipleObjects(None, false, left.as_millis().max(1) as u32, QS_ALLINPUT);
        }
        None => {
            MsgWaitForMultipleObjects(None, false, left.as_millis().max(1) as u32, QS_ALLINPUT);
        }
    }
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn main() -> Result<()> {
    // Hilal playing Quran Radio for itself (see radio::run_apart): no notch, no single-instance.
    if std::env::args().nth(1).as_deref() == Some("--radio-player") {
        return radio::run_apart();
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // One Hilal at a time: a second launch just leaves.
        let _instance = windows::Win32::System::Threading::CreateMutexW(None, true, w!(r"Local\Hilal.SingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        OleInitialize(None)?;
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hIcon: app_icon(false),
            hIconSm: app_icon(true),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("HilalNotch"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("HilalNotch"),
            w!("Hilal"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        )?;
        let scale = GetDpiForWindow(hwnd) as f32 / 96.0;
        let origin = place(hwnd, (WIN_W * scale).round() as i32, (WIN_H * scale).round() as i32);
        let gfx = gfx::Gfx::new(hwnd, WIN_W, WIN_H, scale, (origin.x, origin.y))?;

        // The pinnable FPS overlay: its own small window, hidden until pinned.
        let overlay_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_SIZEALL)?,
            lpszClassName: w!("HilalOverlay"),
            ..Default::default()
        };
        RegisterClassExW(&overlay_class);
        let overlay = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("HilalOverlay"),
            w!("Hilal FPS"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        )?;
        // The Claude session popup: same kind of window, opened from the Claude page.
        let popup_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(popup_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("HilalPopup"),
            ..Default::default()
        };
        RegisterClassExW(&popup_class);
        let popup = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("HilalPopup"),
            w!("Hilal Claude"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        )?;
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let overlay_default = (screen_w - ((OV_W + 24.0) * scale) as i32, (WAKE_H * scale) as i32);
        APP.with(|cell| *cell.borrow_mut() = Some(App::new(hwnd, gfx, origin, overlay, overlay_default, popup)));
        NOTCH_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        start_mouse_hook();
        DragAcceptFiles(hwnd, true);
        // Copies come to the notch (Nova mode's screenshot and smart clipboard toasts).
        let _ = windows::Win32::System::DataExchange::AddClipboardFormatListener(hwnd);
        add_tray(hwnd);
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        with_app(|a| a.apply_position());
        // Ctrl+Alt+H pins the notch open from anywhere (in Nova mode).
        set_hotkey(with_app(|a| a.nova()).unwrap_or(true));
        with_app(|a| a.welcome(false));
        // HILAL_SETTINGS_TEST opens the Settings window at once (for testing).
        if std::env::var_os("HILAL_SETTINGS_TEST").is_some() {
            with_app(|a| a.open_settings());
        }
        // Whenever another window comes to the front, make sure we're still above it.
        let _ = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT);

        // Frame loop: while something moves, every frame is presented with vsync (which
        // also paces the loop); when still, sleep until input or a short poll interval.
        //
        // Not every driver makes that vsync wait (some AMD setups return at once), and then
        // the loop would draw hundreds of frames a second nobody sees, loading the GPU. So
        // a frame that comes much sooner than one refresh after the last waits for its turn.
        let mut msg = MSG::default();
        let pacer = CreateWaitableTimerExW(None, None, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0).ok();
        let mut hz = refresh_rate();
        let mut hz_at = std::time::Instant::now();
        let mut last_start: Option<std::time::Instant> = None;
        let mut held = 0u32;
        // HILAL_NO_PACER=1 turns this off (to compare).
        let pacing = std::env::var_os("HILAL_NO_PACER").is_none();
        'run: loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'run;
                }
                let _ = TranslateMessage(&msg);
                let t = std::time::Instant::now();
                DispatchMessageW(&msg);
                if t.elapsed() > std::time::Duration::from_millis(100) {
                    ui::trace(&format!("[slow message] 0x{:04x} took {} ms", msg.message, t.elapsed().as_millis()));
                }
            }
            WAKE_PENDING.store(false, Ordering::Release);
            if hz_at.elapsed() > std::time::Duration::from_secs(2) {
                hz = refresh_rate();
                hz_at = std::time::Instant::now();
                if held > 0 {
                    ui::trace(&format!("[pacer] held back {held} early frames ({hz} Hz)"));
                    held = 0;
                }
            }
            // Back-to-back frames: more than half a refresh early means the vsync wait
            // didn't happen; hold this one until a whole refresh has passed.
            if let Some(prev) = last_start.filter(|_| pacing) {
                let period = std::time::Duration::from_secs_f64(1.0 / hz as f64);
                // (A little early: waking up runs a fraction of a millisecond late.)
                let due = prev + period.saturating_sub(std::time::Duration::from_micros(400));
                if std::time::Instant::now() < prev + period / 2 {
                    held += 1;
                    wait_until(pacer, due);
                    continue;
                }
            }
            // When still, the mouse hook wakes us instantly when needed; otherwise we
            // check the clock, reminders and the glass backdrop a few times a second.
            let t = std::time::Instant::now();
            let wait = with_app(|a| a.frame()).unwrap_or(120);
            if t.elapsed() > std::time::Duration::from_millis(100) {
                ui::trace(&format!("[slow frame] {} ms", t.elapsed().as_millis()));
            }
            if wait > 0 {
                last_start = None;
                MsgWaitForMultipleObjects(None, false, wait, QS_ALLINPUT);
            } else {
                last_start = Some(t);
            }
        }
        let _ = WTSUnRegisterSessionNotification(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
    }
    Ok(())
}

/// Top-center of the primary monitor, in physical pixels.
fn place(hwnd: HWND, w: i32, h: i32) -> POINT {
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let pos = POINT { x: (screen_w - w) / 2, y: 0 };
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, pos.x, pos.y, w, h, SWP_NOACTIVATE);
    }
    pos
}

/// Hilal's tray icon identity. With a fixed GUID, a new run takes over the icon slot of a
/// run that was killed before it could remove its icon, instead of adding a second one.
const TRAY_GUID: windows::core::GUID = windows::core::GUID::from_u128(0x6c1f1a52_8d3e_4b8e_9a51_2f4f1c7d9e31);
/// Windows binds a tray GUID to one exe path; a copy elsewhere (a dev build) falls back
/// to the classic hwnd + id identity.
static TRAY_USES_GUID: AtomicBool = AtomicBool::new(true);

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let guid = TRAY_USES_GUID.load(Ordering::Relaxed);
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | if guid { NIF_GUID } else { NOTIFY_ICON_DATA_FLAGS(0) },
        uCallbackMessage: WM_TRAY,
        hIcon: app_icon(true),
        guidItem: if guid { TRAY_GUID } else { windows::core::GUID::zeroed() },
        ..Default::default()
    };
    for (dst, src) in data.szTip.iter_mut().zip("Hilal".encode_utf16()) {
        *dst = src;
    }
    data
}

/// The crescent icon embedded in the exe (resource 1), at the system's small or large
/// icon size so it's crisp at any DPI.
fn app_icon(small: bool) -> HICON {
    let (cx, cy) = if small { (SM_CXSMICON, SM_CYSMICON) } else { (SM_CXICON, SM_CYICON) };
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        LoadImageW(instance, PCWSTR(1 as _), IMAGE_ICON, GetSystemMetrics(cx), GetSystemMetrics(cy), LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_default()
    }
}

fn add_tray(hwnd: HWND) {
    unsafe {
        TRAY_USES_GUID.store(true, Ordering::Relaxed);
        // Clear a stale icon left by a run that didn't get to remove its own.
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
        if !Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd)).as_bool() {
            TRAY_USES_GUID.store(false, Ordering::Relaxed);
            let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd));
        }
    }
}

/// Sent to every top-level window when Explorer (and so the tray) starts again.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn tray_menu(hwnd: HWND) {
    let minimal = with_app(|a| a.minimal_setting()).unwrap_or(false);
    let max_mode = with_app(|a| a.maximized_mode()).unwrap_or(1);
    let glass = with_app(|a| a.glass()).unwrap_or(false);
    let share = with_app(|a| a.share_visible()).unwrap_or(false);
    let rem = with_app(|a| a.reminders()).unwrap_or_default();
    let vertical = with_app(|a| a.vertical()).unwrap_or(false);
    let hijri = with_app(|a| a.hijri_adjust()).unwrap_or(0);
    let hanafi = with_app(|a| a.hanafi()).unwrap_or(false);
    let nova = with_app(|a| a.nova()).unwrap_or(true);
    let overlay = with_app(|a| a.overlay_shown()).unwrap_or(false);
    let fullscreen = with_app(|a| a.hide_in_fullscreen()).unwrap_or(true);
    let start = win::starts_with_windows();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS, w!("Settings\u{2026}"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, check(start), MENU_START, w!("Start with Windows"));
        let _ = AppendMenuW(menu, check(glass), MENU_GLASS, w!("Liquid Glass"));
        if nova {
            let _ = AppendMenuW(menu, check(share), MENU_SHARE, w!("Show in screen sharing"));
        }
        let _ = AppendMenuW(menu, check(minimal), MENU_MINIMAL, w!("Minimal notch"));
        let _ = AppendMenuW(menu, check(overlay), MENU_OVERLAY, w!("FPS overlay"));
        // Dhikr, du'a and Qur'an: on or off, how often, for how long, which kinds, quiet hours.
        if let Some(sub) = CreatePopupMenu().ok().filter(|_| nova) {
            let _ = AppendMenuW(sub, check(rem.on), MENU_REM_ON, w!("On"));
            let _ = AppendMenuW(sub, MF_STRING, MENU_REM_NOW, w!("Show one now"));
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, None);
            let steps = |items: &[u32], chosen: u32, first: usize, label: &dyn Fn(u32) -> String| {
                let Ok(m) = CreatePopupMenu() else { return None };
                for (k, &v) in items.iter().enumerate() {
                    let _ = AppendMenuW(m, check(v == chosen), first + k, &HSTRING::from(label(v)));
                }
                Some(m)
            };
            let mins = |v: u32| if v < 60 { format!("{v} min") } else if v % 60 == 0 { format!("{} h", v / 60) } else { format!("{} h {} min", v / 60, v % 60) };
            if let Some(m) = steps(&store::REMIND_EVERY, rem.minutes, MENU_REM_EVERY, &|v| format!("Every {}", mins(v))) {
                let _ = AppendMenuW(sub, MF_POPUP, m.0 as usize, &HSTRING::from(format!("Every {}", mins(rem.minutes))));
            }
            let secs = |v: u32| if v < 60 { format!("{v} s") } else if v % 60 == 0 { format!("{} min", v / 60) } else { format!("{} min {} s", v / 60, v % 60) };
            if let Some(m) = steps(&store::REMIND_FOR, rem.seconds, MENU_REM_FOR, &|v| format!("For {}", secs(v))) {
                let _ = AppendMenuW(sub, MF_POPUP, m.0 as usize, &HSTRING::from(format!("On screen for {}", secs(rem.seconds))));
            }
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(sub, check(rem.ayah), MENU_REM_AYAH, w!("Qur'an verses"));
            let _ = AppendMenuW(sub, check(rem.dhikr), MENU_REM_DHIKR, w!("Dhikr"));
            let _ = AppendMenuW(sub, check(rem.dua), MENU_REM_DUA, w!("Du'a"));
            let _ = AppendMenuW(sub, MF_SEPARATOR, 0, None);
            let hours: Vec<u32> = (0..24).map(|h| h * 60).collect();
            let at = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
            let _ = AppendMenuW(sub, check(rem.quiet_hours), MENU_REM_QUIET, &HSTRING::from(format!("Quiet hours ({} \u{2013} {})", at(rem.quiet_from), at(rem.quiet_to))));
            if rem.quiet_hours {
                if let Some(m) = steps(&hours, rem.quiet_from, MENU_REM_FROM, &|v| at(v)) {
                    let _ = AppendMenuW(sub, MF_POPUP, m.0 as usize, &HSTRING::from(format!("From {}", at(rem.quiet_from))));
                }
                if let Some(m) = steps(&hours, rem.quiet_to, MENU_REM_TO, &|v| at(v)) {
                    let _ = AppendMenuW(sub, MF_POPUP, m.0 as usize, &HSTRING::from(format!("To {}", at(rem.quiet_to))));
                }
            }
            let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Dhikr & Qur'an reminders"));
        }
        // Where it is: drag it anywhere; or stand it on a side edge; or put it back.
        if let Some(sub) = CreatePopupMenu().ok().filter(|_| nova) {
            let _ = AppendMenuW(sub, check(vertical), MENU_VERTICAL, w!("Vertical (dock left / right)"));
            let _ = AppendMenuW(sub, MF_STRING, MENU_RESET_POS, w!("Reset position"));
            let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Position"));
        }
        // Over maximized windows: one of three.
        if let Ok(sub) = CreatePopupMenu() {
            let _ = AppendMenuW(sub, check(max_mode == 0), MENU_MAX_KEEP, w!("Stay as it is"));
            let _ = AppendMenuW(sub, check(max_mode == 1), MENU_MAX_SHRINK, w!("Shrink to a thin line"));
            let _ = AppendMenuW(sub, check(max_mode == 2), MENU_MAX_HIDE, w!("Hide"));
            let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Over maximized windows"));
        }
        let _ = AppendMenuW(menu, check(fullscreen), MENU_FULLSCREEN, w!("Hide in full screen"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        // The Hijri date, moved a day or two for a local moon sighting.
        if let Some(sub) = CreatePopupMenu().ok().filter(|_| nova) {
            for (k, name) in [w!("\u{2212}2 days"), w!("\u{2212}1 day"), w!("None"), w!("+1 day"), w!("+2 days")].into_iter().enumerate() {
                let _ = AppendMenuW(sub, check(hijri == k as i32 - 2), MENU_HIJRI + k, name);
            }
            let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Hijri date adjustment"));
        }
        if nova {
            let _ = AppendMenuW(menu, check(hanafi), MENU_HANAFI, w!("Asr by the Hanafi rule"));
        }
        let adhan = sound::adhan().is_some();
        let _ = AppendMenuW(menu, check(adhan), MENU_ADHAN, w!("Adhan sound\u{2026}"));
        if adhan {
            let _ = AppendMenuW(menu, MF_STRING, MENU_BELLS, w!("Use the bells instead"));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, MENU_EXIT, w!("Exit"));
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        match cmd.0 as usize {
            MENU_START => win::set_start_with_windows(!start),
            MENU_MINIMAL => {
                with_app(|a| a.set_minimal(!minimal));
            }
            MENU_REM_ON => {
                with_app(|a| a.update_reminders(|r| r.on = !r.on));
            }
            MENU_REM_NOW => {
                with_app(|a| a.reminder_now());
            }
            MENU_REM_AYAH => {
                with_app(|a| a.update_reminders(|r| r.ayah = !r.ayah));
            }
            MENU_REM_DHIKR => {
                with_app(|a| a.update_reminders(|r| r.dhikr = !r.dhikr));
            }
            MENU_REM_DUA => {
                with_app(|a| a.update_reminders(|r| r.dua = !r.dua));
            }
            MENU_REM_QUIET => {
                with_app(|a| a.update_reminders(|r| r.quiet_hours = !r.quiet_hours));
            }
            c if (MENU_REM_EVERY..MENU_REM_EVERY + store::REMIND_EVERY.len()).contains(&c) => {
                with_app(|a| a.update_reminders(|r| r.minutes = store::REMIND_EVERY[c - MENU_REM_EVERY]));
            }
            c if (MENU_REM_FOR..MENU_REM_FOR + store::REMIND_FOR.len()).contains(&c) => {
                with_app(|a| a.update_reminders(|r| r.seconds = store::REMIND_FOR[c - MENU_REM_FOR]));
            }
            c if (MENU_REM_FROM..MENU_REM_FROM + 24).contains(&c) => {
                with_app(|a| a.update_reminders(|r| r.quiet_from = (c - MENU_REM_FROM) as u32 * 60));
            }
            c if (MENU_REM_TO..MENU_REM_TO + 24).contains(&c) => {
                with_app(|a| a.update_reminders(|r| r.quiet_to = (c - MENU_REM_TO) as u32 * 60));
            }
            MENU_VERTICAL => {
                with_app(|a| a.set_vertical(!vertical));
            }
            MENU_RESET_POS => {
                with_app(|a| a.reset_position());
            }
            MENU_SHARE => {
                with_app(|a| a.set_share_visible(!share));
            }
            MENU_SETTINGS => {
                with_app(|a| a.open_settings());
            }
            MENU_HANAFI => {
                with_app(|a| a.set_hanafi(!hanafi));
            }
            c @ MENU_HIJRI..=34 => {
                with_app(|a| a.set_hijri_adjust(c as i32 - MENU_HIJRI as i32 - 2));
            }
            MENU_GLASS => {
                with_app(|a| a.set_glass(!glass));
            }
            MENU_OVERLAY => {
                with_app(|a| a.set_overlay(!overlay));
            }
            MENU_MAX_KEEP => {
                with_app(|a| a.set_maximized_mode(0));
            }
            MENU_MAX_SHRINK => {
                with_app(|a| a.set_maximized_mode(1));
            }
            MENU_MAX_HIDE => {
                with_app(|a| a.set_maximized_mode(2));
            }
            MENU_FULLSCREEN => {
                with_app(|a| a.set_hide_in_fullscreen(!fullscreen));
            }
            MENU_ADHAN => {
                // The chosen recording is copied in, then played once to hear it.
                if let Some(path) = pick_audio(hwnd) {
                    if sound::set_adhan(Some(&path)).is_ok() {
                        sound::play_prayer();
                    }
                }
            }
            MENU_BELLS => {
                sound::stop();
                let _ = sound::set_adhan(None);
            }
            MENU_EXIT => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

/// Asks for an audio file with the standard Open dialog.
fn pick_audio(hwnd: HWND) -> Option<std::path::PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let mut buf = vec![0u16; 2048];
    let filter: Vec<u16> = format!("Audio\0{}\0\0", sound::ADHAN_TYPES.map(|e| format!("*.{e}")).join(";")).encode_utf16().collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: windows::core::PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrTitle: w!("Choose the adhan sound"),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).into())
}

fn dp(hwnd: HWND, lparam: LPARAM) -> (f32, f32) {
    let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let x = (lparam.0 & 0xffff) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
    (x / scale, y / scale)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) {
        add_tray(hwnd);
        return LRESULT(0);
    }
    let handled = match msg {
        WM_WAKE => Some(()),
        0x031D /* WM_CLIPBOARDUPDATE */ => with_app(|a| a.clipboard_changed()),
        WM_HOTKEY if wparam.0 as i32 == HOTKEY_OPEN => with_app(|a| a.hotkey()),
        WM_SETTINGCHANGE => {
            win::refresh_time_format();
            None
        }
        WM_SHOT => {
            let (vk, mods) = (wparam.0 as u16, lparam.0 as u8);
            // If the app is busy (a modal loop), don't lose the key: send it on as is.
            if with_app(|a| a.begin_screenshot(vk, mods)).is_none() {
                win::replay_keys(vk, mods);
            }
            Some(())
        }
        WM_WTSSESSION_CHANGE if wparam.0 as u32 == WTS_SESSION_UNLOCK => with_app(|a| a.welcome(true)),
        WM_LBUTTONDOWN => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.mouse_down(x, y))
        }
        WM_MOUSEMOVE => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.mouse_move(x, y))
        }
        WM_LBUTTONUP => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.mouse_up(x, y))
        }
        WM_RBUTTONUP => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.mouse_right(x, y))
        }
        WM_MOUSEWHEEL => with_app(|a| a.wheel(((wparam.0 >> 16) & 0xffff) as i16 as i32)),
        WM_KEYDOWN => with_app(|a| a.key(wparam.0 as u16)),
        WM_CHAR => char::from_u32(wparam.0 as u32).and_then(|c| with_app(|a| a.char(c))),
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => with_app(|a| a.deactivated()),
        WM_DROPFILES => {
            let drop = HDROP(wparam.0 as _);
            let mut paths = Vec::new();
            unsafe {
                for i in 0..DragQueryFileW(drop, u32::MAX, None) {
                    let len = DragQueryFileW(drop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    DragQueryFileW(drop, i, Some(&mut buf));
                    paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
                }
                DragFinish(drop);
            }
            with_app(|a| a.drop_files(paths))
        }
        WM_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU || event == WM_LBUTTONUP {
                tray_menu(hwnd);
            }
            Some(())
        }
        WM_DPICHANGED | WM_DISPLAYCHANGE => with_app(|a| {
            let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
            a.origin = place(hwnd, (WIN_W * scale).round() as i32, (WIN_H * scale).round() as i32);
            if a.gfx.resize(gfx::MAIN, WIN_W, WIN_H, scale, (a.origin.x, a.origin.y)).is_ok() {
                a.draw();
            }
            // (back to the chosen spot along the top)
            a.apply_position();
        }),
        WM_DESTROY => {
            // Tear down while the window still exists: the tray icon, then the app with its
            // GPU and composition objects. Releasing those after the window (or the whole
            // program) is gone makes Windows report a crash on exit.
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
            }
            // An update downloaded but never accepted is installed as Hilal quits.
            if let Some(path) = updater::PENDING.lock().ok().and_then(|mut p| p.take()) {
                updater::install(&path, false);
            }
            let app = APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut a| a.take()));
            drop(app);
            unsafe { PostQuitMessage(0) };
            Some(())
        }
        _ => None,
    };
    if handled.is_some() && msg != WM_ACTIVATE {
        return LRESULT(0);
    }
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The FPS overlay: drag it from anywhere (the system moves the window for us), close it
/// with the button that shows on hover.
extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let screen_pt = || ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
    match msg {
        WM_NCHITTEST => {
            let (x, y) = screen_pt();
            let on_close = with_app(|a| a.overlay_on_close(x, y)).unwrap_or(false);
            return LRESULT(if on_close { HTCLIENT } else { HTCAPTION } as isize);
        }
        WM_LBUTTONUP => {
            // Only a release on the close button closes it (a drag also ends with one).
            let mut p = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut p);
            }
            with_app(|a| {
                if a.overlay_on_close(p.x, p.y) {
                    a.set_overlay(false);
                }
            });
            return LRESULT(0);
        }
        WM_EXITSIZEMOVE => {
            with_app(|a| a.overlay_moved());
            return LRESULT(0);
        }
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The Claude session popup: drag it by its header, close it with its button.
/// Holds Ctrl+Alt+H for Hilal (Nova mode), or lets it go (Classic).
pub fn set_hotkey(on: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
    let hwnd = HWND(NOTCH_HWND.load(Ordering::Relaxed) as _);
    unsafe {
        let _ = UnregisterHotKey(hwnd, HOTKEY_OPEN);
        if on && RegisterHotKey(hwnd, HOTKEY_OPEN, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'H' as u32).is_err() {
            ui::trace("[hotkey] Ctrl+Alt+H is taken by another app");
        }
    }
}

/// Creates the prayer city's drop-down (hidden; the app places, fills and shows it).
pub fn create_city_window() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(city_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            lpszClassName: w!("HilalCities"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("HilalCities"),
            w!("Hilal Cities"),
            WS_POPUP,
            0,
            0,
            CP_W as i32,
            CP_H as i32,
            None,
            None,
            instance,
            None,
        )
        .ok()
    }
}

extern "system" fn city_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    const WM_MOUSELEAVE: u32 = 0x02A3;
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = dp(hwnd, lparam);
            let mut track = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut track);
            }
            with_app(|a| a.city_hover(x, y));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            with_app(|a| a.city_hover(-1.0, -1.0));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.city_click(x, y));
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            with_app(|a| a.city_wheel(((wparam.0 >> 16) & 0xffff) as i16 as i32));
            return LRESULT(0);
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wparam.0 as u32) {
                with_app(|a| a.city_char(c));
            }
            return LRESULT(0);
        }
        // A click anywhere else closes it.
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|a| a.close_city_picker());
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Creates the download options window (hidden; the app places, fills and shows it).
pub fn create_dl_window() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(dl_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            lpszClassName: w!("HilalDownload"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("HilalDownload"),
            w!("Hilal Download"),
            WS_POPUP,
            0,
            0,
            DW_W as i32,
            DW_H as i32,
            None,
            None,
            instance,
            None,
        )
        .ok()
    }
}

extern "system" fn dl_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    const WM_MOUSELEAVE: u32 = 0x02A3;
    match msg {
        WM_NCHITTEST => {
            let mut p = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
            }
            let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
            let drag = with_app(|a| a.dl_drags(p.x as f32 / scale, p.y as f32 / scale)).unwrap_or(false);
            return LRESULT(if drag { HTCAPTION } else { HTCLIENT } as isize);
        }
        WM_MOUSEMOVE => {
            let (x, y) = dp(hwnd, lparam);
            let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut track);
            }
            with_app(|a| a.dl_hover(x, y));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            with_app(|a| a.dl_hover(-1.0, -1.0));
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.dl_down(x, y));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = dp(hwnd, lparam);
            // A drop-down's menu runs its own message loop: not while the app is borrowed.
            if let Some(Some(ui::DlAfter::Menu(which))) = with_app(|a| a.dl_up(x, y)) {
                let items = with_app(|a| a.dl_menu(which)).unwrap_or_default();
                unsafe {
                    if let Ok(menu) = CreatePopupMenu() {
                        for (i, (label, on)) in items.iter().enumerate() {
                            let flags = if *on { MF_STRING | MF_CHECKED } else { MF_STRING };
                            let _ = AppendMenuW(menu, flags, i + 1, &HSTRING::from(label.as_str()));
                        }
                        let mut p = POINT::default();
                        let _ = GetCursorPos(&mut p);
                        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTBUTTON, p.x, p.y, 0, hwnd, None);
                        let _ = DestroyMenu(menu);
                        if cmd.0 > 0 {
                            with_app(|a| a.dl_pick(which, cmd.0 as usize - 1));
                        }
                    }
                }
            }
            return LRESULT(0);
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wparam.0 as u32) {
                with_app(|a| a.dl_char(c));
            }
            return LRESULT(0);
        }
        WM_EXITSIZEMOVE => {
            with_app(|a| a.dl_moved());
            return LRESULT(0);
        }
        WM_CLOSE => {
            with_app(|a| a.close_dl());
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Creates the Settings window (hidden; the app sizes, fills and shows it).
pub fn create_settings_window() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(settings_proc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            hIcon: app_icon(false),
            hIconSm: app_icon(true),
            lpszClassName: w!("HilalSettings"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_APPWINDOW,
            w!("HilalSettings"),
            w!("Hilal Settings"),
            WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
            0,
            0,
            (SW_W) as i32,
            (SW_H) as i32,
            None,
            None,
            instance,
            None,
        )
        .ok()
    }
}

extern "system" fn settings_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    const WM_MOUSELEAVE: u32 = 0x02A3;
    match msg {
        WM_NCHITTEST => {
            let mut p = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
            }
            let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
            let drag = with_app(|a| a.settings_drags(p.x as f32 / scale, p.y as f32 / scale)).unwrap_or(false);
            return LRESULT(if drag { HTCAPTION } else { HTCLIENT } as isize);
        }
        WM_MOUSEMOVE => {
            let (x, y) = dp(hwnd, lparam);
            let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            unsafe {
                let _ = TrackMouseEvent(&mut track);
            }
            with_app(|a| a.settings_hover(x, y));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            with_app(|a| a.settings_hover(-1.0, -1.0));
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.settings_down(x, y));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = dp(hwnd, lparam);
            // The adhan's file dialog runs its own message loop: not while the app is borrowed.
            if let Some(Some(ui::SettingsAfter::PickAdhan)) = with_app(|a| a.settings_up(x, y)) {
                if let Some(path) = pick_audio(hwnd) {
                    if sound::set_adhan(Some(&path)).is_ok() {
                        sound::play_prayer();
                    }
                }
                with_app(|a| a.draw_settings());
            }
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            with_app(|a| a.settings_wheel(((wparam.0 >> 16) & 0xffff) as i16 as i32));
            return LRESULT(0);
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wparam.0 as u32) {
                let typed = with_app(|a| a.settings_char(c)).unwrap_or(false);
                if !typed && c == '\u{1b}' {
                    with_app(|a| a.close_settings());
                }
            }
            return LRESULT(0);
        }
        WM_EXITSIZEMOVE => {
            with_app(|a| a.settings_moved());
            return LRESULT(0);
        }
        WM_CLOSE => {
            with_app(|a| a.close_settings());
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

extern "system" fn popup_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let (sx, sy) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
            let drag = with_app(|a| a.popup_drags(sx, sy)).unwrap_or(false);
            return LRESULT(if drag { HTCAPTION } else { HTCLIENT } as isize);
        }
        WM_LBUTTONUP => {
            let (x, y) = dp(hwnd, lparam);
            with_app(|a| a.popup_click(x, y));
            return LRESULT(0);
        }
        WM_EXITSIZEMOVE => {
            with_app(|a| a.popup_moved());
            return LRESULT(0);
        }
        // Typing a message needs the keyboard; otherwise it never takes focus.
        WM_MOUSEACTIVATE => {
            let typing = with_app(|a| a.popup_typing()).unwrap_or(false);
            return LRESULT(if typing { MA_ACTIVATE } else { MA_NOACTIVATE } as isize);
        }
        WM_KEYDOWN => {
            with_app(|a| a.popup_key(wparam.0 as u16));
            return LRESULT(0);
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wparam.0 as u32) {
                with_app(|a| a.popup_char(c));
            }
            return LRESULT(0);
        }
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            with_app(|a| a.popup_focus(false));
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Foreground-window changes (delivered on our thread through the message loop).
unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    with_app(|a| a.keep_on_top());
}
