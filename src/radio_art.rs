//! Quran Radio's pictures: a station has none of its own, so Hilal finds or makes one. A
//! reciter's photo when Wikidata has a free one (a Wikimedia Commons file); else, as for the
//! official radios, a cover drawn here: a crescent over an eight-pointed star, gold on deep
//! green (the reciters' covers each a shade of their own). Kept in %APPDATA%\Hilal\radio-art
//! once found, so each is looked up once.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

/// The picture for a station (encoded, JPEG or PNG).
pub fn picture(name: &str, official: bool) -> Vec<u8> {
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        name.hash(&mut h);
        format!("{:016x}", h.finish())
    };
    let dir = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Hilal").join("radio-art"));
    let (photo_file, none_file) = match &dir {
        Some(d) => (Some(d.join(format!("{key}.img"))), Some(d.join(format!("{key}.none")))),
        None => (None, None),
    };
    if let Some(bytes) = photo_file.as_ref().and_then(|f| std::fs::read(f).ok()) {
        return bytes;
    }
    let looked = none_file.as_ref().is_some_and(|f| f.exists());
    if !official && !looked {
        if let Some(bytes) = photo(name) {
            if let (Some(d), Some(f)) = (&dir, &photo_file) {
                let _ = std::fs::create_dir_all(d).and_then(|_| std::fs::write(f, &bytes));
            }
            return bytes;
        }
        // (None to be had: not asked again. A full drive just means asking next time.)
        if let (Some(d), Some(f)) = (&dir, &none_file) {
            let _ = std::fs::create_dir_all(d).and_then(|_| std::fs::write(f, b""));
        }
    }
    cover(name, official)
}

/// A reciter's photo: the first of Wikidata's matches for the name that is a reciter (or an
/// imam) and has a picture.
fn photo(name: &str) -> Option<Vec<u8>> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).user_agent("Hilal (https://github.com/Mohamedemad27/hilal)").build();
    let q = name.trim();
    let q = q.strip_prefix("\u{0627}\u{0644}\u{0634}\u{064a}\u{062e} ").or_else(|| q.strip_prefix("\u{0634}\u{064a}\u{062e} ")).unwrap_or(q); // "الشيخ " / "شيخ "
    let api = "https://www.wikidata.org/w/api.php";
    let found: serde_json::Value = agent
        .get(api)
        .query("action", "wbsearchentities")
        .query("format", "json")
        .query("language", "ar")
        .query("uselang", "ar")
        .query("limit", "5")
        .query("search", q)
        .call()
        .ok()?
        .into_json()
        .ok()?;
    let ids: Vec<&str> = found["search"].as_array()?.iter().filter_map(|r| r["id"].as_str()).collect();
    if ids.is_empty() {
        return None;
    }
    let all: serde_json::Value = agent
        .get(api)
        .query("action", "wbgetentities")
        .query("format", "json")
        .query("props", "claims|descriptions")
        .query("languages", "ar")
        .query("ids", &ids.join("|"))
        .call()
        .ok()?
        .into_json()
        .ok()?;
    // قارئ، قرآن، مقرئ، إمام
    const READER: [&str; 4] = ["\u{0642}\u{0627}\u{0631}\u{0626}", "\u{0642}\u{0631}\u{0622}\u{0646}", "\u{0645}\u{0642}\u{0631}\u{0626}", "\u{0625}\u{0645}\u{0627}\u{0645}"];
    let file = ids.iter().find_map(|id| {
        let e = &all["entities"][*id];
        let about = e["descriptions"]["ar"]["value"].as_str().unwrap_or("");
        if !READER.iter().any(|w| about.contains(w)) {
            return None;
        }
        e["claims"]["P18"][0]["mainsnak"]["datavalue"]["value"].as_str()
    })?;
    let resp = agent.get("https://commons.wikimedia.org/w/index.php").query("title", "Special:FilePath").query("file", file).query("width", "320").call().ok()?;
    let mut bytes = Vec::new();
    resp.into_reader().take(4 << 20).read_to_end(&mut bytes).ok()?;
    (bytes.len() > 1000).then_some(bytes)
}

