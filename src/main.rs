#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gfx;
mod lyrics;
mod media;
mod meter;
mod claude;
mod claude_app;
mod download;
mod prayer;
mod shell;
mod sound;
mod store;
mod sysmon;
mod ui;
mod win;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering};

use windows::core::{w, Result, PCWSTR};
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

use ui::{App, OV_W, WIN_H, WIN_W};

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

static NOTCH_HWND: AtomicIsize = AtomicIsize::new(0);
/// Bottom edge of the area (physical px) where cursor moves should wake the loop.
static WAKE_ZONE: AtomicI32 = AtomicI32::new(0);
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
        if info.pt.y <= WAKE_ZONE.load(Ordering::Relaxed) && !WAKE_PENDING.swap(true, Ordering::AcqRel) {
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
        let overlay_default = (screen_w - ((OV_W + 24.0) * scale) as i32, (WIN_H * scale) as i32);
        APP.with(|cell| *cell.borrow_mut() = Some(App::new(hwnd, gfx, origin, overlay, overlay_default, popup)));
        NOTCH_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        WAKE_ZONE.store((WIN_H * scale) as i32 + 40, Ordering::Relaxed);
        start_mouse_hook();
        DragAcceptFiles(hwnd, true);
        add_tray(hwnd);
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        with_app(|a| a.welcome(false));
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
    let overlay = with_app(|a| a.overlay_shown()).unwrap_or(false);
    let fullscreen = with_app(|a| a.hide_in_fullscreen()).unwrap_or(true);
    let start = win::starts_with_windows();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(menu, check(start), MENU_START, w!("Start with Windows"));
        let _ = AppendMenuW(menu, check(glass), MENU_GLASS, w!("Liquid Glass"));
        let _ = AppendMenuW(menu, check(minimal), MENU_MINIMAL, w!("Minimal notch"));
        let _ = AppendMenuW(menu, check(overlay), MENU_OVERLAY, w!("FPS overlay"));
        // Over maximized windows: one of three.
        if let Ok(sub) = CreatePopupMenu() {
            let _ = AppendMenuW(sub, check(max_mode == 0), MENU_MAX_KEEP, w!("Stay as it is"));
            let _ = AppendMenuW(sub, check(max_mode == 1), MENU_MAX_SHRINK, w!("Shrink to a thin line"));
            let _ = AppendMenuW(sub, check(max_mode == 2), MENU_MAX_HIDE, w!("Hide"));
            let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Over maximized windows"));
        }
        let _ = AppendMenuW(menu, check(fullscreen), MENU_FULLSCREEN, w!("Hide in full screen"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
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
        }),
        WM_DESTROY => {
            // Tear down while the window still exists: the tray icon, then the app with its
            // GPU and composition objects. Releasing those after the window (or the whole
            // program) is gone makes Windows report a crash on exit.
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
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
