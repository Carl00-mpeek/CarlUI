//! Kar yağarken fare bir süre kıpırdamazsa imleç buzlanır: imlecin kendi
//! şeklini saran buz kabuğu, kenarlarından büyüyen kristaller, alttan sarkan
//! buz sarkıtları. Fare oynayınca (ya da tıklanınca) erir; ekranın geri kalanına
//! dokunulmaz.
//!
//! Tıklamayı geçiren, imlecin hemen altındaki küçük, en üstteki saydam pencerede
//! tek bir bitmap'tir. Windows imleci her zaman en üstte çizdiği için buz imlecin
//! arkasında kalır, imleç buzun içinde görünür. Buz yokken imleç canlı dock'un
//! tikiyle (en sık `CHECK`) yoklanır. Donukken erime gecikmesin diye ayrı bir
//! iş parçacığı zamanlayıcısı (`WATCH`) imleci `POLL` aralığıyla izler ve
//! kıpırdayınca buzu kendisi eritir; canlı dock'un bütün tikini sıklaştırmak
//! (ölçümde dock tek çekirdeğin ~%7'si) gerekmez.

use super::{rng::Rng, window::overlay_window};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use std::{
    cell::RefCell,
    sync::Once,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Foundation::{Numerics::Vector3, TimeSpan},
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::*,
        },
        System::WinRT::Composition::ICompositorDesktopInterop,
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
            WindowsAndMessaging::*,
        },
    },
    UI::Composition::{Desktop::DesktopWindowTarget, *},
};

/// İmleç bu kadar kıpırdamazsa buz gelmeye başlar.
const IDLE: Duration = Duration::from_secs(15);
/// Buz yokken imleç bu sıklıkla yoklanır.
const CHECK: Duration = Duration::from_millis(400);
/// Donukken imleç bu sıklıkla yoklanır: fare oynayınca buz hemen erisin.
const POLL_MS: u32 = 50;
const IN_MS: f32 = 3200.0;
const OUT_MS: f32 = 300.0;

fn ts(ms: f32) -> TimeSpan {
    TimeSpan { Duration: (ms.max(1.0) * 10_000.0) as i64 }
}

fn col(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// İmlecin görünen pikselleri (`w`×`h`, satır satır) ve sıcak noktası.
struct Shape {
    w: usize,
    h: usize,
    on: Vec<bool>,
    hot: (i32, i32),
}

/// Renkli imleç: alfa kanalı. Alfa hiç yoksa (eski 24 bit imleç) maskeye
/// bakılır: maskede siyah olan piksel görünür.
fn from_color(px: &[u8], mask: &[u8]) -> Vec<bool> {
    let any = px.chunks_exact(4).any(|p| p[3] != 0);
    px.chunks_exact(4).enumerate().map(|(i, p)| if any { p[3] > 24 } else { mask.get(i * 4).is_some_and(|m| *m < 128) }).collect()
}

/// Tek renkli imleç: maske iki kat yüksektir; üst yarı AND, alt yarı XOR.
/// AND siyahsa piksel boyanır, XOR beyazsa ekran ters çevrilir (metin imleci
/// böyle çizilir); ikisi de görünür sayılır.
fn from_mono(mask: &[u8], w: usize, h: usize) -> Vec<bool> {
    (0..w * h)
        .map(|i| {
            let and_black = mask.get(i * 4).is_some_and(|v| *v < 128);
            let xor_white = mask.get((w * h + i) * 4).is_some_and(|v| *v >= 128);
            and_black || xor_white
        })
        .collect()
}

/// Her pikselin şekle uzaklığı (piksel; 3-4 pah dönüşümü, öklide yakın).
fn distance(on: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: u32 = u32::MAX / 4;
    let mut d: Vec<u32> = on.iter().map(|&o| if o { 0 } else { INF }).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + 3);
            }
            if y > 0 {
                v = v.min(d[i - w] + 3);
                if x > 0 {
                    v = v.min(d[i - w - 1] + 4);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + 4);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + 3);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + 3);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + 4);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + 4);
                }
            }
            d[i] = v;
        }
    }
    d.into_iter().map(|v| v as f32 / 3.0).collect()
}

