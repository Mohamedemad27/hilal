//! What the installer actually does: a per-user install into %LOCALAPPDATA%\Programs\Hilal
//! (no admin), Start menu / desktop shortcuts, an entry in Settings > Apps with its own
//! uninstaller, and optional start with Windows.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.exe"));
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Hilal";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy)]
pub struct Options {
    pub start_with_windows: bool,
    pub desktop_shortcut: bool,
}

/// Progress shared with the UI thread.
#[derive(Default)]
pub struct Job {
    pub progress: f32,
    pub step: String,
    pub result: Option<Result<(), String>>,
}

pub type Shared = Arc<Mutex<Job>>;

fn report(job: &Shared, progress: f32, step: &str) {
    let mut j = job.lock().unwrap();
    j.progress = progress;
    j.step = step.to_owned();
}

pub fn install_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("Programs").join("Hilal")
}

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
}

fn start_menu_link() -> PathBuf {
    env_dir("APPDATA").join(r"Microsoft\Windows\Start Menu\Programs\Hilal.lnk")
}

fn desktop_link() -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        Some(PathBuf::from(s?).join("Hilal.lnk"))
    }
}

/// The installed version, from the Settings > Apps entry, if Hilal is installed.
pub fn installed_version() -> Option<String> {
    reg_get(UNINSTALL_KEY, "DisplayVersion").filter(|_| install_dir().join("hilal.exe").exists())
}

/// Runs the install on a worker thread, reporting progress into `job`.
/// The choices already made, for an update: start with Windows if it does, and a desktop
/// shortcut if there is one.
pub fn current_options() -> Options {
    Options { start_with_windows: reg_get(RUN_KEY, "Hilal").is_some(), desktop_shortcut: desktop_link().is_some_and(|p| p.exists()) }
}

pub fn install(opts: Options, job: Shared) {
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            report(&job, 0.08, "Closing Hilal if it's running…");
            stop_running();

            report(&job, 0.25, "Copying Hilal…");
            let dir = install_dir();
            std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
            let exe = dir.join("hilal.exe");
            write_retrying(&exe, PAYLOAD).map_err(|e| format!("Can't write {}: {e}", exe.display()))?;
            // A copy of this installer stays behind as the uninstaller.
            let me = std::env::current_exe().map_err(|e| e.to_string())?;
            let uninstaller = dir.join("uninstall.exe");
            if me != uninstaller {
                std::fs::copy(&me, &uninstaller).map_err(|e| format!("Can't write the uninstaller: {e}"))?;
            }

            report(&job, 0.55, "Adding shortcuts…");
            make_link(&start_menu_link(), &exe).map_err(|e| format!("Can't create the Start menu shortcut: {e}"))?;
            if let Some(desk) = desktop_link() {
                if opts.desktop_shortcut {
                    make_link(&desk, &exe).map_err(|e| format!("Can't create the desktop shortcut: {e}"))?;
                } else {
                    let _ = std::fs::remove_file(desk);
                }
            }

            report(&job, 0.78, "Registering with Windows…");
            register(&dir, &exe, &uninstaller);
            let run_value = format!("\"{}\"", exe.display());
            if opts.start_with_windows {
                reg_set(RUN_KEY, "Hilal", &run_value);
            } else {
                reg_delete_value(RUN_KEY, "Hilal");
            }

            report(&job, 1.0, "Done");
            Ok(())
        })();
        job.lock().unwrap().result = Some(result);
    });
}

