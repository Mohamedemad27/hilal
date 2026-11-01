#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Hilal's installer: a single exe carrying hilal.exe. Run normally it installs or
//! updates; run with /uninstall (what Settings > Apps does) it removes Hilal.

#[allow(dead_code)]
#[path = "../../src/gfx.rs"]
mod gfx;
mod ops;

use std::cell::{Cell, RefCell};
use std::time::Instant;

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

use gfx::{rgb, rgba, white, Align, Color, Gfx, Image, Rect, WHITE};

const CARD_W: f32 = 600.0;
const CARD_H: f32 = 400.0;
/// Room around the card for its shadow.
const M: f32 = 28.0;
const WIN_W: f32 = CARD_W + 2.0 * M;
const WIN_H: f32 = CARD_H + 2.0 * M;

const GOLD_LIGHT: Color = rgb(255, 231, 160);
const GOLD_DEEP: Color = rgb(255, 170, 64);
const RED_LIGHT: Color = rgb(255, 128, 120);
const RED_DEEP: Color = rgb(222, 58, 74);
const INK: Color = rgb(30, 20, 6);
const LOGO: &[u8] = include_bytes!("../../assets/logo-512.png");

#[derive(Clone, PartialEq)]
enum Screen {
    Welcome,
    Working { removing: bool },
    Installed,
    ConfirmRemove,
    Removed,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Close,
    Install,
    Launch,
    AskRemove,
    Remove,
    Cancel,
    ToggleStart,
    ToggleDesktop,
    ToggleData,
}

struct Setup {
    hwnd: HWND,
    gfx: Gfx,
    logo: Option<Image>,
    screen: Screen,
    screen_at: Instant,
    opts: ops::Options,
    delete_data: bool,
    installed: Option<String>,
    uninstall_mode: bool,
    job: Option<(ops::Shared, Instant)>,
    regions: RefCell<Vec<(Rect, Act)>>,
    /// Cursor in card coordinates (dp).
    cursor: (f32, f32),
    pressed: Option<Act>,
    t0: Instant,
    bar: Cell<f32>,
    last: Cell<Instant>,
}

thread_local! {
    static SETUP: RefCell<Option<Setup>> = const { RefCell::new(None) };
}

fn with_setup<R>(f: impl FnOnce(&mut Setup) -> R) -> Option<R> {
    SETUP.with(|c| c.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let icon = |small: bool| {
            let (cx, cy) = if small { (SM_CXSMICON, SM_CYSMICON) } else { (SM_CXICON, SM_CYICON) };
            LoadImageW(instance, PCWSTR(1 as _), IMAGE_ICON, GetSystemMetrics(cx), GetSystemMetrics(cy), LR_DEFAULTCOLOR)
                .map(|h| HICON(h.0))
                .unwrap_or_default()
        };
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hIcon: icon(false),
            hIconSm: icon(true),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("HilalSetup"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_APPWINDOW,
            w!("HilalSetup"),
            w!("Hilal Setup"),
            WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
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
        let (w, h) = ((WIN_W * scale) as i32, (WIN_H * scale) as i32);
        let (x, y) = ((GetSystemMetrics(SM_CXSCREEN) - w) / 2, (GetSystemMetrics(SM_CYSCREEN) - h) / 2);
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
        let gfx = Gfx::new(hwnd, WIN_W, WIN_H, scale, (x, y))?;
        let uninstall_mode = std::env::args().any(|a| a.eq_ignore_ascii_case("/uninstall"));
        SETUP.with(|c| *c.borrow_mut() = Some(Setup::new(hwnd, gfx, uninstall_mode)));
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        let mut msg = MSG::default();
        'run: loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'run;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if IsIconic(hwnd).as_bool() {
                MsgWaitForMultipleObjects(None, false, 100, QS_ALLINPUT);
                continue;
            }
            // Presenting with vsync paces this loop to the display.
            with_setup(|s| s.frame());
        }
    }
    Ok(())
}

