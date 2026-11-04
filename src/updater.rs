//! Updates from Hilal's GitHub releases (Nova mode, as Nova Island has them): checked a minute
//! after start and every 6 hours, the new installer downloaded in the background, then
//! offered on the notch ("Update ready · Update now"). An update never accepted is installed
//! when Hilal quits. Only the installed Hilal updates itself (not a copy run from elsewhere).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/Mohamedemad27/hilal/releases/latest";

/// A new version, downloaded: its number and its installer.
#[derive(Clone, Debug)]
pub struct Ready {
    pub version: String,
    pub installer: PathBuf,
}

/// The update waiting to be installed when Hilal quits (if it was never accepted).
pub static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);

pub struct Feed {
    pub updates: Receiver<Ready>,
    on: Arc<AtomicBool>,
}

/// "0.3.10" is newer than "0.3.9".
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| v.trim_start_matches('v').split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    parts(a) > parts(b)
}

/// Whether this is the installed Hilal (in %LOCALAPPDATA%\Programs\Hilal).
fn installed() -> bool {
    let exe = std::env::current_exe().ok().map(|p| p.to_string_lossy().to_lowercase());
    let dir = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("Hilal").to_string_lossy().to_lowercase());
    matches!((exe, dir), (Some(e), Some(d)) if e.starts_with(&d))
}

impl Feed {
    pub fn start(on: bool) -> Self {
        let (tx, rx) = channel();
        let flag = Arc::new(AtomicBool::new(on));
        let f = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(60));
            loop {
                if f.load(Ordering::Relaxed) && installed() {
                    if let Some(r) = check() {
                        if let Ok(mut p) = PENDING.lock() {
                            *p = Some(r.installer.clone());
                        }
                        let _ = tx.send(r);
                    }
                }
                std::thread::sleep(Duration::from_secs(6 * 3600));
            }
        });
        Self { updates: rx, on: flag }
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }
}

/// The latest release, if newer than this one: its installer downloaded (once).
fn check() -> Option<Ready> {
    let j: serde_json::Value = ureq::get(LATEST)
        .set("User-Agent", "Hilal")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let version = j["tag_name"].as_str()?.trim_start_matches('v').to_owned();
    if !newer(&version, env!("CARGO_PKG_VERSION")) {
        return None;
    }
    let asset = j["assets"].as_array()?.iter().find(|a| a["name"].as_str().is_some_and(|n| n.starts_with("HilalSetup") && n.ends_with(".exe")))?;
    let url = asset["browser_download_url"].as_str()?;
    // (Only from GitHub's own release downloads.)
    if !url.starts_with("https://github.com/Mohamedemad27/hilal/releases/download/") {
        return None;
    }
    let size = asset["size"].as_u64()?;
    let dir = std::env::temp_dir().join("Hilal");
    let _ = std::fs::create_dir_all(&dir);
    let installer = dir.join(format!("HilalSetup-{version}.exe"));
    if std::fs::metadata(&installer).map(|m| m.len()) .ok() != Some(size) {
        let r = ureq::get(url).set("User-Agent", "Hilal").timeout(Duration::from_secs(600)).call().ok()?;
        let mut body = Vec::with_capacity(size as usize);
        std::io::Read::read_to_end(&mut r.into_reader(), &mut body).ok()?;
        if body.len() as u64 != size {
            return None;
        }
        let part = installer.with_extension("part");
        std::fs::write(&part, &body).ok()?;
        std::fs::rename(&part, &installer).ok()?;
    }
    Some(Ready { version, installer })
}

/// Runs the installer to update (Hilal opens again at the end, unless it's quitting).
pub fn install(installer: &PathBuf, reopen: bool) {
    let mut c = std::process::Command::new(installer);
    c.arg("/update");
    if !reopen {
        c.arg("/nolaunch");
    }
    let _ = c.spawn();
}

#[cfg(test)]
mod tests {
    #[test]
    fn compares_versions() {
        assert!(super::newer("0.4.0", "0.3.1"));
        assert!(super::newer("v0.3.10", "0.3.9"));
        assert!(!super::newer("0.3.1", "0.3.1"));
        assert!(!super::newer("0.2.9", "0.3.0"));
    }
}
