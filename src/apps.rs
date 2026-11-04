//! The Apps page's launcher (as Nova Island has it): the Start menu's apps, their icons, and
//! for a pinned app, its open window brought to the front (or the app started). Windows are
//! matched to an app by their AppUserModelID (what the taskbar groups by), else by the path of
//! their exe. Nothing runs between clicks except the check for which apps are open, and that
//! only while the page shows.

use windows::core::{Interface, HSTRING, PWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AppEntry {
    pub name: String,
    /// Its parsing name in shell:AppsFolder (an AppUserModelID, or a known folder + path).
    pub id: String,
}

/// Nova's picks for a first start, when the PC has them.
pub const DEFAULTS: [&str; 12] = [
    "WhatsApp", "Telegram", "Spotify", "Google Chrome", "Visual Studio Code", "Discord",
    "Microsoft Edge", "File Explorer", "Calculator", "Notepad", "Photos", "Settings",
];
pub const MAX_PINNED: usize = 12;

/// The Start menu's apps, by name (slow-ish: run it off the UI thread).
pub fn catalog() -> Vec<AppEntry> {
    let mut out = Vec::new();
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from("shell:AppsFolder"), None) else { return out };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut got = [None];
            let mut n = 0;
            if items.Next(&mut got, Some(&mut n)).is_err() || n == 0 {
                break;
            }
            let Some(item) = got[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok().and_then(|p| take(p));
            let id = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING).ok().and_then(|p| take(p));
            if let (Some(name), Some(id)) = (name, id) {
                // (Uninstallers, help files and web links clutter the list.)
                let low = name.to_lowercase();
                if low.starts_with("uninstall") || id.starts_with("http") || id.to_lowercase().ends_with(".chm") || id.to_lowercase().ends_with(".url") {
                    continue;
                }
                out.push(AppEntry { name, id });
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

unsafe fn take(p: PWSTR) -> Option<String> {
    let s = p.to_string().ok();
    windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
    s.filter(|s| !s.is_empty())
}

/// An app's icon, `size` px square (premultiplied BGRA).
pub fn icon(id: &str, size: i32) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(format!("shell:AppsFolder\\{id}")), None).ok()?;
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let bmp = factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, ..Default::default() },
            ..Default::default()
        };
        let dc = GetDC(None);
        GetDIBits(dc, bmp, 0, 0, None, &mut info, DIB_RGB_COLORS);
        let (w, h) = (info.bmiHeader.biWidth, info.bmiHeader.biHeight.abs());
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        info.bmiHeader.biHeight = -h; // top-down
        let mut px = vec![0u8; (w * h * 4) as usize];
        let rows = GetDIBits(dc, bmp, 0, h as u32, Some(px.as_mut_ptr() as _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(bmp);
        if rows == 0 || w <= 0 {
            return None;
        }
        // Icons without alpha come back with it all zero.
        if px.chunks_exact(4).all(|p| p[3] == 0) {
            px.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        }
        Some((px, w as u32, h as u32))
    }
}

/// Starts an app (a new window, for one that's open).
pub fn launch(id: &str) {
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(format!("shell:AppsFolder\\{id}")), None, None, SW_SHOWNORMAL);
    }
}

/// What a desktop app's id points at: "{known folder}\path\app.exe" as a full path.
fn exe_of(id: &str) -> Option<String> {
    let low = id.to_lowercase();
    if !low.ends_with(".exe") {
        return None;
    }
    if let Some(rest) = id.strip_prefix('{') {
        let (guid, path) = rest.split_once('}')?;
        let guid = windows::core::GUID::try_from(guid).ok()?;
        let base = unsafe { SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None) }.ok()?;
        let base = unsafe { take(base) }?;
        return Some(format!("{base}{path}").to_lowercase());
    }
    Some(low)
}

/// The app windows open now, each with what it belongs to (its AppUserModelID, and its exe).
fn open_windows() -> Vec<(HWND, Option<String>, Option<String>)> {
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let list = &mut *(l.0 as *mut Vec<HWND>);
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if IsWindowVisible(h).as_bool() && GetWindowTextLengthW(h) > 0 && ex & WS_EX_TOOLWINDOW.0 == 0 && GetWindow(h, GW_OWNER).map_or(true, |o| o.is_invalid()) {
            // (Hidden "cloaked" windows, like suspended Store apps, don't count.)
            let mut cloaked = 0u32;
            let _ = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(h, windows::Win32::Graphics::Dwm::DWMWA_CLOAKED, &mut cloaked as *mut u32 as _, 4);
            if cloaked == 0 {
                list.push(h);
            }
        }
        true.into()
    }
    let mut hwnds: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut hwnds as *mut _ as isize));
    }
    hwnds.into_iter().map(|h| (h, window_aumid(h), window_exe(h))).collect()
}

fn window_aumid(h: HWND) -> Option<String> {
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow, PROPERTYKEY};
    // PKEY_AppUserModel_ID
    let key = PROPERTYKEY { fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: 5 };
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(h).ok()?;
        let v = store.GetValue(&key).ok()?;
        let s = PropVariantToStringAlloc(&v).ok()?;
        take(s)
    }
}

fn window_exe(h: HWND) -> Option<String> {
    use windows::Win32::System::Threading::*;
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(p);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]).to_lowercase())
    }
}

/// An open window of the app, if there is one.
fn window_of(id: &str, open: &[(HWND, Option<String>, Option<String>)]) -> Option<HWND> {
    let exe = exe_of(id);
    open.iter()
        .find(|(_, aumid, wexe)| aumid.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(id)) || (exe.is_some() && wexe.as_deref() == exe.as_deref()))
        .map(|(h, _, _)| *h)
}

/// Which of these apps have a window open.
pub fn running(ids: &[String]) -> Vec<bool> {
    let open = open_windows();
    ids.iter().map(|id| window_of(id, &open).is_some()).collect()
}

/// Brings the app's open window to the front, or starts it.
pub fn open(id: &str) {
    let open = open_windows();
    match window_of(id, &open) {
        Some(h) => unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP, VK_MENU};
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            // (A tap of Alt lets another app's window come to the front.)
            keybd_event(VK_MENU.0 as u8, 0, Default::default(), 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            let _ = SetForegroundWindow(h);
        },
        None => launch(id),
    }
}

#[cfg(test)]
mod tests {
    /// Reads this PC's Start menu (run with --ignored: it depends on the PC).
    #[test]
    #[ignore]
    fn reads_the_start_menu() {
        let list = super::catalog();
        println!("{} apps; first: {:?}", list.len(), list.iter().take(5).collect::<Vec<_>>());
        assert!(!list.is_empty());
        let icon = super::icon(&list[0].id, 48);
        println!("icon of {}: {:?}", list[0].name, icon.as_ref().map(|i| (i.1, i.2)));
    }
}