impl Setup {
    fn new(hwnd: HWND, gfx: Gfx, uninstall_mode: bool) -> Self {
        let logo = image::load_from_memory(LOGO).ok().and_then(|img| {
            let rgba = img.resize(256, 256, image::imageops::FilterType::Lanczos3).to_rgba8();
            let (w, h) = rgba.dimensions();
            let mut bgra = Vec::with_capacity((w * h * 4) as usize);
            for p in rgba.pixels() {
                let [r, g, b, a] = p.0;
                let k = a as f32 / 255.0;
                bgra.extend_from_slice(&[(b as f32 * k) as u8, (g as f32 * k) as u8, (r as f32 * k) as u8, a]);
            }
            gfx.image(&bgra, w, h)
        });
        let installed = ops::installed_version();
        let now = Instant::now();
        Self {
            hwnd,
            gfx,
            logo,
            screen: if uninstall_mode { Screen::ConfirmRemove } else { Screen::Welcome },
            screen_at: now,
            opts: ops::Options { start_with_windows: true, desktop_shortcut: false },
            delete_data: false,
            installed,
            uninstall_mode,
            job: None,
            regions: RefCell::new(Vec::new()),
            cursor: (-1.0, -1.0),
            pressed: None,
            t0: now,
            bar: Cell::new(0.0),
            last: Cell::new(now),
        }
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.screen_at = Instant::now();
    }

