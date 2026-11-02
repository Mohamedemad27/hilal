//! Direct2D + DirectWrite drawing on the GPU, shown through DirectComposition. Each frame
//! is presented with vsync, so animation runs at the monitor's refresh rate.
//! All coordinates are in dp (DIPs); the device context's DPI maps them to pixels.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use windows::core::{Interface, Result, HSTRING};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{HMODULE, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

pub type Color = [f32; 4];

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];
pub const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

pub fn white(a: f32) -> Color {
    [1.0, 1.0, 1.0, a]
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    /// Natural start: left for Latin text, right for Arabic/Hebrew.
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn centered(cx: f32, cy: f32, w: f32, h: f32) -> Self {
        Self::new(cx - w / 2.0, cy - h / 2.0, w, h)
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn grow(&self, d: f32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.x + self.w, bottom: self.y + self.h }
    }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

pub struct Image {
    brush: ID2D1BitmapBrush,
    pub w: f32,
    pub h: f32,
}

/// One window's drawing surface: a composition swapchain shown through DirectComposition,
/// plus its own copy of the desktop region behind it (for the glass).
struct Surface {
    swapchain: IDXGISwapChain1,
    _dcomp: (IDCompositionDevice, IDCompositionTarget, IDCompositionVisual),
    target: Option<ID2D1Bitmap1>,
    px_w: i32,
    px_h: i32,
    /// Window position on the desktop, physical px.
    pos: (i32, i32),
    /// The desktop behind this window (texture, its D2D view) and where it's copied from.
    behind: Option<(ID3D11Texture2D, ID2D1Bitmap1, D3D11_BOX)>,
    /// The backdrop needs a whole copy, not just the parts of the screen that changed:
    /// it's new (blank), or its source box moved.
    fresh: bool,
    /// Present waits for vblank. Only the notch paces the frame loop this way.
    vsync: bool,
}

/// The notch's surface; others (like the FPS overlay) come after it.
pub const MAIN: usize = 0;

pub struct Gfx {
    factory: ID2D1Factory1,
    dwrite: IDWriteFactory,
    d3d: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dxgi_factory: IDXGIFactory2,
    /// The app's single Desktop Duplication session (Windows allows one per output), and
    /// the output's desktop origin. Every surface's backdrop is copied out of it.
    dup: Option<(IDXGIOutputDuplication, (i32, i32))>,
    /// Blur -> saturation chain fed by a backdrop bitmap.
    glass_fx: Option<(ID2D1Effect, ID2D1Effect)>,
    rt: ID2D1DeviceContext,
    surfaces: Vec<Surface>,
    current: usize,
    brush: ID2D1SolidColorBrush,
    pub scale: f32,
    /// Bumped if the device is ever rebuilt; images made before it are invalid.
    pub generation: u64,
    /// Multiplies every color's alpha; used to fade page content in and out.
    pub fade: Cell<f32>,
    /// The (shift, scale, origin x) last set with `transform`, which `scale_at` builds on.
    base: Cell<(f32, f32, f32)>,
    /// When set, the next presented frame is also saved to this PNG.
    pub dump_to: Option<std::path::PathBuf>,
    /// Desktop frames whose captured content changed (the on-screen FPS counts these),
    /// and the bounding box of what changed since the owner last cleared it.
    pub screen_frames: u64,
    /// When every backdrop was last copied whole (a periodic refresh, so a still area of
    /// the screen can never leave the glass stuck on an old or blank image).
    last_full: std::time::Instant,
    /// The whole desktop as last captured. Windows only hands over a new image when the
    /// screen changes, so a backdrop that needs filling (new, moved, refreshed) takes it
    /// from here instead of waiting for the screen to change.
    desk: Option<ID3D11Texture2D>,
    pub screen_dirty: Option<(i32, i32, i32, i32)>,
    pub presents: u64,
    font: String,
    icon_font: String,
    /// Inter, loaded from memory: (family name, its collection).
    display: Option<(String, IDWriteFontCollection1)>,
    /// The in-memory loader Inter was registered with; unregistered on drop, before the
    /// fonts that use it are gone (leaving it registered crashes DirectWrite at exit).
    font_loader: Option<IDWriteInMemoryFontFileLoader>,
    /// Typography with tabular figures, for the display face.
    tabular: Option<IDWriteTypography>,
    formats: RefCell<HashMap<(u32, u32, u8), IDWriteTextFormat>>,
}

impl Gfx {
    /// Creates the device and the notch's surface (`MAIN`).
    pub fn new(hwnd: HWND, width_dp: f32, height_dp: f32, scale: f32, pos: (i32, i32)) -> Result<Self> {
        unsafe {
            let d3d = create_d3d(D3D_DRIVER_TYPE_HARDWARE).or_else(|_| create_d3d(D3D_DRIVER_TYPE_WARP))?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let rt = factory.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let fonts = std::path::Path::new(r"C:\Windows\Fonts");
            let font = if fonts.join("SegUIVar.ttf").exists() { "Segoe UI Variable Text" } else { "Segoe UI" };
            let icon_font = if fonts.join("SegoeIcons.ttf").exists() { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
            let brush = rt.CreateSolidColorBrush(&color(WHITE), None)?;
            let ctx = d3d.GetImmediateContext()?;
            let (display, font_loader) = match try_load_inter(&dwrite) {
                Ok((name, coll, loader)) => (Some((name, coll)), Some(loader)),
                Err(_) => (None, None),
            };
            let tabular = dwrite.CreateTypography().ok().filter(|t| {
                let tnum = DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, parameter: 1 };
                t.AddFontFeature(tnum).is_ok()
            });

            let mut g = Self {
                factory,
                dwrite,
                d3d,
                ctx,
                dxgi_factory,
                dup: None,
                glass_fx: None,
                rt,
                surfaces: Vec::new(),
                current: MAIN,
                brush,
                scale,
                generation: 0,
                fade: Cell::new(1.0),
                base: Cell::new((0.0, 1.0, 0.0)),
                dump_to: None,
                screen_frames: 0,
                last_full: std::time::Instant::now(),
                desk: None,
                screen_dirty: None,
                presents: 0,
                font: font.into(),
                icon_font: icon_font.into(),
                display,
                font_loader,
                tabular,
                formats: RefCell::new(HashMap::new()),
            };
            g.add_surface(hwnd, width_dp, height_dp, pos, true)?;
            Ok(g)
        }
    }

    /// Adds a window to draw into, on the same device (so it shares the glass capture).
    pub fn add_surface(&mut self, hwnd: HWND, width_dp: f32, height_dp: f32, pos: (i32, i32), vsync: bool) -> Result<usize> {
        let px_w = (width_dp * self.scale).round() as i32;
        let px_h = (height_dp * self.scale).round() as i32;
        unsafe {
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: px_w as u32,
                Height: px_h as u32,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swapchain = self.dxgi_factory.CreateSwapChainForComposition(&self.d3d, &desc, None)?;
            let dxgi: IDXGIDevice = self.d3d.cast()?;
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swapchain)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;
            self.surfaces.push(Surface { swapchain, _dcomp: (dcomp, target, visual), target: None, px_w, px_h, pos, behind: None, fresh: true, vsync });
        }
        let id = self.surfaces.len() - 1;
        self.bind(id)?;
        if self.dup.is_some() {
            let _ = self.make_behind(id);
        }
        Ok(id)
    }

    /// Wraps the surface's back buffer as a D2D target bitmap.
    fn bind(&mut self, id: usize) -> Result<()> {
        unsafe {
            let surface: IDXGISurface = self.surfaces[id].swapchain.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0 * self.scale,
                dpiY: 96.0 * self.scale,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            self.surfaces[id].target = Some(self.rt.CreateBitmapFromDxgiSurface(&surface, Some(&props))?);
        }
        Ok(())
    }

    /// Resizes a surface (e.g. for a new DPI).
    pub fn resize(&mut self, id: usize, width_dp: f32, height_dp: f32, scale: f32, pos: (i32, i32)) -> Result<()> {
        self.scale = scale;
        let s = &mut self.surfaces[id];
        s.px_w = (width_dp * scale).round() as i32;
        s.px_h = (height_dp * scale).round() as i32;
        s.pos = pos;
        s.behind = None;
        s.target = None;
        unsafe {
            self.rt.SetTarget(None);
            s.swapchain.ResizeBuffers(2, s.px_w as u32, s.px_h as u32, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SWAP_CHAIN_FLAG(0))?;
        }
        self.bind(id)?;
        if self.dup.is_some() {
            let _ = self.make_behind(id);
        }
        Ok(())
    }

    /// The window moved: capture the desktop from its new spot.
    pub fn move_surface(&mut self, id: usize, pos: (i32, i32)) {
        self.surfaces[id].pos = pos;
        if self.dup.is_some() {
            let _ = self.make_behind(id);
        }
    }

    /// Draws what follows (until `pop_layer`) as one piece: faded to `opacity`, scaled by
    /// `k` around (`cx`, `cy`) and moved down by `dy`. For a whole window coming and going.
    pub fn push_layer(&self, opacity: f32, k: f32, cx: f32, cy: f32, dy: f32) {
        let m = Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: cx - k * cx, M32: cy - k * cy + dy };
        unsafe {
            self.rt.SetTransform(&m);
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                geometricMask: std::mem::ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 },
                opacity: opacity.clamp(0.0, 1.0),
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            self.rt.PushLayer(&params, None);
        }
    }

    pub fn pop_layer(&self) {
        unsafe {
            self.rt.PopLayer();
            self.rt.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 });
        }
    }

    pub fn begin(&mut self, id: usize) -> Result<()> {
        self.current = id;
        unsafe {
            if let Some(target) = &self.surfaces[id].target {
                self.rt.SetTarget(target);
            }
            self.rt.SetDpi(96.0 * self.scale, 96.0 * self.scale);
            self.rt.BeginDraw();
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        }
        self.fade.set(1.0);
        self.offset(0.0);
        Ok(())
    }

    /// Shifts everything drawn after this call right by `dx` dp.
    pub fn offset(&self, dx: f32) {
        self.transform(dx, 1.0, 0.0);
    }

    /// Rotates everything drawn after this call by `deg` (clockwise) around (`cx`, `cy`),
    /// then shifts it right by `dx`.
    pub fn rotate(&self, dx: f32, cx: f32, cy: f32, deg: f32) {
        let (s, c) = deg.to_radians().sin_cos();
        let m = Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: cx - c * cx + s * cy + dx, M32: cy - s * cx - c * cy };
        unsafe { self.rt.SetTransform(&m) };
    }

    /// Scales what's drawn next by `k` around (`cx`, `cy`), on top of the current
    /// `transform`. Undo with `unscale`.
    pub fn scale_at(&self, cx: f32, cy: f32, k: f32) {
        let (dx, bk, ox) = self.base.get();
        let m = Matrix3x2 { M11: k * bk, M12: 0.0, M21: 0.0, M22: k * bk, M31: bk * (cx - k * cx + dx - ox) + ox, M32: bk * (cy - k * cy) };
        unsafe { self.rt.SetTransform(&m) };
    }

    /// Back to the current `transform` after `scale_at`.
    pub fn unscale(&self) {
        let (dx, k, ox) = self.base.get();
        self.transform(dx, k, ox);
    }

    /// Shifts everything drawn after this call by (`dx`, `dy`) dp. (Used by the installer.)
    #[allow(dead_code)]
    pub fn translate(&self, dx: f32, dy: f32) {
        let m = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: dx, M32: dy };
        unsafe { self.rt.SetTransform(&m) };
    }

    /// Shifts by `dx`, then scales by `k` around (`origin_x`, 0): content grows out of the
    /// top edge together with the notch.
    pub fn transform(&self, dx: f32, k: f32, origin_x: f32) {
        self.base.set((dx, k, origin_x));
        let m = Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: k * (dx - origin_x) + origin_x, M32: 0.0 };
        unsafe { self.rt.SetTransform(&m) };
    }

    // ----- liquid glass -------------------------------------------------------------

    /// Starts capturing the desktop behind every surface. Returns false where that isn't
    /// possible (then the glass falls back to a dark tint).
    pub fn enable_backdrop(&mut self) -> bool {
        self.dup = unsafe { self.make_dup().ok() };
        if self.dup.is_some() {
            for id in 0..self.surfaces.len() {
                let _ = self.make_behind(id);
            }
            if self.glass_fx.is_none() {
                self.glass_fx = unsafe { self.make_glass_fx().ok() };
            }
        }
        self.has_backdrop()
    }

    pub fn disable_backdrop(&mut self) {
        self.dup = None;
        self.desk = None;
        for s in &mut self.surfaces {
            s.behind = None;
        }
    }

    unsafe fn make_dup(&self) -> Result<(IDXGIOutputDuplication, (i32, i32))> {
        let dxgi: IDXGIDevice = self.d3d.cast()?;
        let output = dxgi.GetAdapter()?.EnumOutputs(0)?;
        let desc = output.GetDesc()?;
        let dup = output.cast::<IDXGIOutput1>()?.DuplicateOutput(&self.d3d)?;
        Ok((dup, (desc.DesktopCoordinates.left, desc.DesktopCoordinates.top)))
    }

    /// Gives a surface its own texture of the desktop region behind it.
    fn make_behind(&mut self, id: usize) -> Result<()> {
        let Some((_, (ox, oy))) = self.dup else { return Ok(()) };
        self.surfaces[id].fresh = true;
        let s = &self.surfaces[id];
        let src = D3D11_BOX {
            left: (s.pos.0 - ox).max(0) as u32,
            top: (s.pos.1 - oy).max(0) as u32,
            front: 0,
            right: (s.pos.0 - ox + s.px_w).max(0) as u32,
            bottom: (s.pos.1 - oy + s.px_h).max(0) as u32,
            back: 1,
        };
        // Same size as before: just move the source box.
        if let Some((tex, bmp, _)) = s.behind.clone() {
            self.surfaces[id].behind = Some((tex, bmp, src));
            return Ok(());
        }
        unsafe {
            let tdesc = D3D11_TEXTURE2D_DESC {
                Width: s.px_w as u32,
                Height: s.px_h as u32,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                ..Default::default()
            };
            let mut tex = None;
            self.d3d.CreateTexture2D(&tdesc, None, Some(&mut tex))?;
            let tex = tex.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0 * self.scale,
                dpiY: 96.0 * self.scale,
                bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                ..Default::default()
            };
            let bitmap = self.rt.CreateBitmapFromDxgiSurface(&tex.cast::<IDXGISurface>()?, Some(&props))?;
            self.surfaces[id].behind = Some((tex, bitmap, src));
        }
        Ok(())
    }

    unsafe fn make_glass_fx(&self) -> Result<(ID2D1Effect, ID2D1Effect)> {
        let blur = self.rt.CreateEffect(&CLSID_D2D1GaussianBlur)?;
        blur.SetValue(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &9.0f32.to_le_bytes())?;
        blur.SetValue(D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_BORDER_MODE_HARD.0 as u32).to_le_bytes())?;
        blur.SetValue(D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_GAUSSIANBLUR_OPTIMIZATION_QUALITY.0 as u32).to_le_bytes())?;
        let sat = self.rt.CreateEffect(&CLSID_D2D1Saturation)?;
        sat.SetValue(D2D1_SATURATION_PROP_SATURATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &1.6f32.to_le_bytes())?;
        sat.SetInput(0, &blur.GetOutput()?, true);
        Ok((blur, sat))
    }

    /// Pulls the newest desktop image if the screen changed. Returns a bit per surface
    /// (1 << id) whose backdrop changed.
    pub fn update_backdrop(&mut self) -> u32 {
        let changed = self.pull_frame();
        changed | self.fill_fresh()
    }

    /// Backdrops that need a whole new image take it from the last desktop capture; every
    /// two seconds all of them do, so none can stay stuck on an old or blank image.
    fn fill_fresh(&mut self) -> u32 {
        if self.last_full.elapsed() > std::time::Duration::from_secs(2) {
            self.last_full = std::time::Instant::now();
            for s in &mut self.surfaces {
                s.fresh = true;
            }
        }
        let Some(desk) = &self.desk else { return 0 };
        let mut changed = 0u32;
        for (id, s) in self.surfaces.iter_mut().enumerate() {
            if let (true, Some((tex, _, b))) = (s.fresh, &s.behind) {
                unsafe { self.ctx.CopySubresourceRegion(tex, 0, 0, 0, 0, desk, 0, Some(b)) };
                s.fresh = false;
                changed |= 1 << id;
            }
        }
        changed
    }

    /// Takes the newest desktop image if the screen changed: keeps the whole of it, and
    /// updates the backdrops where it changed behind them.
    fn pull_frame(&mut self) -> u32 {
        let Some((dup, _)) = self.dup.clone() else { return 0 };
        unsafe {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res = None;
            match dup.AcquireNextFrame(0, &mut info, &mut res) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return 0,
                Err(e) => {
                    // Access lost (mode change, secure desktop): drop it; the owner retries.
                    if let Some(path) = std::env::var_os("HILAL_FPS") {
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                            let _ = writeln!(f, "  [glass] capture lost: {e}");
                        }
                    }
                    self.disable_backdrop();
                    return 0;
                }
            }
            let mut changed = 0u32;
            // Keep the whole new image.
            if info.LastPresentTime != 0 {
                if let Some(frame) = res.as_ref().and_then(|r| r.cast::<ID3D11Texture2D>().ok()) {
                    if self.desk.is_none() {
                        let mut d = D3D11_TEXTURE2D_DESC::default();
                        frame.GetDesc(&mut d);
                        d.Usage = D3D11_USAGE_DEFAULT;
                        d.BindFlags = 0;
                        d.CPUAccessFlags = 0;
                        d.MiscFlags = 0;
                        let mut t = None;
                        if self.d3d.CreateTexture2D(&d, None, Some(&mut t)).is_ok() {
                            self.desk = t;
                        }
                    }
                    if let Some(desk) = &self.desk {
                        self.ctx.CopyResource(desk, &frame);
                    }
                }
            }
            if info.LastPresentTime != 0 && info.TotalMetadataBufferSize > 0 {
                let mut rects = [RECT::default(); 64];
                let mut needed = 0u32;
                let ok = dup.GetFrameDirtyRects(std::mem::size_of_val(&rects) as u32, rects.as_mut_ptr(), &mut needed).is_ok();
                let n = if ok { (needed as usize / std::mem::size_of::<RECT>()).min(rects.len()) } else { 0 };
                // Frames where captured content changed are the on-screen FPS. Our own
                // windows are excluded from capture, so their redraws leave no dirty rects.
                if n > 0 || !ok {
                    self.screen_frames += info.AccumulatedFrames.max(1) as u64;
                    for r in &rects[..n] {
                        self.screen_dirty = Some(match self.screen_dirty {
                            None => (r.left, r.top, r.right, r.bottom),
                            Some(d) => (d.0.min(r.left), d.1.min(r.top), d.2.max(r.right), d.3.max(r.bottom)),
                        });
                    }
                    let frame = res.and_then(|r| r.cast::<ID3D11Texture2D>().ok());
                    for (id, s) in self.surfaces.iter().enumerate() {
                        let (Some((tex, _, b)), Some(frame)) = (&s.behind, &frame) else { continue };
                        // Only re-copy (and redraw) what changed behind this window.
                        let behind = !ok
                            || rects[..n].iter().any(|r| r.left < b.right as i32 && r.right > b.left as i32 && r.top < b.bottom as i32 && r.bottom > b.top as i32);
                        if behind {
                            self.ctx.CopySubresourceRegion(tex, 0, 0, 0, 0, frame, 0, Some(b));
                            changed |= 1 << id;
                        }
                    }
                }
            }
            let _ = dup.ReleaseFrame();
            changed
        }
    }

    /// The display face's family, when Inter loaded (else text falls back to Segoe UI).
    pub fn display_family(&self) -> Option<&str> {
        self.display.as_ref().map(|(name, _)| name.as_str())
    }

    pub fn has_backdrop(&self) -> bool {
        self.dup.is_some() && self.glass_fx.is_some()
    }

    /// The notch as Liquid Glass.
    pub fn glass(&self, cx: f32, w: f32, h: f32, r: f32, e: f32) {
        let Some(geo) = self.notch_geo(cx, w, h, r, e) else { return };
        let k = 1.0 + 0.05 * (h / 140.0).min(1.0);
        self.glass_fill(&geo.into(), Rect::new(cx - w / 2.0, 0.0, w, h), (cx, 0.0, k));
        // A soft sheen inside the bottom edge.
        if h > 40.0 {
            if let Some(inner) = self.notch_geo(cx, w - 6.0, h - 3.0, (r - 3.0).max(0.0), 0.0) {
                let rim = Rect::new(cx - w / 2.0, 0.0, w, h);
                self.stroke_geo_gradient(&inner.into(), rim, 2.5, &[(0.0, white(0.0)), (0.7, white(0.03)), (1.0, white(0.10))]);
            }
        }
    }

    /// A free-floating rounded pane of Liquid Glass.
    pub fn glass_rrect(&self, r: Rect, radius: f32) {
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        let Ok(geo) = (unsafe { self.f().CreateRoundedRectangleGeometry(&rr) }) else { return };
        self.glass_fill(&geo.into(), r, (r.x + r.w / 2.0, r.y + r.h / 2.0, 1.04));
        let inner = D2D1_ROUNDED_RECT { rect: r.grow(-2.0).d2d(), radiusX: radius - 2.0, radiusY: radius - 2.0 };
        if let Ok(inner) = unsafe { self.f().CreateRoundedRectangleGeometry(&inner) } {
            self.stroke_geo_gradient(&inner.into(), r, 2.0, &[(0.0, white(0.06)), (0.5, white(0.0)), (1.0, white(0.07))]);
        }
    }

    /// Fills `geo` with the blurred, saturated, slightly magnified desktop behind the
    /// current surface (lens = center x, center y, zoom), a dark tint so white text stays
    /// legible, and light catching the rim.
    fn glass_fill(&self, geo: &ID2D1Geometry, bounds: Rect, lens: (f32, f32, f32)) {
        let s = &self.surfaces[self.current];
        unsafe {
            let backdrop = s.behind.as_ref().zip(self.glass_fx.as_ref()).filter(|_| self.dup.is_some());
            if let Some(((_, bitmap, _), (blur, sat))) = backdrop {
                blur.SetInput(0, bitmap, true);
                if let Ok(img) = sat.GetOutput() {
                    let props = D2D1_IMAGE_BRUSH_PROPERTIES {
                        sourceRectangle: D2D_RECT_F { left: 0.0, top: 0.0, right: s.px_w as f32 / self.scale, bottom: s.px_h as f32 / self.scale },
                        extendModeX: D2D1_EXTEND_MODE_CLAMP,
                        extendModeY: D2D1_EXTEND_MODE_CLAMP,
                        interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
                    };
                    if let Ok(brush) = self.rt.CreateImageBrush(&img, &props, None) {
                        let (cx, cy, k) = lens;
                        brush.SetTransform(&Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: cx * (1.0 - k), M32: cy * (1.0 - k) });
                        self.rt.FillGeometry(geo, &brush, None);
                    }
                }
                self.fill_geo_gradient(geo, bounds, &[(0.0, [0.02, 0.02, 0.04, 0.58]), (1.0, [0.02, 0.02, 0.05, 0.72])], true);
            } else {
                self.fill_geo_gradient(geo, bounds, &[(0.0, [0.05, 0.05, 0.07, 0.90]), (1.0, [0.02, 0.02, 0.03, 0.94])], true);
            }
        }
        // Light on the rim: brightest along the bottom, fading up the sides.
        self.stroke_geo_gradient(geo, bounds, 1.2, &[(0.0, white(0.06)), (0.55, white(0.16)), (1.0, white(0.42))]);
    }

    fn gradient_brush(&self, r: Rect, stops: &[(f32, Color)], vertical: bool) -> Option<ID2D1LinearGradientBrush> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(p, c)| D2D1_GRADIENT_STOP { position: *p, color: color([c[0], c[1], c[2], c[3] * self.fade.get()]) })
            .collect();
        unsafe {
            let coll = self.r().CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).ok()?;
            let (a, b) = if vertical { (pt(r.x, r.y), pt(r.x, r.y + r.h)) } else { (pt(r.x, r.y), pt(r.x + r.w, r.y)) };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: a, endPoint: b };
            self.rt.CreateLinearGradientBrush(&props, None, &coll).ok()
        }
    }

    fn fill_geo_gradient(&self, geo: &ID2D1Geometry, r: Rect, stops: &[(f32, Color)], vertical: bool) {
        if let Some(brush) = self.gradient_brush(r, stops, vertical) {
            unsafe { self.rt.FillGeometry(geo, &brush, None) };
        }
    }

    fn stroke_geo_gradient(&self, geo: &ID2D1Geometry, r: Rect, width: f32, stops: &[(f32, Color)]) {
        if let Some(brush) = self.gradient_brush(r, stops, true) {
            unsafe { self.rt.DrawGeometry(geo, &brush, width, None) };
        }
    }

    /// Saves what the current surface drew as a PNG, composited over dark gray. Our
    /// windows are excluded from screen capture, so this is how to look at them.
    pub fn dump(&self, path: &std::path::Path) -> Result<()> {
        unsafe {
            let back: ID3D11Texture2D = self.surfaces[self.current].swapchain.GetBuffer(0)?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            back.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            self.d3d.CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging = staging.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
            self.ctx.CopyResource(&staging, &back);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let (w, h) = (desc.Width as usize, desc.Height as usize);
            let mut rgb = Vec::with_capacity(w * h * 3);
            for y in 0..h {
                let row = std::slice::from_raw_parts((map.pData as *const u8).add(y * map.RowPitch as usize), w * 4);
                for p in row.chunks_exact(4) {
                    let a = p[3] as f32 / 255.0;
                    let bg = 48.0 * (1.0 - a);
                    rgb.extend_from_slice(&[(p[2] as f32 + bg) as u8, (p[1] as f32 + bg) as u8, (p[0] as f32 + bg) as u8]);
                }
            }
            self.ctx.Unmap(&staging, 0);
            image::save_buffer(path, &rgb, w as u32, h as u32, image::ExtendedColorType::Rgb8)
                .map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
        }
    }

    /// Finishes the frame and shows it. On the notch's surface this blocks until the next
    /// vertical blank, which is what paces the animation to the display.
    pub fn present(&mut self) -> Result<()> {
        unsafe {
            self.rt.EndDraw(None, None)?;
            // The back buffer must be read before Present flips it away.
            if let Some(path) = self.dump_to.take() {
                let _ = self.dump(&path);
            }
            let s = &self.surfaces[self.current];
            if s.vsync {
                self.presents += 1;
                // HILAL_NO_VSYNC=1 presents without waiting, the way some drivers behave,
                // to test the frame loop's own pacing.
                static NO_VSYNC: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                let sync = if *NO_VSYNC.get_or_init(|| std::env::var_os("HILAL_NO_VSYNC").is_some()) { 0 } else { 1 };
                s.swapchain.Present(sync, DXGI_PRESENT(0)).ok()
            } else {
                // Side surfaces must never hold up the notch's vsync-paced loop: if their
                // buffers are still busy, this frame is skipped (the caller retries).
                s.swapchain.Present(0, DXGI_PRESENT_DO_NOT_WAIT).ok()
            }
        }
    }

    fn solid(&self, c: Color) -> &ID2D1SolidColorBrush {
        let c = [c[0], c[1], c[2], c[3] * self.fade.get()];
        unsafe { self.brush.SetColor(&color(c)) };
        &self.brush
    }

    /// The D2D 1.0 views of the factory and context; their 1.1 versions shadow some
    /// methods with variants that take different structs.
    fn f(&self) -> &ID2D1Factory {
        &self.factory
    }

    fn r(&self) -> &ID2D1RenderTarget {
        &self.rt
    }

    // ----- shapes -------------------------------------------------------------------

    pub fn fill_rrect(&self, r: Rect, radius: f32, c: Color) {
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.rt.FillRoundedRectangle(&rr, self.solid(c)) };
    }

    pub fn stroke_rrect(&self, r: Rect, radius: f32, width: f32, c: Color, dashed: bool) {
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        let style = dashed.then(|| self.dash_style()).flatten();
        unsafe { self.rt.DrawRoundedRectangle(&rr, self.solid(c), width, style.as_ref()) };
    }

    fn dash_style(&self) -> Option<ID2D1StrokeStyle> {
        let props = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            dashCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            miterLimit: 10.0,
            dashStyle: D2D1_DASH_STYLE_CUSTOM,
            dashOffset: 0.0,
        };
        unsafe { self.f().CreateStrokeStyle(&props, Some(&[3.0, 2.5])).ok() }
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, r: f32, c: Color) {
        self.fill_ellipse(cx, cy, r, r, c);
    }

    pub fn fill_ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32, c: Color) {
        let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: rx, radiusY: ry };
        unsafe { self.rt.FillEllipse(&e, self.solid(c)) };
    }

    pub fn stroke_circle(&self, cx: f32, cy: f32, r: f32, width: f32, c: Color) {
        let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: r, radiusY: r };
        unsafe { self.rt.DrawEllipse(&e, self.solid(c), width, None) };
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, c: Color) {
        unsafe { self.rt.DrawLine(pt(x1, y1), pt(x2, y2), self.solid(c), width, None) };
    }

    /// Ring arc, angles in degrees clockwise from 12 o'clock.
    pub fn arc(&self, cx: f32, cy: f32, r: f32, start: f32, sweep: f32, width: f32, c: Color) {
        if sweep <= 0.01 {
            return;
        }
        if sweep >= 359.99 {
            return self.stroke_circle(cx, cy, r, width, c);
        }
        let at = |deg: f32| {
            let a = (deg - 90.0).to_radians();
            pt(cx + r * a.cos(), cy + r * a.sin())
        };
        let Some(geo) = self.path(|sink| unsafe {
            sink.BeginFigure(at(start), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: at(start + sweep),
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if sweep > 180.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }) else {
            return;
        };
        let props = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            ..Default::default()
        };
        let style = unsafe { self.f().CreateStrokeStyle(&props, None).ok() };
        unsafe { self.rt.DrawGeometry(&geo, self.solid(c), width, style.as_ref()) };
    }

    /// A smooth line chart over `values` (0..1) inside `r`, with a fading fill under it.
    pub fn sparkline(&self, r: Rect, values: &[f32], c: Color) {
        if values.len() < 2 {
            return;
        }
        let step = r.w / (values.len() - 1) as f32;
        let p = |i: usize| pt(r.x + i as f32 * step, r.y + r.h * (1.0 - values[i].clamp(0.0, 1.0)));
        // Catmull-Rom through the samples, as cubic beziers.
        let curve = |s: &ID2D1GeometrySink| unsafe {
            for i in 0..values.len() - 1 {
                let p0 = p(i.saturating_sub(1));
                let (p1, p2) = (p(i), p(i + 1));
                let p3 = p((i + 2).min(values.len() - 1));
                s.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: pt(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0),
                    point2: pt(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0),
                    point3: p2,
                });
            }
        };
        let fill = self.path(|s| unsafe {
            s.BeginFigure(pt(r.x, r.y + r.h), D2D1_FIGURE_BEGIN_FILLED);
            s.AddLine(p(0));
            curve(s);
            s.AddLine(pt(r.x + r.w, r.y + r.h));
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        if let (Some(geo), Some(brush)) = (fill, self.gradient_brush(r, &[(0.0, [c[0], c[1], c[2], 0.22]), (1.0, [c[0], c[1], c[2], 0.0])], true)) {
            unsafe { self.rt.FillGeometry(&geo, &brush, None) };
        }
        let line = self.path(|s| unsafe {
            s.BeginFigure(p(0), D2D1_FIGURE_BEGIN_HOLLOW);
            curve(s);
            s.EndFigure(D2D1_FIGURE_END_OPEN);
        });
        if let Some(geo) = line {
            unsafe { self.rt.DrawGeometry(&geo, self.solid(c), 1.6, None) };
        }
    }

    pub fn fill_poly(&self, points: &[(f32, f32)], c: Color) {
        let Some((first, rest)) = points.split_first() else { return };
        let Some(geo) = self.path(|s| unsafe {
            s.BeginFigure(pt(first.0, first.1), D2D1_FIGURE_BEGIN_FILLED);
            for p in rest {
                s.AddLine(pt(p.0, p.1));
            }
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        }) else {
            return;
        };
        unsafe { self.rt.FillGeometry(&geo, self.solid(c), None) };
    }

    fn path(&self, build: impl FnOnce(&ID2D1GeometrySink)) -> Option<ID2D1PathGeometry> {
        unsafe {
            let geo = self.f().CreatePathGeometry().ok()?;
            let sink = geo.Open().ok()?;
            build(&sink);
            sink.Close().ok()?;
            Some(geo)
        }
    }

    /// The notch: a black slab hanging from the top edge, centered on `cx`, with rounded
    /// bottom corners (radius `r`) and concave "ears" (radius `e`) flaring into the top edge.
    pub fn notch(&self, cx: f32, w: f32, h: f32, r: f32, e: f32, c: Color) {
        if let Some(geo) = self.notch_geo(cx, w, h, r, e) {
            unsafe { self.rt.FillGeometry(&geo, self.solid(c), None) };
        }
    }

    fn notch_geo(&self, cx: f32, w: f32, h: f32, r: f32, e: f32) -> Option<ID2D1PathGeometry> {
        let (l, rt) = (cx - w / 2.0, cx + w / 2.0);
        let r = r.min(h / 2.0).min(w / 2.0).max(0.0);
        let e = e.min(h - r).max(0.0);
        let arc = |p: D2D_POINT_2F, rad: f32, cw: bool| D2D1_ARC_SEGMENT {
            point: p,
            size: D2D_SIZE_F { width: rad, height: rad },
            rotationAngle: 0.0,
            sweepDirection: if cw { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
            arcSize: D2D1_ARC_SIZE_SMALL,
        };
        self.path(|s| unsafe {
            s.BeginFigure(pt(l - e, 0.0), D2D1_FIGURE_BEGIN_FILLED);
            s.AddArc(&arc(pt(l, e), e, true));
            s.AddLine(pt(l, h - r));
            s.AddArc(&arc(pt(l + r, h), r, false));
            s.AddLine(pt(rt - r, h));
            s.AddArc(&arc(pt(rt, h - r), r, false));
            s.AddLine(pt(rt, e));
            s.AddArc(&arc(pt(rt + e, 0.0), e, true));
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        })
    }

    pub fn linear(&self, r: Rect, radius: f32, stops: &[(f32, Color)], vertical: bool) {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(p, c)| D2D1_GRADIENT_STOP { position: *p, color: color([c[0], c[1], c[2], c[3] * self.fade.get()]) })
            .collect();
        unsafe {
            let Ok(coll) = self.r().CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else {
                return;
            };
            let (a, b) = if vertical {
                (pt(r.x, r.y), pt(r.x, r.y + r.h))
            } else {
                (pt(r.x, r.y), pt(r.x + r.w, r.y))
            };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: a, endPoint: b };
            let Ok(brush) = self.rt.CreateLinearGradientBrush(&props, None, &coll) else { return };
            let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
            self.rt.FillRoundedRectangle(&rr, &brush);
        }
    }

    pub fn radial(&self, cx: f32, cy: f32, rx: f32, ry: f32, inner: Color) {
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: color([inner[0], inner[1], inner[2], inner[3] * self.fade.get()]) },
            D2D1_GRADIENT_STOP { position: 1.0, color: color([inner[0], inner[1], inner[2], 0.0]) },
        ];
        unsafe {
            let Ok(coll) = self.r().CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else {
                return;
            };
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: pt(cx, cy),
                gradientOriginOffset: pt(0.0, 0.0),
                radiusX: rx,
                radiusY: ry,
            };
            let Ok(brush) = self.rt.CreateRadialGradientBrush(&props, None, &coll) else { return };
            let e = D2D1_ELLIPSE { point: pt(cx, cy), radiusX: rx, radiusY: ry };
            self.rt.FillEllipse(&e, &brush);
        }
    }

    pub fn clip(&self, r: Rect) {
        unsafe { self.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    pub fn unclip(&self) {
        unsafe { self.rt.PopAxisAlignedClip() };
    }

    // ----- images -------------------------------------------------------------------

    /// `bgra` must be premultiplied BGRA, `w * h * 4` bytes.
    pub fn image(&self, bgra: &[u8], w: u32, h: u32) -> Option<Image> {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            let bmp = self
                .r()
                .CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(bgra.as_ptr() as _), w * 4, &props)
                .ok()?;
            let bprops = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            let brush = self.r().CreateBitmapBrush(&bmp, Some(&bprops), None).ok()?;
            Some(Image { brush, w: w as f32, h: h as f32 })
        }
    }

    pub fn draw_image(&self, img: &Image, r: Rect, radius: f32, alpha: f32) {
        let m = Matrix3x2 { M11: r.w / img.w, M12: 0.0, M21: 0.0, M22: r.h / img.h, M31: r.x, M32: r.y };
        unsafe {
            img.brush.SetTransform(&m);
            img.brush.SetOpacity(alpha * self.fade.get());
            let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
            self.rt.FillRoundedRectangle(&rr, &img.brush);
        }
    }

    // ----- text ---------------------------------------------------------------------

    fn format(&self, size: f32, weight: u32, face: Face) -> Option<IDWriteTextFormat> {
        let key = ((size * 10.0) as u32, weight, face as u8);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Some(f.clone());
        }
        let (family, collection): (&str, Option<IDWriteFontCollection>) = match (face, &self.display) {
            (Face::Icon, _) => (&self.icon_font, None),
            (Face::Display, Some((name, coll))) => (name, coll.cast().ok()),
            _ => (&self.font, None),
        };
        let f = unsafe {
            let f = self
                .dwrite
                .CreateTextFormat(
                    &HSTRING::from(family),
                    collection.as_ref(),
                    DWRITE_FONT_WEIGHT(weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    &HSTRING::from("en-us"),
                )
                .ok()?;
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if let Ok(sign) = self.dwrite.CreateEllipsisTrimmingSign(&f) {
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                let _ = f.SetTrimming(&trim, &sign);
            }
            f
        };
        self.formats.borrow_mut().insert(key, f.clone());
        Some(f)
    }

    fn layout(&self, s: &str, size: f32, weight: u32, max_w: f32, align: Align, face: Face) -> Option<IDWriteTextLayout> {
        let f = self.format(size, weight, face)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let l = self.dwrite.CreateTextLayout(&wide, &f, max_w.max(1.0), size * 1.6).ok()?;
            if face == Face::Display {
                // Same-width digits, so a ticking clock doesn't shuffle sideways.
                if let Some(t) = &self.tabular {
                    let _ = l.SetTypography(t, DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 });
                }
            }
            let rtl = is_rtl(s);
            if rtl {
                let _ = l.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);
            }
            let a = match (align, rtl) {
                (Align::Center, _) => DWRITE_TEXT_ALIGNMENT_CENTER,
                // In RTL, LEADING is the right edge.
                (Align::Start, _) => DWRITE_TEXT_ALIGNMENT_LEADING,
                (Align::End, false) => DWRITE_TEXT_ALIGNMENT_TRAILING,
                (Align::End, true) => DWRITE_TEXT_ALIGNMENT_LEADING,
            };
            let _ = l.SetTextAlignment(a);
            Some(l)
        }
    }

    /// Draws one line of text vertically centered on `cy`, inside [x, x+w]. Returns the inked width.
    pub fn text(&self, s: &str, x: f32, cy: f32, w: f32, size: f32, weight: u32, c: Color, align: Align) -> f32 {
        self.draw_layout(s, x, cy, w, size, weight, c, align, Face::Text)
    }

    /// Like `text`, in the display face (Inter, with tabular figures): for the clock and
    /// big numbers.
    pub fn display(&self, s: &str, x: f32, cy: f32, w: f32, size: f32, weight: u32, c: Color, align: Align) -> f32 {
        self.draw_layout(s, x, cy, w, size, weight, c, align, Face::Display)
    }

    pub fn icon(&self, glyph: char, cx: f32, cy: f32, size: f32, c: Color) {
        let w = size * 2.0;
        self.draw_layout(&glyph.to_string(), cx - w / 2.0, cy, w, size, 400, c, Align::Center, Face::Icon);
    }

    fn draw_layout(&self, s: &str, x: f32, cy: f32, w: f32, size: f32, weight: u32, c: Color, align: Align, face: Face) -> f32 {
        let Some(l) = self.layout(s, size, weight, w, align, face) else { return 0.0 };
        unsafe {
            self.rt.DrawTextLayout(pt(x, cy - size * 0.8), &l, self.solid(c), D2D1_DRAW_TEXT_OPTIONS_NONE);
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = l.GetMetrics(&mut m);
            m.width
        }
    }

    /// Draws wrapped text from its top-left corner, at most `max_lines` lines (the last
    /// one ends in "…" if it doesn't all fit). Returns the height used.
    pub fn paragraph(&self, s: &str, x: f32, y: f32, w: f32, size: f32, weight: u32, c: Color, max_lines: usize) -> f32 {
        let Some(l) = self.layout(s, size, weight, w, Align::Start, Face::Text) else { return 0.0 };
        let line = size * 1.42;
        unsafe {
            let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let _ = l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
            let _ = l.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, line, line * 0.78);
            let _ = l.SetMaxHeight(line * max_lines as f32 + 1.0);
            self.rt.DrawTextLayout(pt(x, y), &l, self.solid(c), D2D1_DRAW_TEXT_OPTIONS_CLIP);
            let mut m = DWRITE_TEXT_METRICS::default();
            let _ = l.GetMetrics(&mut m);
            m.height.min(line * max_lines as f32)
        }
    }

    pub fn measure(&self, s: &str, size: f32, weight: u32) -> f32 {
        self.measure_face(s, size, weight, Face::Text)
    }

    pub fn measure_display(&self, s: &str, size: f32, weight: u32) -> f32 {
        self.measure_face(s, size, weight, Face::Display)
    }

    fn measure_face(&self, s: &str, size: f32, weight: u32, face: Face) -> f32 {
        let Some(l) = self.layout(s, size, weight, 4000.0, Align::Start, face) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe {
            let _ = l.GetMetrics(&mut m);
        }
        m.widthIncludingTrailingWhitespace
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        // Everything holding Inter goes first, then its loader is unregistered.
        self.formats.borrow_mut().clear();
        self.display = None;
        if let Some(loader) = self.font_loader.take() {
            unsafe {
                let _ = self.dwrite.UnregisterFontFileLoader(&loader);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Face {
    Text,
    Icon,
    Display,
}

/// Inter (SIL Open Font License, see assets/fonts/Inter-LICENSE.txt), shipped inside the
/// exe and loaded from memory: nothing is installed on the system.
const INTER: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");

/// Loads Inter from memory. Returns the family name to ask for, the collection holding
/// it, and the loader (which must be unregistered before the factory goes away).
unsafe fn try_load_inter(dwrite: &IDWriteFactory) -> std::result::Result<(String, IDWriteFontCollection1, IDWriteInMemoryFontFileLoader), String> {
    let step = |what: &str, e: windows::core::Error| format!("{what}: {e}");
    let f5: IDWriteFactory5 = dwrite.cast().map_err(|e| step("IDWriteFactory5", e))?;
    let loader = f5.CreateInMemoryFontFileLoader().map_err(|e| step("loader", e))?;
    f5.RegisterFontFileLoader(&loader).map_err(|e| step("register loader", e))?;
    let file = loader
        .CreateInMemoryFontFileReference(&f5, INTER.as_ptr() as _, INTER.len() as u32, None)
        .map_err(|e| step("font file", e))?;
    let builder = f5.CreateFontSetBuilder().map_err(|e| step("set builder", e))?;
    builder.AddFontFile(&file).map_err(|e| step("add file", e))?;
    let set = builder.CreateFontSet().map_err(|e| step("font set", e))?;
    let coll = f5.CreateFontCollectionFromFontSet(&set).map_err(|e| step("collection", e))?;
    let mut names = Vec::new();
    for i in 0..coll.GetFontFamilyCount() {
        if let Ok(names_list) = coll.GetFontFamily(i).and_then(|f| f.GetFamilyNames()) {
            let mut buf = [0u16; 64];
            if names_list.GetString(0, &mut buf).is_ok() {
                let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                names.push(String::from_utf16_lossy(&buf[..n]));
            }
        }
    }
    // The variable font shows up as "… Text" and "… Display" families (its optical sizes);
    // the display cut is drawn for large type like the clock, as SF Display is on iOS.
    let pick = names.iter().find(|n| n.contains("Display")).or(names.first()).cloned();
    pick.map(|n| (n, coll.clone(), loader.clone())).ok_or_else(|| format!("no families in the collection ({names:?})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_inter_from_memory() {
        unsafe {
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
            match try_load_inter(&dw) {
                Ok((name, _, _)) => println!("loaded family {name:?}"),
                Err(e) => panic!("Inter didn't load: {e}"),
            }
        }
    }
}

unsafe fn create_d3d(driver: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let mut device = None;
    D3D11CreateDevice(
        None,
        driver,
        HMODULE::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )?;
    device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

fn color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: c[3] }
}

/// True when the first strongly-directional character is Arabic or Hebrew.
pub fn is_rtl(s: &str) -> bool {
    for ch in s.chars() {
        let c = ch as u32;
        if (0x0590..=0x08FF).contains(&c) || (0xFB1D..=0xFEFF).contains(&c) {
            return true;
        }
        if ch.is_alphabetic() {
            return false;
        }
    }
    false
}