/// The cover drawn for a station (PNG): a gold crescent over a faint eight-pointed star
/// and its ring, on a deep green that's lighter in the middle. A reciter's green leans a
/// little toward teal or olive, after his name.
fn cover(name: &str, official: bool) -> Vec<u8> {
    const N: u32 = 320;
    let shade = if official {
        0.0
    } else {
        let sum: u32 = name.chars().map(|c| c as u32).sum();
        (sum % 7) as f32 / 6.0 * 2.0 - 1.0
    };
    // Deep green, leaning toward teal (shade < 0) or olive (shade > 0).
    let base = [0.04 + 0.03 * shade.max(0.0), 0.24 + 0.02 * shade, 0.17 - 0.05 * shade];
    let gold = [0.91, 0.76, 0.35];
    let c = N as f32 / 2.0;
    // The star: two squares, one turned an eighth, as one outline of 16 points.
    let star: Vec<(f32, f32)> = (0..16)
        .map(|i| {
            let a = std::f32::consts::PI * i as f32 / 8.0 - std::f32::consts::FRAC_PI_2;
            let r = if i % 2 == 0 { 128.0 } else { 128.0 * 0.765 };
            (c + r * a.cos(), c + r * a.sin())
        })
        .collect();
    let seg = |px: f32, py: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32)| -> f32 {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
    };
    let cover = |d: f32| (d + 0.5).clamp(0.0, 1.0); // inside by `d` px, smoothed over a pixel
    let mut img = image::RgbImage::new(N, N);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let r = ((fx - c).powi(2) + (fy - c).powi(2)).sqrt();
        let glow = 1.0 + 0.55 * (1.0 - r / (c * 1.42)).max(0.0);
        let mut col = [base[0] * glow, base[1] * glow, base[2] * glow];
        let mut over = |a: f32, k: [f32; 3]| {
            for i in 0..3 {
                col[i] += (k[i] - col[i]) * a;
            }
        };
        // The star's outline and the ring around it, faint.
        let d_star = (0..16).map(|i| seg(fx, fy, star[i], star[(i + 1) % 16])).fold(f32::MAX, f32::min);
        over(0.28 * cover(1.4 - d_star), gold);
        over(0.22 * cover(1.1 - (r - 138.0).abs()), gold);
        // The crescent: a disc less a disc shifted up and right.
        let d_in = 62.0 - r;
        let d_cut = ((fx - c - 24.0).powi(2) + (fy - c + 16.0).powi(2)).sqrt() - 54.0;
        over(cover(d_in.min(d_cut)), gold);
        *px = image::Rgb(col.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

#[cfg(test)]
mod tests {
    /// Writes both kinds of cover to K:\hilal\research\radio-art (to look at). Run with
    /// --ignored.
    #[test]
    #[ignore]
    fn covers() {
        let dir = std::path::Path::new(r"K:\hilal\research\radio-art");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("official.png"), super::cover("x", true)).unwrap();
        std::fs::write(dir.join("reciter.png"), super::cover("\u{0623}\u{062d}\u{0645}\u{062f}", false)).unwrap();
        let photo = super::photo("\u{0639}\u{0628}\u{062f} \u{0627}\u{0644}\u{0628}\u{0627}\u{0633}\u{0637} \u{0639}\u{0628}\u{062f} \u{0627}\u{0644}\u{0635}\u{0645}\u{062f}"); // عبد الباسط عبد الصمد
        if let Some(p) = &photo {
            std::fs::write(dir.join("photo.jpg"), p).unwrap();
        }
        println!("photo for Abdul Basit: {} bytes", photo.map_or(0, |p| p.len()));
    }
}