    fn frame(&mut self) {
        // Move on once the work is done and the bar has visibly reached the end.
        if let Some((job, started)) = &self.job {
            let j = job.lock().unwrap();
            let removing = matches!(self.screen, Screen::Working { removing: true });
            match &j.result {
                Some(Ok(())) if self.bar.get() > 0.97 && started.elapsed().as_secs_f32() > 1.1 => {
                    drop(j);
                    self.job = None;
                    self.installed = ops::installed_version();
                    self.go(if removing { Screen::Removed } else { Screen::Installed });
                }
                Some(Err(e)) => {
                    let e = e.clone();
                    drop(j);
                    self.job = None;
                    self.go(Screen::Failed(e));
                }
                _ => {}
            }
        }
        let mut p = POINT::default();
        let mut r = RECT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
            let _ = GetWindowRect(self.hwnd, &mut r);
        }
        let k = self.gfx.scale;
        self.cursor = ((p.x - r.left) as f32 / k - M, (p.y - r.top) as f32 / k - M);
        self.draw();
    }

    fn perform(&mut self, a: Act) {
        match a {
            Act::Close => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            Act::Install => {
                let job = ops::Shared::default();
                ops::install(self.opts, job.clone());
                self.job = Some((job, Instant::now()));
                self.bar.set(0.0);
                self.go(Screen::Working { removing: false });
            }
            Act::Launch => {
                ops::launch();
                unsafe {
                    let _ = DestroyWindow(self.hwnd);
                }
            }
            Act::AskRemove => self.go(Screen::ConfirmRemove),
            Act::Remove => {
                let job = ops::Shared::default();
                ops::uninstall(self.delete_data, job.clone());
                self.job = Some((job, Instant::now()));
                self.bar.set(0.0);
                self.go(Screen::Working { removing: true });
            }
            Act::Cancel if self.uninstall_mode => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            Act::Cancel => self.go(Screen::Welcome),
            Act::ToggleStart => self.opts.start_with_windows = !self.opts.start_with_windows,
            Act::ToggleDesktop => self.opts.desktop_shortcut = !self.opts.desktop_shortcut,
            Act::ToggleData => self.delete_data = !self.delete_data,
        }
    }

    fn region_at(&self, x: f32, y: f32) -> Option<Act> {
        self.regions.borrow().iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| *a)
    }

    fn hit(&self, r: Rect, a: Act) {
        if self.gfx.fade.get() > 0.5 {
            self.regions.borrow_mut().push((r, a));
        }
    }

    fn hovered(&self, r: Rect) -> bool {
        r.contains(self.cursor.0, self.cursor.1)
    }

    // ----- drawing ----------------------------------------------------------------------

    fn draw(&mut self) {
        if self.gfx.begin(gfx::MAIN).is_err() {
            return;
        }
        self.regions.borrow_mut().clear();
        let t = self.t0.elapsed().as_secs_f32();
        let dt = self.last.get().elapsed().as_secs_f32().min(0.05);
        self.last.set(Instant::now());
        let g = &self.gfx;
        g.translate(M, M);

        // A soft shadow under the card.
        for i in 0..10 {
            let d = i as f32 * 2.2;
            g.fill_rrect(Rect::new(-d, -d + 8.0, CARD_W + 2.0 * d, CARD_H + 2.0 * d), 24.0 + d, rgba(0, 0, 0, 0.045));
        }

        // The night sky: midnight gradient, a cool glow, a warm glow behind the logo, stars.
        let card = Rect::new(0.0, 0.0, CARD_W, CARD_H);
        g.linear(card, 24.0, &[(0.0, rgb(22, 28, 66)), (1.0, rgb(7, 9, 26))], true);
        g.clip(card);
        g.radial(170.0, 110.0, 190.0, 190.0, rgba(70, 96, 200, 0.30));
        g.radial(300.0, 92.0, 150.0, 120.0, rgba(255, 170, 64, 0.15));
        let mut seed = 0x2545_f491u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 10_000) as f32 / 10_000.0
        };
        for i in 0..48 {
            let (x, y, r, a) = (rnd() * CARD_W, rnd() * CARD_H, 0.5 + rnd() * 0.9, 0.12 + rnd() * 0.4);
            let twinkle = 0.65 + 0.35 * (t * (0.8 + rnd() * 1.4) + i as f32).sin();
            g.fill_circle(x, y, r, white(a * twinkle));
        }
        g.unclip();
        g.stroke_rrect(card, 24.0, 1.0, white(0.09), false);

        // Close.
        let close = Rect::centered(CARD_W - 26.0, 26.0, 26.0, 26.0);
        if self.hovered(close) {
            g.fill_circle(close.x + 13.0, close.y + 13.0, 13.0, white(0.12));
        }
        g.icon('\u{E8BB}', close.x + 13.0, close.y + 13.5, 9.0, white(0.8));
        self.hit(close, Act::Close);

        // Logo, floating gently, then the name.
        let bob = (t * 1.3).sin() * 2.5;
        if let Some(logo) = &self.logo {
            g.draw_image(logo, Rect::new(CARD_W / 2.0 - 52.0, 32.0 + bob, 104.0, 104.0), 0.0, 1.0);
        }
        g.text("Hilal", 0.0, 164.0, CARD_W, 30.0, 700, WHITE, Align::Center);
        g.text("Music, focus, water, files and live stats, right in your notch.", 0.0, 194.0, CARD_W, 13.0, 400, white(0.6), Align::Center);

        // Each screen eases in.
        let a = (self.screen_at.elapsed().as_secs_f32() / 0.28).min(1.0);
        let ease = 1.0 - (1.0 - a).powi(3);
        g.fade.set(ease);
        g.translate(M, M + (1.0 - ease) * 10.0);
        match self.screen.clone() {
            Screen::Welcome => self.welcome(),
            Screen::Working { removing } => self.working(removing, dt),
            Screen::Installed => self.done("Hilal is installed", "It lives at the top of your screen, and in the Start menu.", Some(("Launch Hilal", Act::Launch))),
            Screen::ConfirmRemove => self.confirm_remove(),
            Screen::Removed => self.done("Hilal was removed", "Thanks for giving it a try.", None),
            Screen::Failed(msg) => self.failed(&msg),
        }
        let _ = self.gfx.present();
    }

    fn welcome(&self) {
        let g = &self.gfx;
        let panel = Rect::new(150.0, 222.0, 300.0, 84.0);
        g.fill_rrect(panel, 14.0, white(0.05));
        g.stroke_rrect(panel, 14.0, 1.0, white(0.08), false);
        self.toggle_row(243.0, "Start with Windows", self.opts.start_with_windows, Act::ToggleStart);
        g.line(170.0, 264.0, 430.0, 264.0, 1.0, white(0.07));
        self.toggle_row(285.0, "Desktop shortcut", self.opts.desktop_shortcut, Act::ToggleDesktop);

        let label = match &self.installed {
            None => "Install".to_owned(),
            Some(v) if v != ops::VERSION => format!("Update to {}", ops::VERSION),
            Some(_) => "Reinstall".to_owned(),
        };
        self.button(Rect::new(200.0, 326.0, 200.0, 42.0), &label, (GOLD_LIGHT, GOLD_DEEP), Act::Install);
        if self.installed.is_some() {
            self.link(382.0, "Uninstall", Act::AskRemove);
        } else {
            g.text("Installs for you only  ·  No admin needed", 0.0, 382.0, CARD_W, 11.0, 400, white(0.38), Align::Center);
        }
    }

    fn working(&self, removing: bool, dt: f32) {
        let g = &self.gfx;
        let (target, step) = self
            .job
            .as_ref()
            .map(|(j, _)| {
                let j = j.lock().unwrap();
                (j.progress, j.step.clone())
            })
            .unwrap_or((1.0, String::new()));
        // The bar glides toward the real progress instead of jumping.
        let bar = self.bar.get() + (target - self.bar.get()) * (1.0 - (-dt * 7.0).exp());
        self.bar.set(bar);
        g.text(if step.is_empty() { if removing { "Removing…" } else { "Installing…" } } else { &step }, 0.0, 246.0, CARD_W, 13.0, 400, white(0.85), Align::Center);
        let track = Rect::new(150.0, 266.0, 300.0, 6.0);
        g.fill_rrect(track, 3.0, white(0.1));
        if bar > 0.005 {
            let (a, b) = if removing { (RED_LIGHT, RED_DEEP) } else { (GOLD_LIGHT, GOLD_DEEP) };
            g.linear(Rect::new(track.x, track.y, track.w * bar.min(1.0), track.h), 3.0, &[(0.0, a), (1.0, b)], false);
            // A soft light at the leading edge.
            g.radial(track.x + track.w * bar.min(1.0), track.y + 3.0, 14.0, 8.0, [b[0], b[1], b[2], 0.45]);
        }
        g.text(&format!("{:.0}%", bar * 100.0), 0.0, 290.0, CARD_W, 11.0, 400, white(0.45), Align::Center);
    }

    fn done(&self, title: &str, sub: &str, action: Option<(&str, Act)>) {
        let g = &self.gfx;
        let (cx, cy) = (CARD_W / 2.0, 236.0);
        g.radial(cx, cy, 30.0, 30.0, rgba(255, 170, 64, 0.35));
        g.linear(Rect::new(cx - 15.0, cy - 15.0, 30.0, 30.0), 15.0, &[(0.0, GOLD_LIGHT), (1.0, GOLD_DEEP)], true);
        g.line(cx - 7.0, cy + 0.5, cx - 2.0, cy + 5.5, 2.6, INK);
        g.line(cx - 2.0, cy + 5.5, cx + 7.5, cy - 5.0, 2.6, INK);
        g.text(title, 0.0, 272.0, CARD_W, 17.0, 600, WHITE, Align::Center);
        g.text(sub, 0.0, 295.0, CARD_W, 12.0, 400, white(0.55), Align::Center);
        match action {
            Some((label, act)) => {
                self.button(Rect::new(200.0, 322.0, 200.0, 42.0), label, (GOLD_LIGHT, GOLD_DEEP), act);
                self.link(382.0, "Close", Act::Close);
            }
            None => self.button(Rect::new(200.0, 322.0, 200.0, 42.0), "Close", (GOLD_LIGHT, GOLD_DEEP), Act::Close),
        }
    }

    fn confirm_remove(&self) {
        let g = &self.gfx;
        g.text("Remove Hilal from this PC?", 0.0, 236.0, CARD_W, 17.0, 600, WHITE, Align::Center);
        let panel = Rect::new(150.0, 256.0, 300.0, 46.0);
        g.fill_rrect(panel, 14.0, white(0.05));
        g.stroke_rrect(panel, 14.0, 1.0, white(0.08), false);
        self.toggle_row(279.0, "Also delete my tasks, water log, shelf", self.delete_data, Act::ToggleData);
        self.button(Rect::new(200.0, 326.0, 200.0, 42.0), "Uninstall", (RED_LIGHT, RED_DEEP), Act::Remove);
        self.link(382.0, "Cancel", Act::Cancel);
    }

    fn failed(&self, msg: &str) {
        let g = &self.gfx;
        g.text("Something went wrong", 0.0, 246.0, CARD_W, 17.0, 600, WHITE, Align::Center);
        g.text(msg, 40.0, 272.0, CARD_W - 80.0, 12.0, 400, white(0.6), Align::Center);
        self.button(Rect::new(200.0, 316.0, 200.0, 42.0), "Close", (GOLD_LIGHT, GOLD_DEEP), Act::Close);
    }

    // ----- pieces -----------------------------------------------------------------------

    fn toggle_row(&self, cy: f32, label: &str, on: bool, act: Act) {
        let g = &self.gfx;
        let row = Rect::new(156.0, cy - 18.0, 288.0, 36.0);
        if self.hovered(row) {
            g.fill_rrect(row, 10.0, white(0.04));
        }
        g.text(label, 170.0, cy, 220.0, 13.0, 400, white(0.9), Align::Start);
        let track = Rect::new(392.0, cy - 11.0, 38.0, 22.0);
        if on {
            g.linear(track, 11.0, &[(0.0, GOLD_LIGHT), (1.0, GOLD_DEEP)], false);
        } else {
            g.fill_rrect(track, 11.0, white(0.16));
        }
        let kx = if on { track.x + track.w - 11.0 } else { track.x + 11.0 };
        g.fill_circle(kx, cy, 8.5, WHITE);
        self.hit(row, act);
    }

    fn button(&self, r: Rect, label: &str, (light, deep): (Color, Color), act: Act) {
        let g = &self.gfx;
        let hover = self.hovered(r);
        g.radial(r.x + r.w / 2.0, r.y + r.h - 2.0, r.w * 0.55, 16.0, [deep[0], deep[1], deep[2], if hover { 0.5 } else { 0.32 }]);
        g.linear(r, r.h / 2.0, &[(0.0, light), (1.0, deep)], false);
        if hover {
            g.fill_rrect(r, r.h / 2.0, white(0.12));
        }
        g.text(label, r.x, r.y + r.h / 2.0, r.w, 14.0, 700, INK, Align::Center);
        self.hit(r, act);
    }

    fn link(&self, cy: f32, label: &str, act: Act) {
        let g = &self.gfx;
        let w = g.measure(label, 12.0, 400) + 16.0;
        let r = Rect::centered(CARD_W / 2.0, cy, w, 24.0);
        let hover = self.hovered(r);
        g.text(label, r.x, cy, r.w, 12.0, 400, white(if hover { 0.95 } else { 0.55 }), Align::Center);
        self.hit(r, act);
    }
}

