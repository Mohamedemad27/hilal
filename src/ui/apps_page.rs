//! The Apps page (Nova mode): up to twelve pinned apps in two rows. A click brings the app's
//! open window to the front (or starts it), a right-click opens a new window, a dot marks
//! the apps that are open. Add opens the Start menu list in Settings; Edit shows a × on each.

use std::time::{Duration, Instant};

use super::{Action, App, Page};
use crate::apps::{self, AppEntry};
use crate::gfx::{white, Align, Rect, WHITE};

impl App {
    /// The pinned apps: as chosen, or Nova's picks from what the Start menu has.
    pub(super) fn pinned_apps(&self) -> Vec<AppEntry> {
        if let Some(list) = &self.store.apps_pinned {
            return list.clone();
        }
        let cat = &self.store.apps_catalog;
        let mut out: Vec<AppEntry> = Vec::new();
        for want in apps::DEFAULTS {
            let w = want.to_lowercase();
            let hit = cat.iter().find(|a| a.name.to_lowercase() == w).or_else(|| cat.iter().find(|a| a.name.to_lowercase().starts_with(&w)));
            if let Some(a) = hit {
                if !out.iter().any(|o| o.id == a.id) {
                    out.push(a.clone());
                }
            }
        }
        out
    }

    /// Reads the Start menu in the background (once a run, or when there's no list yet).
    pub(super) fn load_apps(&mut self) {
        if self.apps_loading.is_some() || self.apps_loaded {
            return;
        }
        self.apps_loaded = true;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(apps::catalog());
        });
        self.apps_loading = Some(rx);
    }

    /// The Start menu list as it comes, and which pinned apps are open (every 2 s while the
    /// page shows).
    pub(super) fn tick_apps(&mut self) {
        // The icons asked for, read in the background.
        if let Some(rx) = &self.icons_loading {
            while let Ok((id, px)) = rx.try_recv() {
                let img = px.and_then(|(px, w, h)| self.gfx.image(&px, w, h));
                self.app_icons.borrow_mut().insert(id, img);
            }
        }
        let wanted: Vec<String> = std::mem::take(&mut *self.icons_wanted.borrow_mut());
        let fresh: Vec<String> = wanted.into_iter().filter(|id| !self.app_icons.borrow().contains_key(id) && !self.icons_asked.contains(id)).collect();
        if !fresh.is_empty() {
            self.icons_asked.extend(fresh.iter().cloned());
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                unsafe {
                    let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
                }
                for id in fresh {
                    let px = apps::icon(&id, 48);
                    if tx.send((id, px)).is_err() {
                        break;
                    }
                }
            });
            self.icons_loading = Some(rx);
        }
        if let Some(rx) = &self.apps_loading {
            if let Ok(list) = rx.try_recv() {
                self.apps_loading = None;
                if !list.is_empty() {
                    // Nova's picks become the pinned list the first time.
                    if self.store.apps_pinned.is_none() {
                        self.store.apps_catalog = list.clone();
                        self.store.apps_pinned = Some(self.pinned_apps());
                    }
                    self.store.apps_catalog = list;
                    self.store.save();
                }
            }
        }
        if self.open && self.page == Page::Apps {
            self.load_apps();
            if self.apps_running_at.elapsed() >= Duration::from_secs(2) {
                self.apps_running_at = Instant::now();
                let ids: Vec<String> = self.pinned_apps().into_iter().map(|a| a.id).collect();
                self.apps_running = apps::running(&ids);
            }
        } else {
            self.apps_edit = false;
            // (So the dots are fresh the next time it opens.)
            self.apps_running_at = Instant::now() - Duration::from_secs(10);
        }
    }

    /// A right-click: on a pinned app, a new window of it.
    pub fn mouse_right(&mut self, x: f32, y: f32) {
        if let Some(Action::AppOpen(i)) = self.region_at(x, y) {
            if let Some(a) = self.pinned_apps().get(i) {
                apps::launch(&a.id);
            }
        }
    }

    pub(super) fn apps_page(&self) {
        let g = &self.gfx;
        self.header("Apps", None);
        let list = self.pinned_apps();
        // Add and Edit, beside the title.
        let mut x = 15.0 + g.measure("Apps", 15.0, 600) + 10.0;
        for (label, act, on) in [("Add", Action::AppsAdd, list.len() < apps::MAX_PINNED), (if self.apps_edit { "Done" } else { "Edit" }, Action::AppsEdit, !list.is_empty() || self.apps_edit)] {
            if !on {
                continue;
            }
            let w = g.measure(label, 9.5, 500) + 16.0;
            let r = Rect::new(x, 11.0, w, 16.0);
            let lit = self.apps_edit && act == Action::AppsEdit;
            g.fill_rrect(r, 8.0, if lit { super::rgba(0x26, 0x99, 0xff, 0.4) } else { white(if self.hovered(r) { 0.22 } else { 0.13 }) });
            g.text(label, r.x, r.y + 8.0, r.w, 9.5, 500, white(0.9), Align::Center);
            self.hit(r, act);
            x += w + 6.0;
        }
        if list.is_empty() {
            let msg = if self.apps_loading.is_some() { "Reading the Start menu\u{2026}" } else { "No apps yet. Use Add to pin your favourites." };
            g.text(msg, 44.0, 78.0, 336.0, 12.0, 500, white(0.6), Align::Center);
            return;
        }
        let icons = self.app_icons.borrow();
        for (i, a) in list.iter().enumerate().take(apps::MAX_PINNED) {
            let (col, row) = ((i % 6) as f32, (i / 6) as f32);
            let tile = Rect::new(44.0 + col * 56.0, 33.0 + row * 45.0, 52.0, 43.0);
            let lit = self.hovered(tile);
            if lit {
                g.fill_rrect(tile, 10.0, white(0.08));
            }
            // (Icons load in the background; a letter stands in until then.)
            let ic = Rect::centered(tile.x + tile.w / 2.0, tile.y + 15.0, 24.0, 24.0);
            match icons.get(&a.id) {
                Some(Some(img)) => g.draw_image(img, ic, 5.0, 1.0),
                got => {
                    if got.is_none() {
                        let mut want = self.icons_wanted.borrow_mut();
                        if !want.contains(&a.id) {
                            want.push(a.id.clone());
                        }
                    }
                    g.fill_rrect(ic, 6.0, white(0.15));
                    let first: String = a.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                    g.text(&first, ic.x, ic.y + 12.0, ic.w, 12.0, 700, WHITE, Align::Center);
                }
            }
            g.text(&a.name, tile.x + 2.0, tile.y + 35.0, tile.w - 4.0, 8.5, 500, white(0.75), Align::Center);
            if self.apps_running.get(i).copied().unwrap_or(false) {
                g.fill_circle(tile.x + tile.w / 2.0, tile.y + 29.5, 1.6, white(0.85));
            }
            if self.apps_edit {
                let x = Rect::centered(tile.x + tile.w - 9.0, tile.y + 6.0, 14.0, 14.0);
                g.fill_circle(x.x + 7.0, x.y + 7.0, 7.0, super::rgba(0xff, 0x4b, 0x55, if self.hovered(x) { 1.0 } else { 0.85 }));
                g.icon('\u{E711}', x.x + 7.0, x.y + 7.5, 6.0, WHITE);
                self.hit(x, Action::AppRemove(i));
            } else {
                self.hit(tile, Action::AppOpen(i));
            }
        }
    }
}