/// Bitmap'i 32 bit üstten alta piksellere çevirir.
unsafe fn dib(bmp: HBITMAP) -> Option<(usize, usize, Vec<u8>)> {
    if bmp.is_invalid() {
        return None;
    }
    let mut bm = BITMAP::default();
    if GetObjectW(bmp, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 || w > 512 || h > 1024 {
        return None;
    }
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let n = GetDIBits(dc, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    (n > 0).then_some((w as usize, h as usize, px))
}

/// Görünen imleç: ekran konumu ve tutamağı. İmleç gizliyse (yazarken bazı
/// uygulamalar gizler) `None`.
unsafe fn cursor() -> Option<(POINT, HCURSOR)> {
    let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
    GetCursorInfo(&mut ci).ok()?;
    (ci.flags == CURSOR_SHOWING && !ci.hCursor.is_invalid()).then_some((ci.ptScreenPos, ci.hCursor))
}

/// Fare düğmesi basılı mı?
fn pressed() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } < 0)
}

fn fade_out(comp: &Compositor, vis: &SpriteVisual) -> Result<()> {
    let a = comp.CreateScalarKeyFrameAnimation()?;
    a.InsertExpressionKeyFrame(0.0, h!("this.StartingValue"))?;
    a.InsertKeyFrame(1.0, 0.0)?;
    a.SetDuration(ts(OUT_MS))?;
    vis.StartAnimation(h!("Opacity"), &a)
}

/// Donmuş imleci izleyen zamanlayıcı: kimliği, donduğu konum ve eritilecek görsel.
struct Watch {
    id: usize,
    at: (i32, i32),
    comp: Compositor,
    vis: SpriteVisual,
}

thread_local! {
    static WATCH: RefCell<Option<Watch>> = const { RefCell::new(None) };
}

/// İzlemeyi durdurur; izleme sürüyorsa görseli döner.
fn unwatch() -> Option<Watch> {
    let w = WATCH.with(|c| c.borrow_mut().take())?;
    unsafe {
        let _ = KillTimer(None, w.id);
    }
    Some(w)
}

fn watching() -> bool {
    WATCH.with(|c| c.borrow().is_some())
}

extern "system" fn watch_proc(_: HWND, _: u32, _: usize, _: u32) {
    let Some(at) = WATCH.with(|c| c.borrow().as_ref().map(|w| w.at)) else { return };
    let now = unsafe { cursor() }.map(|(p, _)| (p.x, p.y));
    if now != Some(at) || pressed() {
        if let Some(w) = unwatch() {
            let _ = fade_out(&w.comp, &w.vis);
        }
    }
}

unsafe fn shape(c: HCURSOR) -> Option<Shape> {
    let mut ii = ICONINFO::default();
    GetIconInfo(HICON(c.0), &mut ii).ok()?;
    let mask = dib(ii.hbmMask);
    let color = dib(ii.hbmColor);
    let _ = DeleteObject(ii.hbmMask);
    if !ii.hbmColor.is_invalid() {
        let _ = DeleteObject(ii.hbmColor);
    }
    let (mw, mh, m) = mask?;
    let hot = (ii.xHotspot as i32, ii.yHotspot as i32);
    let s = match color {
        Some((w, h, px)) => Shape { w, h, on: from_color(&px, if mh == h { &m } else { &[] }), hot },
        None => {
            let h = mh / 2;
            Shape { w: mw, h, on: from_mono(&m, mw, h), hot }
        }
    };
    s.on.iter().any(|&o| o).then_some(s)
}

