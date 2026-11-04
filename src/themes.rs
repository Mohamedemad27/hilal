//! Nova Island's themes for the notch (Nova mode): only colors and one still decoration, so
//! none costs more than another. A glass theme tints the Liquid Glass its own color; a solid
//! one has its own surface (and skips capturing the screen behind it). Each has an outline,
//! and a light one turns the white text dark.

use crate::gfx::Color;

/// A soft spot of color on a solid surface: where (fractions of the shape), how far, what.
#[derive(Clone, Copy)]
pub struct Blob {
    pub x: f32,
    pub y: f32,
    pub rx: f32,
    pub ry: f32,
    pub c: Color,
}

#[derive(Clone)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    /// Glass, tinted top to bottom; else a solid surface (top, bottom) with its spots.
    pub glass: Option<(Color, Color)>,
    pub bg: (Color, Color),
    pub blobs: Vec<Blob>,
    /// Little stars or dots (fractions, radius in dp, color).
    pub dots: Vec<(f32, f32, f32, Color)>,
    /// A strip along the bottom (snow on the ground), its height and color.
    pub ground: Option<(f32, Color)>,
    pub line: (Color, f32),
    /// The text's own color (light or colored themes), in place of white.
    pub ink: Option<Color>,
    /// Corners this round at most (the retro one is square-ish).
    pub radius: Option<f32>,
}

