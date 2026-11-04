//! The address bar slot (Nova mode, as Nova Island has it): while a Chromium browser is in
//! front, the notch takes the right third of its address bar (two thirds stay the address's),
//! and shrinks to a thin line while you type there. Found through UI Automation, in the
//! background: the address bar is looked up once per browser window, then only its place
//! and focus are read, a few times a second.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The address bar in front: its place on screen (px), and whether it's being typed in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub rect: RECT,
    pub typing: bool,
}

pub struct Watch {
    pub updates: Receiver<Option<Bar>>,
    on: Arc<AtomicBool>,
}

impl Watch {
    pub fn start() -> Self {
        let (tx, rx) = channel();
        let on = Arc::new(AtomicBool::new(false));
        let flag = on.clone();
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            }
            let uia: Option<IUIAutomation> = unsafe { windows::Win32::System::Com::CoCreateInstance(&CUIAutomation, None, windows::Win32::System::Com::CLSCTX_INPROC_SERVER).ok() };
            let Some(uia) = uia else { return };
            let mut cached: Option<(HWND, Option<IUIAutomationElement>)> = None;
            let mut last: Option<Bar> = None;
            loop {
                std::thread::sleep(Duration::from_millis(350));
                let now = if flag.load(Ordering::Relaxed) { bar_now(&uia, &mut cached) } else { None };
                if now != last {
                    last = now;
                    if tx.send(now).is_err() {
                        return;
                    }
                }
            }
        });
        Self { updates: rx, on }
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }
}

fn is_browser(h: HWND) -> bool {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut class) };
    if String::from_utf16_lossy(&class[..n as usize]) != "Chrome_WidgetWin_1" {
        return false;
    }
    use windows::Win32::System::Threading::*;
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(p);
        let exe = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        ok && ["\\chrome.exe", "\\msedge.exe", "\\brave.exe"].iter().any(|e| exe.ends_with(e))
    }
}

fn bar_now(uia: &IUIAutomation, cached: &mut Option<(HWND, Option<IUIAutomationElement>)>) -> Option<Bar> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() || !is_browser(fg) || unsafe { IsIconic(fg) }.as_bool() {
        return None;
    }
    // (Chromium builds its accessibility tree only once something asks for it, so a first
    // look may find nothing: it looks again until the bar is there.)
    if cached.as_ref().map(|c| c.0) != Some(fg) || cached.as_ref().is_some_and(|c| c.1.is_none()) {
        let edit = unsafe {
            uia.ElementFromHandle(fg).ok().and_then(|root| {
                let cond = uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &windows::core::VARIANT::from(UIA_EditControlTypeId.0)).ok()?;
                root.FindFirst(TreeScope_Descendants, &cond).ok()
            })
        };
        *cached = Some((fg, edit));
    }
    let edit = cached.as_ref()?.1.as_ref()?;
    unsafe {
        let rect = edit.CurrentBoundingRectangle().ok()?;
        // (A bar hidden in full screen, or one that's gone, has no size.)
        if rect.right - rect.left < 200 || rect.bottom - rect.top < 10 {
            return None;
        }
        let typing = edit.CurrentHasKeyboardFocus().is_ok_and(|b| b.as_bool());
        Some(Bar { rect, typing })
    }
}
