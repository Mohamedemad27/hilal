//! Downloads by browsers and other download managers, seen from the Downloads folder (Nova
//! mode, as Nova Island has it): a partial file (.crdownload, .part, ...) that grows is a
//! download, and its disappearance with a finished file left behind is the end. Only the
//! folder itself is looked at (not its subfolders), once a second, and no file is ever read.
//! A toast says when one starts and when it's done (a click shows it in Explorer).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use super::toasts::ToastAct;
use super::App;
use crate::gfx::rgb;

const PARTIAL: [&str; 7] = ["crdownload", "part", "partial", "download", "opdownload", "!ut", "!qb"];
/// A download that ends sooner never shows as starting.
const SHOW_AFTER: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct Watched {
    /// Partial files: when first seen, and whether its start was told.
    live: HashMap<PathBuf, (Instant, bool)>,
    /// Partial files already there when watching began (old news).
    ignored: Vec<PathBuf>,
    primed: bool,
    at: Option<Instant>,
}

fn is_partial(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| PARTIAL.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).map(|r| r.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.path()).collect()).unwrap_or_default()
}

/// The user's Downloads folder (wherever it was moved).
fn downloads_dir() -> Option<PathBuf> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let p = unsafe { SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, HANDLE::default()) }.ok()?;
    unsafe { p.to_string() }.ok().map(PathBuf::from)
}

impl App {
    pub(super) fn tick_folder_downloads(&mut self) {
        let on = self.nova() && self.store.features.folder_downloads;
        if !on {
            self.folder_dl = Watched::default();
            return;
        }
        if self.folder_dl.at.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        self.folder_dl.at = Some(Instant::now());
        let Some(dir) = downloads_dir() else { return };
        let list = files(&dir);
        let partials: Vec<PathBuf> = list.iter().filter(|p| is_partial(p)).cloned().collect();
        let w = &mut self.folder_dl;
        if !w.primed {
            w.primed = true;
            w.ignored = partials;
            return;
        }
        let toasts = self.store.features.dl_toasts;
        let now = Instant::now();
        let mut start: Vec<String> = Vec::new();
        for p in &partials {
            if w.ignored.contains(p) {
                continue;
            }
            let e = w.live.entry(p.clone()).or_insert((now, false));
            if !e.1 && now - e.0 >= SHOW_AFTER {
                e.1 = true;
                start.push(final_name(p));
            }
        }
        let gone: Vec<(PathBuf, (Instant, bool))> = w.live.iter().filter(|(p, _)| !partials.contains(p)).map(|(p, v)| (p.clone(), *v)).collect();
        w.ignored.retain(|p| partials.contains(p));
        let mut done: Vec<PathBuf> = Vec::new();
        for (p, _) in gone {
            w.live.remove(&p);
            if let Some(f) = finished(&p, &list) {
                done.push(f);
            }
        }
        if !toasts {
            return;
        }
        for name in start {
            self.toast('\u{E896}', rgb(0x40, 0xc8, 0xff), name, "Downloading", None, 2600);
        }
        for f in done {
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.happened(format!("Downloaded {name}"));
            self.celebrate(&[rgb(0x30, 0xd1, 0x58), rgb(0x40, 0xc8, 0xff), rgb(0xff, 0xd4, 0x79)]);
            self.toast('\u{E73E}', rgb(0x30, 0xd1, 0x58), name, "Done", Some(1.0), 4200);
            if let Some(t) = &mut self.toast {
                t.action = Some(ToastAct::Reveal(f));
            }
        }
    }
}

/// "Movie.mp4.crdownload" is "Movie.mp4".
fn final_name(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The finished file a partial one became (None: cancelled). Chrome's "Unconfirmed
/// 123.crdownload" gets its real name only at the end: then it's the newest file that came in
/// the last few seconds.
fn finished(partial: &Path, list: &[PathBuf]) -> Option<PathBuf> {
    let f = partial.with_extension("");
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if size(&f) > 0 {
        return Some(f);
    }
    if final_name(partial).to_lowercase().starts_with("unconfirmed ") {
        let now = SystemTime::now();
        return list
            .iter()
            .filter(|p| !is_partial(p) && size(p) > 0)
            .filter_map(|p| Some((std::fs::metadata(p).ok()?.modified().ok()?, p)))
            .filter(|(m, _)| now.duration_since(*m).map_or(true, |d| d < Duration::from_secs(5)))
            .max_by_key(|(m, _)| *m)
            .map(|(_, p)| p.clone());
    }
    None
}