const fn hex(v: u32) -> Color {
    [((v >> 16) & 0xff) as f32 / 255.0, ((v >> 8) & 0xff) as f32 / 255.0, (v & 0xff) as f32 / 255.0, 1.0]
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

fn blob(x: f32, y: f32, rx: f32, ry: f32, c: Color) -> Blob {
    Blob { x, y, rx, ry, c }
}

/// A theme with nothing set (its Settings swatch color noted beside each).
fn base(id: &'static str, name: &'static str, _swatch: u32) -> Theme {
    Theme {
        id,
        name,
        glass: None,
        bg: (hex(0x000000), hex(0x000000)),
        blobs: Vec::new(),
        dots: Vec::new(),
        ground: None,
        line: (rgba(255, 255, 255, 0.0), 0.0),
        ink: None,
        radius: None,
    }
}

/// Every theme, "original" (Hilal's own look) first.
pub fn all() -> Vec<Theme> {
    let white = |a| rgba(255, 255, 255, a);
    let stars = |c: Color| vec![(0.12, 0.22, 1.4, c), (0.30, 0.72, 1.0, c), (0.51, 0.18, 1.6, c), (0.63, 0.66, 1.0, c), (0.80, 0.28, 1.4, c), (0.92, 0.70, 1.0, c)];
    vec![
        Theme { line: (white(0.15), 1.0), ..base("original", "Original", 0x262b3a) },
        Theme {
            bg: (hex(0x0d4337), hex(0x0a3229)),
            blobs: vec![blob(0.5, -0.3, 0.6, 1.2, rgba(246, 216, 138, 0.16))],
            line: (hex(0xc9a54c), 1.0),
            ink: Some(hex(0xf6ecd0)),
            ..base("bakka", "Bakka", 0x0c211c)
        },
        Theme { glass: Some((rgba(10, 10, 14, 0.38), rgba(10, 10, 14, 0.62))), line: (white(0.85), 1.5), ..base("outline", "Outline", 0x3a3f4d) },
        Theme { line: (white(0.06), 1.0), ..base("midnight", "Midnight", 0x2b2f3a) },
        Theme {
            bg: (hex(0x14301f), hex(0x0b1d13)),
            blobs: vec![
                blob(0.08, 1.1, 0.06, 0.6, hex(0x1f4d2e)),
                blob(0.22, 1.15, 0.08, 0.8, hex(0x183f26)),
                blob(0.84, 1.12, 0.07, 0.7, hex(0x1f4d2e)),
                blob(0.95, 1.18, 0.09, 0.9, hex(0x183f26)),
            ],
            line: (rgba(134, 239, 172, 0.22), 1.0),
            ink: Some(hex(0xecfdf3)),
            ..base("forest", "Forest", 0x12251a)
        },
        Theme {
            glass: Some((rgba(22, 48, 84, 0.55), rgba(18, 38, 68, 0.7))),
            dots: vec![(0.14, 0.26, 1.6, white(1.0)), (0.36, 0.62, 1.2, white(0.8)), (0.58, 0.20, 1.8, white(1.0)), (0.74, 0.58, 1.2, white(0.8)), (0.90, 0.30, 1.6, white(1.0))],
            ground: Some((3.0, rgba(232, 241, 251, 0.85))),
            line: (rgba(224, 242, 254, 0.35), 1.0),
            ..base("snow", "Snow", 0x2c4a6e)
        },
        Theme { bg: (hex(0x2b2b33), hex(0x2b2b33)), line: (rgba(247, 168, 196, 0.25), 1.0), ..base("cat", "Cat", 0x3a3245) },
        Theme {
            bg: (hex(0x0b0a24), hex(0x0b0a24)),
            blobs: vec![blob(0.18, 0.2, 0.6, 1.4, rgba(124, 58, 237, 0.75)), blob(0.85, 0.85, 0.55, 1.4, rgba(219, 39, 119, 0.55))],
            dots: stars(white(1.0)),
            line: (rgba(196, 181, 253, 0.3), 1.0),
            ..base("galaxy", "Galaxy", 0x15122e)
        },
        Theme {
            bg: (hex(0x07121c), hex(0x07121c)),
            blobs: vec![blob(0.2, 1.2, 0.5, 1.6, rgba(13, 148, 136, 0.8)), blob(0.55, 1.3, 0.45, 1.6, rgba(34, 197, 94, 0.55)), blob(0.88, 1.1, 0.5, 1.6, rgba(109, 40, 217, 0.75))],
            line: (rgba(94, 234, 212, 0.28), 1.0),
            ..base("aurora", "Aurora", 0x0a1a22)
        },
        Theme {
            bg: (hex(0x0a3a5c), hex(0x062a44)),
            blobs: vec![blob(0.2, 1.04, 0.6, 0.35, rgba(56, 189, 248, 0.45)), blob(0.78, 1.06, 0.55, 0.35, rgba(186, 230, 253, 0.3))],
            line: (rgba(125, 211, 252, 0.3), 1.0),
            ..base("ocean", "Ocean", 0x082136)
        },
        Theme {
            bg: (hex(0xfff1f6), hex(0xfde4ee)),
            dots: vec![(0.70, 0.22, 2.0, hex(0xf9a8d4)), (0.82, 0.70, 1.6, hex(0xfbcfe8)), (0.58, 0.78, 1.8, hex(0xf9a8d4)), (0.92, 0.34, 1.5, hex(0xf472b6)), (0.40, 0.18, 1.4, hex(0xfbcfe8))],
            line: (rgba(244, 114, 182, 0.45), 1.0),
            ink: Some(hex(0x831843)),
            ..base("nody", "Nody", 0xf6c9d9)
        },
        Theme {
            bg: (hex(0x1c0a05), hex(0x1c0a05)),
            blobs: vec![blob(0.15, 1.35, 0.45, 1.3, rgba(249, 115, 22, 0.85)), blob(0.75, 1.4, 0.5, 1.3, rgba(220, 38, 38, 0.75))],
            line: (rgba(253, 186, 116, 0.3), 1.0),
            ..base("lava", "Lava", 0x1f0e08)
        },
        Theme { bg: (hex(0x0d1410), hex(0x0d1410)), line: (hex(0x4ade80), 2.0), ink: Some(hex(0x4ade80)), radius: Some(5.0), ..base("retro", "Retro", 0x0b0f0c) },
    ]
}

pub fn by_id(id: &str) -> Option<Theme> {
    all().into_iter().find(|t| t.id == id).filter(|t| t.id != "original")
}