/// Removes everything the installer added. `delete_data` also removes the user's tasks,
/// water log, shelf and settings in %APPDATA%\Hilal.
pub fn uninstall(delete_data: bool, job: Shared) {
    std::thread::spawn(move || {
        report(&job, 0.15, "Closing Hilal…");
        stop_running();
        report(&job, 0.4, "Removing shortcuts…");
        let _ = std::fs::remove_file(start_menu_link());
        if let Some(desk) = desktop_link() {
            let _ = std::fs::remove_file(desk);
        }
        report(&job, 0.65, "Unregistering…");
        reg_delete_value(RUN_KEY, "Hilal");
        reg_delete_tree(UNINSTALL_KEY);
        if delete_data {
            let _ = std::fs::remove_dir_all(env_dir("APPDATA").join("Hilal"));
        }
        report(&job, 0.85, "Removing files…");
        let dir = install_dir();
        let _ = std::fs::remove_file(dir.join("hilal.exe"));
        // This program may be running from that folder (as uninstall.exe), which locks it
        // until the window closes; keep retrying once a second, for up to a minute.
        let d = dir.display();
        let script = format!("for /l %i in (1,1,60) do (if exist \"{d}\" (rmdir /s /q \"{d}\" 2>nul & ping 127.0.0.1 -n 2 >nul))");
        // Raw, so cmd sees the quotes as written (Rust's own escaping uses \" which cmd
        // doesn't understand).
        let _ = Command::new("cmd").arg("/c").raw_arg(&script).creation_flags(NO_WINDOW).spawn();
        report(&job, 1.0, "Done");
        job.lock().unwrap().result = Some(Ok(()));
    });
}

pub fn launch() {
    let _ = Command::new(install_dir().join("hilal.exe")).current_dir(install_dir()).spawn();
}

/// Asks a running Hilal to close (so it removes its own tray icon), and only forces it
/// if it hasn't gone within two seconds.
/// Windows can keep an exe locked for a moment after its process ends, so keep trying
/// for a few seconds before giving up.
fn write_retrying(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let start = std::time::Instant::now();
    loop {
        match std::fs::write(path, data) {
            Err(e) if start.elapsed().as_secs() < 5 => {
                let _ = e;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            other => return other,
        }
    }
}

fn stop_running() {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    let start = std::time::Instant::now();
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("HilalNotch"), None) {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            while FindWindowW(w!("HilalNotch"), None).is_ok_and(|h| !h.is_invalid()) && start.elapsed().as_secs_f32() < 2.0 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    let _ = Command::new("taskkill").args(["/im", "hilal.exe", "/f"]).creation_flags(NO_WINDOW).output();
    std::thread::sleep(std::time::Duration::from_millis(300));
}

fn make_link(at: &Path, target: &Path) -> windows::core::Result<()> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))?;
        if let Some(dir) = target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        link.SetDescription(&HSTRING::from("Your notch for Windows: music, focus, water, files and live stats"))?;
        link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(at.as_os_str()), true)
    }
}

/// The entry under Settings > Apps > Installed apps, with its Uninstall button.
fn register(dir: &Path, exe: &Path, uninstaller: &Path) {
    let k = UNINSTALL_KEY;
    reg_set(k, "DisplayName", "Hilal");
    reg_set(k, "DisplayVersion", VERSION);
    reg_set(k, "Publisher", "Hilal");
    reg_set(k, "DisplayIcon", &exe.display().to_string());
    reg_set(k, "InstallLocation", &dir.display().to_string());
    reg_set(k, "UninstallString", &format!("\"{}\" /uninstall", uninstaller.display()));
    reg_set_dword(k, "NoModify", 1);
    reg_set_dword(k, "NoRepair", 1);
    reg_set_dword(k, "EstimatedSize", ((PAYLOAD.len() * 2) / 1024) as u32);
}

fn reg_set(key: &str, name: &str, value: &str) {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            REG_SZ.0,
            Some(data.as_ptr() as _),
            (data.len() * 2) as u32,
        );
    }
}

fn reg_set_dword(key: &str, name: &str, value: u32) {
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), REG_DWORD.0, Some(&value as *const u32 as _), 4);
    }
}

fn reg_get(key: &str, name: &str) -> Option<String> {
    let mut buf = [0u16; 260];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size))
            .ok()
            .ok()?;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn reg_delete_value(key: &str, name: &str) {
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name));
    }
}

fn reg_delete_tree(key: &str) {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key));
    }
}
