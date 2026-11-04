//! News from Hilal (Nova mode, as Nova Island has its team's news): news.json in Hilal's
//! GitHub repository, read two minutes after start and every 15 minutes. A new item shows once
//! as a toast for half a minute (one a day at most), and links only to Hilal's own pages.
//! Nothing is sent from the PC: the file is only read.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

const FEED: &str = "https://raw.githubusercontent.com/Mohamedemad27/hilal/master/news.json";
const OWN: &str = "https://github.com/Mohamedemad27/hilal";

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub link: Option<String>,
}

pub struct Feed {
    pub items: Receiver<Vec<Item>>,
    on: Arc<AtomicBool>,
}

impl Feed {
    pub fn start(on: bool) -> Self {
        let (tx, rx) = channel();
        let flag = Arc::new(AtomicBool::new(on));
        let f = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(120));
            loop {
                if f.load(Ordering::Relaxed) {
                    if let Some(list) = read() {
                        let _ = tx.send(list);
                    }
                }
                std::thread::sleep(Duration::from_secs(15 * 60));
            }
        });
        Self { items: rx, on: flag }
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }
}

/// The items in the feed ({"items": [{"id", "title", "link"}]}), their links only to Hilal's
/// own pages.
fn read() -> Option<Vec<Item>> {
    let j: serde_json::Value = ureq::get(FEED).timeout(Duration::from_secs(15)).call().ok()?.into_json().ok()?;
    Some(
        j["items"]
            .as_array()?
            .iter()
            .filter_map(|i| {
                let id = i["id"].as_str()?.to_owned();
                let title = i["title"].as_str()?.trim().to_owned();
                let link = i["link"].as_str().filter(|l| l.starts_with(OWN)).map(str::to_owned);
                (!id.is_empty() && !title.is_empty()).then_some(Item { id, title, link })
            })
            .collect(),
    )
}