/// Converts a client-area mouse position (physical px) to card coordinates (dp).
fn card_point(lparam: LPARAM, scale: f32) -> (f32, f32) {
    let x = (lparam.0 & 0xffff) as i16 as f32;
    let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
    (x / scale - M, y / scale - M)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            // The top band of the card drags the window (except the close button).
            let hit = with_setup(|s| {
                let mut r = RECT::default();
                unsafe {
                    let _ = GetWindowRect(hwnd, &mut r);
                }
                let k = s.gfx.scale;
                let x = ((lparam.0 & 0xffff) as i16 as i32 - r.left) as f32 / k - M;
                let y = (((lparam.0 >> 16) & 0xffff) as i16 as i32 - r.top) as f32 / k - M;
                (0.0..CARD_W - 50.0).contains(&x) && (0.0..56.0).contains(&y)
            });
            if hit == Some(true) {
                return LRESULT(HTCAPTION as isize);
            }
            return LRESULT(HTCLIENT as isize);
        }
        WM_SETCURSOR => {
            if with_setup(|s| s.region_at(s.cursor.0, s.cursor.1).is_some()) == Some(true) {
                unsafe {
                    if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                        SetCursor(hand);
                    }
                }
                return LRESULT(1);
            }
        }
        WM_LBUTTONDOWN => {
            with_setup(|s| {
                let (x, y) = card_point(lparam, s.gfx.scale);
                s.pressed = s.region_at(x, y);
            });
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            with_setup(|s| {
                let (x, y) = card_point(lparam, s.gfx.scale);
                if let Some(a) = s.pressed.take() {
                    if s.region_at(x, y) == Some(a) {
                        s.perform(a);
                    }
                }
            });
            return LRESULT(0);
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