/// Buzun tuvali: imleç şekli ortada (sarkıtlara yer kalsın diye biraz yukarıda).
struct Canvas {
    side: usize,
    /// Şeklin tuvaldeki sol üst köşesi.
    at: (usize, usize),
    /// Her tuval pikselinin şekle uzaklığı.
    dist: Vec<f32>,
    /// İmlecin boyu (piksel); buzun bütün ölçüleri buna göre.
    size: f32,
}

impl Canvas {
    fn new(s: &Shape) -> Self {
        let size = s.w.max(s.h) as f32;
        let side = (size * 2.4).ceil() as usize;
        let at = ((side - s.w) / 2, ((side - s.h) as f32 * 0.38) as usize);
        let mut on = vec![false; side * side];
        for y in 0..s.h {
            for x in 0..s.w {
                on[(at.1 + y) * side + at.0 + x] = s.on[y * s.w + x];
            }
        }
        let dist = distance(&on, side, side);
        Self { side, at, dist, size }
    }

    fn d(&self, x: usize, y: usize) -> f32 {
        self.dist[y.min(self.side - 1) * self.side + x.min(self.side - 1)]
    }

    /// Şekilden dışarı bakan birim yön (uzaklığın eğimi).
    fn out(&self, x: usize, y: usize) -> (f32, f32) {
        let gx = self.d(x + 1, y) - self.d(x.saturating_sub(1), y);
        let gy = self.d(x, y + 1) - self.d(x, y.saturating_sub(1));
        let l = (gx * gx + gy * gy).sqrt();
        if l < 1e-3 {
            (0.0, 1.0)
        } else {
            (gx / l, gy / l)
        }
    }

    fn rim(&self) -> f32 {
        (self.size * 0.13).max(2.5)
    }

    fn glow(&self) -> f32 {
        self.size * 0.22
    }

    /// Buz kabuğu ve hâlesi: PBGRA pikselleri.
    fn crust(&self, rng: &mut Rng) -> Vec<u8> {
        let (rim, glow) = (self.rim(), self.glow());
        let mut px = vec![0u8; self.side * self.side * 4];
        for (i, &d) in self.dist.iter().enumerate() {
            let grain = rng.range(-0.14, 0.14);
            let (c, a) = if d <= 0.0 {
                ((236.0, 244.0, 255.0), 0.55)
            } else if d <= rim {
                let u = d / rim;
                ((232.0, 242.0, 255.0), (0.88 - 0.38 * u * u + grain).clamp(0.0, 1.0))
            } else if d <= rim + glow {
                let u = 1.0 - (d - rim) / glow;
                ((196.0, 222.0, 255.0), 0.24 * u * u)
            } else {
                continue;
            };
            let p = &mut px[i * 4..i * 4 + 4];
            p[0] = (c.2 * a) as u8;
            p[1] = (c.1 * a) as u8;
            p[2] = (c.0 * a) as u8;
            p[3] = (255.0 * a) as u8;
        }
        px
    }

    /// Kabuğun dış kenarındaki pikseller.
    fn edge(&self) -> Vec<(usize, usize)> {
        let rim = self.rim();
        let mut v = Vec::new();
        for y in 0..self.side {
            for x in 0..self.side {
                if (self.d(x, y) - rim).abs() <= 0.7 {
                    v.push((x, y));
                }
            }
        }
        v
    }

    /// Sıcak noktanın tuvaldeki yeri.
    fn hot(&self, s: &Shape) -> (f32, f32) {
        ((self.at.0 as i32 + s.hot.0) as f32, (self.at.1 as i32 + s.hot.1) as f32)
    }
}

/// Eğrelti otu gibi dallanan buz çizgisi.
unsafe fn fern(dc: &ID2D1DeviceContext, b: &ID2D1SolidColorBrush, x: f32, y: f32, ang: f32, len: f32, w: f32, depth: u32, rng: &mut Rng) {
    let (mut x, mut y, mut ang) = (x, y, ang);
    let steps = 4;
    let seg = len / steps as f32;
    for i in 0..steps {
        ang += rng.range(-0.2, 0.2);
        let (nx, ny) = (x + ang.cos() * seg, y + ang.sin() * seg);
        dc.DrawLine(pt(x, y), pt(nx, ny), b, w, None);
        if depth > 0 && i % 2 == 0 {
            for side in [-1.0f32, 1.0] {
                fern(dc, b, nx, ny, ang + side * rng.range(0.6, 0.95), len * 0.36, w * 0.75, depth - 1, rng);
            }
        }
        x = nx;
        y = ny;
    }
}

