//! The mini player (Nova mode): a YouTube video playing in a browser pops out into a small
//! window that stays on top, at the second it had reached. The video's link comes from the
//! browser's address bar (read through UI Automation, like a screen reader would); the window
//! is Edge's app window on YouTube's own embed page, placed at the bottom right.

use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

const BROWSERS: [&str; 4] = ["chrome.exe", "msedge.exe", "brave.exe", "opera.exe"];

/// The video id in a YouTube link ("watch?v=", "youtu.be/", "shorts/", "live/", "embed/").
pub fn video_id(url: &str) -> Option<String> {
    let u = url.trim();
    let after = |key: &str| u.find(key).map(|i| &u[i + key.len()..]);
    let raw = if u.contains("youtube.com/watch") {
        u.split(['?', '&']).find_map(|p| p.strip_prefix("v="))
    } else {
        after("youtu.be/").or_else(|| after("youtube.com/shorts/")).or_else(|| after("youtube.com/live/")).or_else(|| after("youtube.com/embed/"))
    }?;
    let id: String = raw.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    (id.len() >= 6).then_some(id)
}

fn exe_name(h: HWND) -> Option<String> {
    use windows::Win32::System::Threading::*;
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(p);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]).rsplit('\\').next().unwrap_or("").to_lowercase())
    }
}

/// The browsers' main windows, titled (Chromium's class), those whose title has `want` first.
fn browser_windows(want: &str) -> Vec<(HWND, String)> {
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let list = &mut *(l.0 as *mut Vec<HWND>);
        let mut class = [0u16; 64];
        let n = GetClassNameW(h, &mut class);
        if IsWindowVisible(h).as_bool() && String::from_utf16_lossy(&class[..n as usize]) == "Chrome_WidgetWin_1" && GetWindowTextLengthW(h) > 0 {
            list.push(h);
        }
        true.into()
    }
    let mut hs: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut hs as *mut _ as isize));
    }
    let want = want.to_lowercase();
    let mut out: Vec<(HWND, String)> = hs
        .into_iter()
        .filter(|h| exe_name(*h).is_some_and(|e| BROWSERS.contains(&e.as_str())))
        .map(|h| {
            let mut buf = [0u16; 512];
            let n = unsafe { GetWindowTextW(h, &mut buf) };
            (h, String::from_utf16_lossy(&buf[..n as usize]))
        })
        .collect();
    out.sort_by_key(|(_, t)| if !want.is_empty() && t.to_lowercase().contains(&want) { 0 } else { 1 });
    out
}

/// The address bar's text in a browser window.
fn address(h: HWND) -> Option<String> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Accessibility::*;
    unsafe {
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let root = uia.ElementFromHandle(h).ok()?;
        let is_edit = uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &windows::core::VARIANT::from(UIA_EditControlTypeId.0)).ok()?;
        let edit = root.FindFirst(TreeScope_Descendants, &is_edit).ok()?;
        let v: IUIAutomationValuePattern = edit.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
        v.CurrentValue().ok().map(|s| s.to_string())
    }
}

/// Finds the YouTube video playing (the tab titled like the song, else any YouTube tab in
/// front), as (video id, browser window). Err: what to tell.
pub fn find(title: &str) -> Result<(String, HWND), &'static str> {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let wins = browser_windows(title);
    if wins.is_empty() {
        return Err("Works with YouTube in Chrome, Edge or Brave");
    }
    for (h, _) in &wins {
        if let Some(id) = address(*h).and_then(|u| video_id(&u)) {
            return Ok((id, *h));
        }
    }
    Err("Couldn't find this video's YouTube link")
}

/// Opens the mini player on a video at `seconds` in: Edge's app window, kept on top, at the
/// bottom right of the work area.
pub fn open(id: &str, seconds: u64) {
    let url = format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1&start={seconds}");
    let before: Vec<HWND> = browser_windows("").into_iter().map(|w| w.0).collect();
    let started = std::process::Command::new("cmd")
        .args(["/c", "start", "", "msedge", &format!("--app={url}"), "--window-size=480,300"])
        .creation_flags_hidden()
        .spawn()
        .is_ok();
    if !started {
        return;
    }
    // Waits for its window, then keeps it on top in the corner.
    let until = Instant::now() + Duration::from_secs(8);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(250));
        let new = browser_windows("").into_iter().find(|(h, _)| !before.contains(h) && exe_name(*h).as_deref() == Some("msedge.exe"));
        if let Some((h, _)) = new {
            unsafe {
                let mut wa = windows::Win32::Foundation::RECT::default();
                let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
                let k = windows::Win32::UI::HiDpi::GetDpiForWindow(h) as i32;
                let (w, hgt) = (480 * k / 96, 300 * k / 96);
                let gap = 16 * k / 96;
                let _ = SetWindowPos(h, HWND_TOPMOST, wa.right - w - gap, wa.bottom - hgt - gap, w, hgt, SWP_SHOWWINDOW);
            }
            return;
        }
    }
}

trait Hidden {
    fn creation_flags_hidden(&mut self) -> &mut Self;
}

impl Hidden for std::process::Command {
    fn creation_flags_hidden(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_video_ids() {
        for (u, id) in [
            ("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s", "dQw4w9WgXcQ"),
            ("youtube.com/watch?list=PL1&v=abcdef12345", "abcdef12345"),
            ("https://youtu.be/dQw4w9WgXcQ?si=x", "dQw4w9WgXcQ"),
            ("https://www.youtube.com/shorts/aBcDeF_-123", "aBcDeF_-123"),
        ] {
            assert_eq!(super::video_id(u).as_deref(), Some(id), "{u}");
        }
        assert_eq!(super::video_id("https://www.youtube.com/"), None);
    }

    /// Reads the browsers' address bars on this PC, for a YouTube video (run with --ignored).
    #[test]
    #[ignore]
    fn finds_the_video_in_a_browser() {
        println!("browser windows: {:?}", super::browser_windows("").iter().map(|w| &w.1).collect::<Vec<_>>());
        println!("found: {:?}", super::find("").map(|f| f.0));
    }
}
