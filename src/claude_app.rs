//! Hands text to the Claude desktop app: puts it on the clipboard, brings the app's window
//! to the front, and pastes it into whatever box has focus there (the message box). The
//! app keeps the final say: the user presses Enter in it.

use windows::core::PWSTR;
use windows::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HGLOBAL, HWND, LPARAM, RECT};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

const CF_UNICODETEXT: u32 = 13;

/// The clipboard's text, if it holds any.
pub fn clipboard_text() -> Option<String> {
    unsafe {
        OpenClipboard(None).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(g);
            Some(s)
        });
        let _ = CloseClipboard();
        text
    }
}

fn set_clipboard(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let ok = (|| {
            EmptyClipboard().ok()?;
            let g = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let p = GlobalLock(g) as *mut u16;
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(g);
            // The clipboard owns the memory from here on.
            SetClipboardData(CF_UNICODETEXT, HANDLE(g.0)).ok()?;
            Some(())
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}

/// The path of the program behind a window.
fn exe_of(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// The Claude app's main window: the biggest visible, titled window of its Claude.exe
/// (not the Claude Code engine, which has no window).
pub fn find_window() -> Option<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let best = &mut *(lp.0 as *mut Option<(HWND, i64)>);
        if IsWindowVisible(hwnd).as_bool() && GetWindowTextLengthW(hwnd) > 0 {
            if let Some(exe) = exe_of(hwnd) {
                let lower = exe.to_ascii_lowercase();
                if lower.ends_with("\\claude.exe") && !lower.contains("claude-code") {
                    let mut r = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut r);
                    let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
                    if best.map_or(true, |(_, a)| area > a) {
                        *best = Some((hwnd, area));
                    }
                }
            }
        }
        BOOL(1)
    }
    let mut best: Option<(HWND, i64)> = None;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut best as *mut _ as isize));
    }
    best.map(|(h, _)| h)
}

/// Focuses the Claude app's message box (its accessibility name is "Prompt"), found
/// through UI Automation like a screen reader would. Does nothing if it can't be found.
fn focus_prompt(hwnd: HWND) {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_EditControlTypeId,
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let found = (|| -> windows::core::Result<()> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let root = uia.ElementFromHandle(hwnd)?;
            // Only the text boxes (asking for everything walks the whole app and is slow).
            let is_edit = uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &windows::core::VARIANT::from(UIA_EditControlTypeId.0))?;
            let edits = root.FindAll(TreeScope_Descendants, &is_edit)?;
            for i in 0..edits.Length()? {
                let e = edits.GetElement(i)?;
                if e.CurrentName()? == "Prompt" {
                    return e.SetFocus();
                }
            }
            Ok(())
        })();
        let _ = found;
    }
}

/// Puts `text` in the Claude app's message box. Fails if the app isn't open.
pub fn paste(text: &str) -> Result<(), &'static str> {
    let hwnd = find_window().ok_or("The Claude app isn't open")?;
    if !set_clipboard(text) {
        return Err("Couldn't use the clipboard");
    }
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // The popup was just clicked and typed in, so Windows lets us pass the foreground
        // on. (No Alt-tap trick: a lone Alt moves the app's focus to its menu.)
        let _ = SetForegroundWindow(hwnd);
    }
    // Paste only once the app really is in front, so Ctrl+V can't land anywhere else; if
    // it doesn't come forward, the text just waits on the clipboard.
    let target = hwnd.0 as isize;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let front = (0..30).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(40));
            unsafe { GetForegroundWindow() }.0 as isize == target
        });
        if front {
            // Put the focus in the message box itself (wherever it was in the app), then a
            // beat for it to take it.
            focus_prompt(HWND(target as _));
            std::thread::sleep(std::time::Duration::from_millis(120));
            unsafe {
                keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
                keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            }
        }
        let _ = tx.send(front);
    });
    // Up to ~1.3 s, so the popup can say which way it went.
    match rx.recv_timeout(std::time::Duration::from_millis(1500)) {
        Ok(true) => Ok(()),
        _ => Err("Copied \u{2014} the Claude app didn't come forward; paste it there with Ctrl+V"),
    }
}

#[cfg(test)]
mod tests {
    /// Finds the Claude app's window without touching it (run with --ignored --nocapture
    /// while the app is open).
    #[test]
    #[ignore]
    fn finds_the_claude_window() {
        use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
        let h = super::find_window().expect("no Claude window");
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, &mut buf) };
        println!("Claude window: {:?} title {:?}", h, String::from_utf16_lossy(&buf[..n as usize]));
    }
}