unsafe fn draw(dc: &ID2D1DeviceContext, cv: &Canvas, rng: &mut Rng) -> Result<()> {
    let n = cv.side as u32;
    let px = cv.crust(rng);
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0,
        dpiY: 96.0,
        ..Default::default()
    };
    let bmp = dc.CreateBitmap(D2D_SIZE_U { width: n, height: n }, Some(px.as_ptr() as _), n * 4, &props)?;
    let full = D2D_RECT_F { left: 0.0, top: 0.0, right: n as f32, bottom: n as f32 };
    dc.DrawBitmap(&bmp, Some(&full), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);

    let edge = cv.edge();
    if edge.is_empty() {
        return Ok(());
    }
    let k = cv.size / 32.0;
    let pick = |rng: &mut Rng| edge[((rng.f() * edge.len() as f32) as usize).min(edge.len() - 1)];

    // Kenardan dışarı büyüyen kristaller.
    let line = dc.CreateSolidColorBrush(&col(255, 255, 255, 0.62), None)?;
    for _ in 0..12 {
        let (x, y) = pick(rng);
        let (ox, oy) = cv.out(x, y);
        let ang = oy.atan2(ox) + rng.range(-0.35, 0.35);
        fern(dc, &line, x as f32, y as f32, ang, cv.size * rng.range(0.16, 0.32), 1.1 * k, 2, rng);
    }

    // Alttan sarkan buz sarkıtları: dışa doğru yönü aşağı bakan kenarlardan.
    let down: Vec<_> = edge.iter().copied().filter(|&(x, y)| cv.out(x, y).1 > 0.8).collect();
    if !down.is_empty() {
        let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: col(246, 250, 255, 0.9) }, D2D1_GRADIENT_STOP { position: 1.0, color: col(200, 226, 255, 0.12) }];
        let rt: &ID2D1RenderTarget = dc;
        let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
        let f = dc.GetFactory()?;
        for _ in 0..3 {
            let (x, y) = down[((rng.f() * down.len() as f32) as usize).min(down.len() - 1)];
            let (x, y) = (x as f32, y as f32 - 1.0);
            let len = cv.size * rng.range(0.14, 0.32);
            let half = cv.size * rng.range(0.035, 0.06);
            let g = f.CreatePathGeometry()?;
            let sink = g.Open()?;
            sink.BeginFigure(pt(x - half, y), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(pt(x + half, y));
            sink.AddLine(pt(x + rng.range(-0.3, 0.3) * half, y + len));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            let fill = dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(x, y), endPoint: pt(x, y + len) }, None, &coll)?;
            dc.FillGeometry(&g, &fill, None);
        }
    }

    // Kristal parıltıları: kenarın hemen dışında.
    let spark = dc.CreateSolidColorBrush(&col(255, 255, 255, 0.8), None)?;
    for i in 0..26 {
        let (x, y) = pick(rng);
        let (ox, oy) = cv.out(x, y);
        let r = rng.range(0.0, cv.glow());
        let (sx, sy) = (x as f32 + ox * r, y as f32 + oy * r);
        if i % 6 == 0 {
            let l = 2.6 * k;
            dc.DrawLine(pt(sx - l, sy), pt(sx + l, sy), &spark, 0.7 * k, None);
            dc.DrawLine(pt(sx, sy - l), pt(sx, sy + l), &spark, 0.7 * k, None);
        } else {
            let rr = rng.range(0.35, 0.9) * k;
            dc.FillEllipse(&D2D1_ELLIPSE { point: pt(sx, sy), radiusX: rr, radiusY: rr }, &spark);
        }
    }
    Ok(())
}

