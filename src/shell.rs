//! Windows shell glue for the Shelf: thumbnails, opening files, dragging them out.

use std::path::Path;

use windows::core::{w, Interface, HSTRING};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_NONE};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

fn item(path: &Path) -> Option<IShellItem> {
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok() }
}

/// The file's thumbnail (or icon) as premultiplied BGRA, at most `size` px square.
pub fn thumbnail(path: &Path, size: i32) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let factory: IShellItemImageFactory = item(path)?.cast().ok()?;
        let hbm = factory.GetImage(SIZE { cx: size, cy: size }, SIIGBF_RESIZETOFIT).ok()?;
        let mut bm = BITMAP::default();
        GetObjectW(hbm, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as _));
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(dc, hbm, 0, h as u32, Some(buf.as_mut_ptr() as _), &mut info, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(hbm);
        if lines == 0 || w == 0 {
            return None;
        }
        // Photo thumbnails come back as 24-bit with an empty alpha channel.
        if buf.chunks_exact(4).all(|p| p[3] == 0) {
            buf.chunks_exact_mut(4).for_each(|p| p[3] = 255);
        }
        Some((buf, w as u32, h as u32))
    }
}

pub fn open(path: &Path) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL);
    }
}

/// Starts a shell drag of the file. Only copy/link are offered, so the original never moves.
/// Returns true when it was dropped somewhere.
pub fn drag_out(path: &Path) -> bool {
    unsafe {
        let Some(item) = item(path) else { return false };
        let Ok(data) = item.BindToHandler::<_, IDataObject>(None, &BHID_DataObject) else { return false };
        matches!(SHDoDragDrop(None, &data, None, DROPEFFECT_COPY | DROPEFFECT_LINK), Ok(e) if e != DROPEFFECT_NONE)
    }
}
