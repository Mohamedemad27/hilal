#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gfx;
mod lyrics;
mod media;
mod shell;
mod store;
mod sysmon;
mod ui;
mod win;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering};

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use ui::{App, OV_W, WIN_H, WIN_W};

const WM_TRAY: u32 = WM_APP + 1;
/// Posted by the mouse hook when the cursor nears the notch; it only wakes the loop.
const WM_WAKE: u32 = WM_APP + 2;
const MENU_START: usize = 1;
const MENU_MINIMAL: usize = 2;
const MENU_EXIT: usize = 3;
const MENU_GLASS: usize = 4;
const MENU_OVERLAY: usize = 5;
const MENU_FULLSCREEN: usize = 6;

static NOTCH_HWND: AtomicIsize = AtomicIsize::new(0);
/// Bottom edge of the area (physical px) where cursor moves should wake the loop.
static WAKE_ZONE: AtomicI32 = AtomicI32::new(0);
static WAKE_PENDING: AtomicBool = AtomicBool::new(false);

/// A low-level mouse hook on its own thread, so the notch reacts the instant the cursor
/// reaches the top of the screen, and the system's mouse never waits on our rendering.
fn start_mouse_hook() {
    std::thread::spawn(|| unsafe {
        let Ok(_hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0) else { return };
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
    });
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
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
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
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let overlay_default = (screen_w - ((OV_W + 24.0) * scale) as i32, (WIN_H * scale) as i32);
        APP.with(|cell| *cell.borrow_mut() = Some(App::new(hwnd, gfx, origin, overlay, overlay_default)));
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
        let mut msg = MSG::default();
        'run: loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'run;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            WAKE_PENDING.store(false, Ordering::Release);
            // When still, the mouse hook wakes us instantly when needed; otherwise we
            // check the clock, reminders and the glass backdrop a few times a second.
            let wait = with_app(|a| a.frame()).unwrap_or(120);
            if wait > 0 {
                MsgWaitForMultipleObjects(None, false, wait, QS_ALLINPUT);
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

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: app_icon(true),
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
        let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd));
    }
}

fn tray_menu(hwnd: HWND) {
    let minimal = with_app(|a| a.minimal()).unwrap_or(false);
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
        let _ = AppendMenuW(menu, check(fullscreen), MENU_FULLSCREEN, w!("Hide in full screen"));
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
            MENU_FULLSCREEN => {
                with_app(|a| a.set_hide_in_fullscreen(!fullscreen));
            }
            MENU_EXIT => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

fn dp(hwnd: HWND, lparam: LPARAM) -> (f32, f32) {
    let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let x = (lparam.0 & 0xffff) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
    (x / scale, y / scale)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let handled = match msg {
        WM_WAKE => Some(()),
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

/// Foreground-window changes (delivered on our thread through the message loop).
unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    with_app(|a| a.keep_on_top());
}