pub struct Frost {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    vis: Option<SpriteVisual>,
    on: bool,
    checked: Instant,
    /// Son görülen imleç konumu (gizliyse `None`) ve o konuma geliş anı.
    last: Option<(i32, i32)>,
    still: Instant,
    rng: Rng,
}

impl Frost {
    pub fn new(gfx: &Gfx) -> Result<Self> {
        static CLASS: Once = Once::new();
        let hwnd = unsafe { overlay_window(w!("CarlUI.DockFrost"), &CLASS)? };
        let made = (|| -> Result<_> {
            let target = unsafe { gfx.comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
            let root = gfx.comp.CreateContainerVisual()?;
            target.SetRoot(&root)?;
            Ok((target, root))
        })();
        match made {
            Ok((target, root)) => {
                let now = Instant::now();
                Ok(Self { hwnd, _target: target, root, vis: None, on: false, checked: now, last: None, still: now, rng: Rng::from_time() })
            }
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                Err(e)
            }
        }
    }

    /// İmleç kıpırdamadıkça buzu getirir; kıpırdayınca ya da tıklanınca eritir
    /// (donukken asıl eritme `watch_proc`'ta, burada yalnız durum eşlenir).
    pub fn step(&mut self, gfx: &Gfx, now: Instant) {
        if self.on && !watching() {
            self.on = false;
        }
        let cur = unsafe { cursor() };
        let pos = cur.map(|(p, _)| (p.x, p.y));
        if pos != self.last || pressed() {
            self.last = pos;
            self.still = now;
            if self.on {
                self.melt(gfx);
            }
            return;
        }
        if self.on || now - self.checked < CHECK {
            return;
        }
        self.checked = now;
        if let Some((p, c)) = cur {
            if now - self.still >= IDLE {
                // Şekli okunamayan imleç (ör. boş imleç) donmaz; bir sonraki
                // kıpırdamaya kadar yeniden denenmez.
                if self.freeze(gfx, p, c).is_err() {
                    self.still = now + Duration::from_secs(3600);
                }
            }
        }
    }

    fn freeze(&mut self, gfx: &Gfx, p: POINT, c: HCURSOR) -> Result<()> {
        let s = unsafe { shape(c) }.ok_or_else(|| Error::from(E_FAIL))?;
        let cv = Canvas::new(&s);
        let n = cv.side as f32;
        let (hx, hy) = cv.hot(&s);
        let rng = &mut self.rng;
        let surf = gfx.surface(n, n, |dc| unsafe { draw(dc, &cv, rng) })?;
        let brush = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
        let vis = gfx.comp.CreateSpriteVisual()?;
        vis.SetBrush(&brush)?;
        vis.SetSize(v2(n, n))?;
        vis.SetOffset(v3(0.0, 0.0))?;
        vis.SetCenterPoint(v3(hx, hy))?;
        vis.SetOpacity(0.0)?;
        vis.SetScale(Vector3 { X: 0.55, Y: 0.55, Z: 1.0 })?;
        self.root.Children()?.RemoveAll()?;
        self.root.Children()?.InsertAtTop(&vis)?;
        unsafe {
            SetWindowPos(self.hwnd, HWND_TOPMOST, p.x - hx as i32, p.y - hy as i32, cv.side as i32, cv.side as i32, SWP_NOACTIVATE | SWP_SHOWWINDOW)?;
        }
        let ease = gfx.comp.CreateCubicBezierEasingFunction(v2(0.2, 0.0), v2(0.3, 1.0))?;
        let fade = gfx.comp.CreateScalarKeyFrameAnimation()?;
        fade.InsertKeyFrameWithEasingFunction(1.0, 1.0, &ease)?;
        fade.SetDuration(ts(IN_MS))?;
        vis.StartAnimation(h!("Opacity"), &fade)?;
        let grow = gfx.comp.CreateVector3KeyFrameAnimation()?;
        grow.InsertKeyFrameWithEasingFunction(1.0, Vector3 { X: 1.0, Y: 1.0, Z: 1.0 }, &ease)?;
        grow.SetDuration(ts(IN_MS))?;
        vis.StartAnimation(h!("Scale"), &grow)?;
        let _ = unwatch();
        let id = unsafe { SetTimer(None, 0, POLL_MS, Some(watch_proc)) };
        if id != 0 {
            WATCH.with(|c| *c.borrow_mut() = Some(Watch { id, at: (p.x, p.y), comp: gfx.comp.clone(), vis: vis.clone() }));
        }
        self.vis = Some(vis);
        self.on = true;
        Ok(())
    }

    fn melt(&mut self, gfx: &Gfx) {
        self.on = false;
        let _ = unwatch();
        if let Some(vis) = self.vis.as_ref() {
            let _ = fade_out(&gfx.comp, vis);
        }
    }
}

impl Drop for Frost {
    fn drop(&mut self) {
        let _ = unwatch();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uzaklik_sekle_gore_artar() {
        let (w, h) = (9, 9);
        let mut on = vec![false; w * h];
        on[4 * w + 4] = true;
        let d = distance(&on, w, h);
        assert_eq!(d[4 * w + 4], 0.0);
        assert!((d[4 * w + 7] - 3.0).abs() < 1e-4);
        // Köşegen: 3·4/3 = 4 (gerçek 4,24).
        assert!((d[7 * w + 7] - 4.0).abs() < 1e-4);
        assert!(d[0] > d[2 * w + 2]);
    }

    #[test]
    fn tek_renkli_metin_imleci_gorunur_sayilir() {
        // 2×1: soldaki piksel siyah boyanır (AND 0), sağdaki ters çevrilir
        // (AND 1, XOR 1). İkisi de görünür.
        let px = |v: u8| [v, v, v, 0];
        let mask: Vec<u8> = [px(0), px(255), px(0), px(255)].concat();
        assert_eq!(from_mono(&mask, 2, 1), vec![true, true]);
        // Saydam piksel: AND 1, XOR 0.
        let clear: Vec<u8> = [px(255), px(0)].concat();
        assert_eq!(from_mono(&clear, 1, 1), vec![false]);
    }

    #[test]
    fn alfasiz_renkli_imlec_maskeye_bakar() {
        let px = vec![10, 10, 10, 0, 20, 20, 20, 0];
        let mask = vec![0, 0, 0, 0, 255, 255, 255, 0];
        assert_eq!(from_color(&px, &mask), vec![true, false]);
        let alpha = vec![0, 0, 0, 255, 0, 0, 0, 0];
        assert_eq!(from_color(&alpha, &[]), vec![true, false]);
    }

    #[test]
    fn tuval_imleci_sarar_ve_kenari_bulur() {
        // 32×32 ok benzeri üçgen.
        let (w, h) = (32, 32);
        let on: Vec<bool> = (0..w * h).map(|i| (i % w) <= (i / w) / 2).collect();
        let s = Shape { w, h, on, hot: (0, 0) };
        let cv = Canvas::new(&s);
        assert!(cv.side >= 76);
        let (hx, hy) = cv.hot(&s);
        assert_eq!((hx as usize, hy as usize), cv.at);
        let e = cv.edge();
        assert!(!e.is_empty());
        // Kenar pikselleri şeklin dışında, kabuk kalınlığında.
        assert!(e.iter().all(|&(x, y)| (cv.d(x, y) - cv.rim()).abs() <= 0.7));
        // Kabuk tuvalin içinde kalır (sarkıtlar ve hâle için pay var).
        let rim_room = cv.rim() + cv.glow();
        assert!((cv.at.0 as f32) > rim_room && (cv.side - cv.at.1 - h) as f32 > rim_room);
    }
}
