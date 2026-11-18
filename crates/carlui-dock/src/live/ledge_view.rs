//! Kenar birikintisinin çizimi: kar yağarken açık pencerelerin, dock'un ve
//! bildirim adasının üst kenarında biriken kar; yağmurda aynı kenarlarda
//! ıslak parıltı ve arada kenar boyunca kayan bir ışık.
//!
//! Monitör boyutunda, tıklamayı geçiren tek bir katman penceresi dock'un
//! hemen altında durur (dock ve önizlemeleri karın üstünde kalır). Pencereler
//! z sırasıyla toplanır; bir kenarın önünde başka pencere varsa o kısma kar
//! çizilmez (`ledge::visible`). Her yüzeyin kendi küçük çizim yüzeyi vardır ve
//! yalnızca kar gözle görülür değiştiğinde yeniden çizilir; karelerde yalnızca
//! ışığın konumu/saydamlığı gönderilir.

use super::{
    ledge::{max_height, visible, Edge, Ledge, Ledges, R4, ROUGH},
    rng::Rng,
    snowfall::{Plow, Snowfall},
    snowplay::SnowPlay,
    window::{overlay_window, BackdropWindow},
};
use crate::dock::LiveView;
use carlui_core::live::LiveIntensity;
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use std::{
    sync::Once,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Foundation::{Numerics::Matrix3x2, Size},
    Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat},
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Dwm::*,
            Gdi::{GetWindowRgnBox, GDI_REGION_TYPE},
        },
        System::WinRT::Composition::{ICompositionDrawingSurfaceInterop, ICompositorDesktopInterop},
        UI::WindowsAndMessaging::*,
    },
    UI::Composition::{Desktop::DesktopWindowTarget, *},
};

/// Hangi hava.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Snow,
    Rain,
}

/// Kenar listesi bu sıklıkla yeniden okunur (pencere taşınınca kar düşsün).
const GATHER: Duration = Duration::from_millis(150);
/// Yağmurda ışık akıcı kaysın diye kare aralığı.
const RAIN_FRAME: Duration = Duration::from_millis(50);
/// Kar topu hareket ederken kare aralığı (akıcı uçuş).
const PLAY_FRAME: Duration = Duration::from_millis(16);
/// Karda/erirken kare aralığı (birikme yavaş).
const SLOW_FRAME: Duration = Duration::from_millis(150);
/// Işığın kenarı geçme süresi (sn).
const GLINT_RUN: f32 = 2.5;
/// Ekranın alt kenarı boyunca uzanan birikinti.
const DRIFT_KEY: isize = -2;
/// Dock'taki kar: daha kalın, bir yerde tepecik yapar.
const DOCK_DEPTH: f32 = 1.5;
const DOCK_MOUND: f32 = 0.8;
/// Kenarın üstünde ayrılan en yüksek kar (kalınlık × tepecik).
const PEAK: f32 = DOCK_DEPTH * (1.0 + DOCK_MOUND);

type C = (u8, u8, u8);

fn col(c: C, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// Sabit karışım: aynı sütun her çizimde aynı kararı versin (titreme yok).
fn hash(a: u32, b: u32) -> u32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

unsafe fn linear(dc: &ID2D1DeviceContext, y0: f32, y1: f32, c0: D2D1_COLOR_F, c1: D2D1_COLOR_F) -> Result<ID2D1LinearGradientBrush> {
    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: c0 }, D2D1_GRADIENT_STOP { position: 1.0, color: c1 }];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(0.0, y0), endPoint: pt(0.0, y1) };
    dc.CreateLinearGradientBrush(&props, None, &coll)
}

/// Yağmur ışığının bitmap'i: yatay, yumuşak beyaz parıltı.
unsafe fn draw_glint(dc: &ID2D1DeviceContext) -> Result<()> {
    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: col((255, 255, 255), 0.95) }, D2D1_GRADIENT_STOP { position: 1.0, color: col((235, 245, 255), 0.0) }];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: pt(32.0, 4.0), gradientOriginOffset: pt(0.0, 0.0), radiusX: 32.0, radiusY: 4.0 };
    let b = dc.CreateRadialGradientBrush(&props, None, &coll)?;
    dc.FillEllipse(&D2D1_ELLIPSE { point: pt(32.0, 4.0), radiusX: 32.0, radiusY: 4.0 }, &b);
    Ok(())
}

/// Bir yüzeyin görselleri.
struct Slot {
    key: isize,
    root: ContainerVisual,
    cap: SpriteVisual,
    surf: CompositionDrawingSurface,
    glint: SpriteVisual,
    /// Yüzeyin piksel boyutu ve pencere içi konumu (son gönderilen).
    size: (i32, i32),
    off: (i32, i32),
    /// Son çizimdeki ıslaklık.
    sheen: f32,
    /// Işık döngüsü: `clock < GLINT_RUN` iken kayar, sonra `gap` kadar bekler.
    clock: f32,
    gap: f32,
    glint_on: bool,
}

pub struct LedgeView {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    glint_brush: CompositionSurfaceBrush,
    mon: RECT,
    s: f32,
    model: Ledges,
    slots: Vec<Slot>,
    mode: Option<(Mode, f32)>,
    /// Islaklık (0..=1): yağmurda yavaşça gelir, durunca kurur.
    sheen: f32,
    rng: Rng,
    shown: bool,
    last: Instant,
    gathered: Option<Instant>,
    due: Instant,
    /// Yağan kar (yalnızca kar modunda, hareket azaltılmamışken).
    /// Yağan karın çizildiği, pencerelerin arkasındaki katman.
    backdrop: Option<BackdropWindow>,
    backdrop_z: Instant,
    snowfall: Option<Snowfall>,
    plow: Option<Plow>,
    next_plow: Instant,
    /// Kar topları ve kardan adamlar.
    play: SnowPlay,
}

impl LedgeView {
    pub fn new(gfx: &Gfx, view: &LiveView, intensity: LiveIntensity) -> Result<Self> {
        static CLASS: Once = Once::new();
        let class = if crate::sandbox::on() { w!("CarlUI.DockSandboxLedge") } else { w!("CarlUI.DockLedge") };
        let hwnd = unsafe { overlay_window(class, &CLASS)? };
        let mon = view.mon;
        unsafe {
            // Dock'un hemen altına: ikonlar, önizlemeler ve hayvan karın üstünde kalır.
            let _ = SetWindowPos(hwnd, view.hwnd, mon.left, mon.top, (mon.right - mon.left).max(1), (mon.bottom - mon.top).max(1), SWP_NOACTIVATE);
        }
        let made = (|| -> Result<_> {
            let target = unsafe { gfx.comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
            let root = gfx.comp.CreateContainerVisual()?;
            root.SetOpacity(if intensity == LiveIntensity::Normal { 1.0 } else { 0.75 })?;
            target.SetRoot(&root)?;
            let surf = gfx.surface(64.0, 8.0, |dc| unsafe { draw_glint(dc) })?;
            let glint_brush = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
            glint_brush.SetStretch(CompositionStretch::Fill)?;
            Ok((target, root, glint_brush))
        })();
        let (target, root, glint_brush) = match made {
            Ok(v) => v,
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return Err(e);
            }
        };
        let s = view.scale.max(0.5);
        let mut seed = Rng::from_time();
        let model = Ledges::new(s, Rng::new(seed.next_u32()));
        let now = Instant::now();
        Ok(Self {
            hwnd,
            _target: target,
            root,
            glint_brush,
            mon,
            s,
            model,
            slots: Vec::new(),
            mode: None,
            sheen: 0.0,
            rng: Rng::new(seed.next_u32()),
            shown: false,
            last: now,
            gathered: None,
            due: now,
            backdrop: None,
            backdrop_z: now,
            snowfall: None,
            plow: None,
            next_plow: now + Duration::from_secs(75),
            play: SnowPlay::new(s, mon, DRIFT_KEY),
        })
    }

    pub fn show(&mut self, on: bool) {
        if on != self.shown {
            self.shown = on;
            unsafe {
                let _ = ShowWindow(self.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
            if let Some(b) = self.backdrop.as_mut() {
                b.show(on);
            }
        }
    }

    /// Hava değişti. `None` ya da yağmur karı eritir; yağmur dışı her şey kurutur.
    pub fn set_mode(&mut self, mode: Option<(Mode, f32)>) {
        self.mode = mode.map(|(m, l)| (m, if l.is_finite() { l.clamp(0.15, 1.0) } else { 0.6 }));
    }

    /// Kar biriktiren yüzey yüksekliği (pencere içinde kenarın üstü).
    fn above(&self) -> i32 {
        (max_height(1.0, self.s) * PEAK + 3.0 * self.s).ceil() as i32
    }

    /// Kenarın altı (gölge, ıslaklık).
    fn below(&self) -> i32 {
        (7.0 * self.s).ceil() as i32
    }

    /// Ekrandaki yüzeyleri z sırasıyla toplar.
    fn gather(&self, pill: Option<RECT>) -> Vec<Edge> {
        let s = self.s;
        let mon = self.mon;
        let ha = self.above();
        let hb = self.below();
        let mut edges = Vec::new();
        // Dock en üstte: önünde hiçbir şey yok.
        if let Some(p) = pill {
            if p.right - p.left > 20 {
                edges.push(Edge {
                    key: -1,
                    x0: p.left,
                    x1: p.right,
                    y: p.top,
                    room: ha,
                    taper: ((p.bottom - p.top) / 2).max(4),
                    spans: vec![(p.left, p.right)],
                    depth: DOCK_DEPTH,
                    rough: ROUGH,
                    mound: DOCK_MOUND,
                });
            }
        }
        let mut found: Vec<HWND> = Vec::new();
        unsafe extern "system" fn cb(h: HWND, data: LPARAM) -> BOOL {
            (*(data.0 as *mut Vec<HWND>)).push(h);
            TRUE
        }
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut found as *mut _ as isize));
        }
        let own_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        let mon_area = (mon.right - mon.left) as i64 * (mon.bottom - mon.top) as i64;
        let mut cover: Vec<R4> = Vec::new();
        for h in found {
            // Bir yer alt kenar birikintisine kalır.
            if edges.len() >= super::ledge::MAX_LEDGES - 1 {
                break;
            }
            unsafe {
                if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                    continue;
                }
                let mut pid = 0;
                GetWindowThreadProcessId(h, Some(&mut pid));
                if pid == own_pid {
                    continue;
                }
                let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
                if ex & WS_EX_TRANSPARENT.0 != 0 || crate::tracker::is_cloaked(h) {
                    continue;
                }
                let class = carlui_core::win::class_name(h);
                // Masaüstünün kendisi (simgeler, duvar kâğıdı) örtücü değildir.
                if class == "Progman" || class == "WorkerW" {
                    continue;
                }
                let island = class == "CarlUI.Island";
                let r = if island { island_rect(h) } else { frame_rect(h) };
                let Some(r) = r else { continue };
                if r.right <= mon.left || r.left >= mon.right || r.bottom <= mon.top || r.top >= mon.bottom || r.right - r.left < 4 {
                    continue;
                }
                // Ekranı kaplayan saydam araç katmanları (masaüstü süsleri vb.) örtücü sayılmaz.
                let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
                let tool = ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_LAYERED.0 != 0;
                if tool && !island && area * 10 >= mon_area * 9 {
                    continue;
                }
                let surface = if island {
                    true
                } else {
                    crate::tracker::is_app_window(h, own_pid) && !IsZoomed(h).as_bool() && (r.right - r.left) as f32 >= 100.0 * s && r.top > mon.top + 2
                };
                if surface {
                    let (x0, x1) = (r.left.max(mon.left), r.right.min(mon.right));
                    let room = (r.top - mon.top).min(ha);
                    if room > 0 && x1 - x0 > 20 {
                        let taper = if island { ((r.bottom - r.top) / 2).max(4) } else { (8.0 * s) as i32 };
                        let spans = visible(x0, x1, r.top - ha, r.top + hb, &cover);
                        edges.push(Edge { key: h.0 as isize, x0, x1, y: r.top, room, taper, spans, depth: 1.0, rough: ROUGH, mound: 0.3 });
                    }
                }
                cover.push((r.left, r.top, r.right, r.bottom));
            }
        }
        // Ekranın alt kenarı: en arkada, önündeki pencereler örter; dalgalı.
        let spans = visible(mon.left, mon.right, mon.bottom - ha, mon.bottom, &cover);
        edges.push(Edge { key: DRIFT_KEY, x0: mon.left, x1: mon.right, y: mon.bottom, room: ha, taper: 0, spans, depth: 1.2, rough: 0.32, mound: 0.0 });
        edges
    }

    /// Model ile görselleri eşler: yeni yüzeye görsel kurar, gidenleri kaldırır.
    fn sync_slots(&mut self, gfx: &Gfx) -> Result<()> {
        let mut old = std::mem::take(&mut self.slots);
        let keys: Vec<isize> = self.model.list.iter().map(|l| l.edge.key).collect();
        for key in keys {
            let slot = match old.iter().position(|s| s.key == key) {
                Some(i) => old.swap_remove(i),
                None => self.new_slot(gfx, key)?,
            };
            self.slots.push(slot);
        }
        let kids = self.root.Children()?;
        for s in old {
            let _ = kids.Remove(&s.root);
        }
        Ok(())
    }

    fn new_slot(&mut self, gfx: &Gfx, key: isize) -> Result<Slot> {
        let root = gfx.comp.CreateContainerVisual()?;
        let surf = gfx.gdev.CreateDrawingSurface(Size { Width: 1.0, Height: 1.0 }, DirectXPixelFormat::B8G8R8A8UIntNormalized, DirectXAlphaMode::Premultiplied)?;
        let b = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
        b.SetStretch(CompositionStretch::None)?;
        b.SetHorizontalAlignmentRatio(0.0)?;
        b.SetVerticalAlignmentRatio(0.0)?;
        let cap = gfx.comp.CreateSpriteVisual()?;
        cap.SetBrush(&b)?;
        let glint = gfx.comp.CreateSpriteVisual()?;
        glint.SetBrush(&self.glint_brush)?;
        glint.SetOpacity(0.0)?;
        glint.SetSize(v2(44.0 * self.s, 5.0 * self.s))?;
        root.Children()?.InsertAtTop(&cap)?;
        root.Children()?.InsertAtTop(&glint)?;
        self.root.Children()?.InsertAtTop(&root)?;
        Ok(Slot { key, root, cap, surf, glint, size: (0, 0), off: (i32::MIN, 0), sheen: -1.0, clock: 0.0, gap: self.rng.range(0.5, 4.0), glint_on: false })
    }

    /// Her yüzeyi gerekiyorsa yeniden çizer.
    fn redraw(&mut self, force: bool) -> Result<()> {
        let (ha, hb) = (self.above(), self.below());
        let (s, sheen, mon) = (self.s, self.sheen, self.mon);
        for (l, slot) in self.model.list.iter_mut().zip(self.slots.iter_mut()) {
            let w = (l.edge.x1 - l.edge.x0).max(1);
            let size = (w, ha + hb);
            let off = (l.edge.x0 - mon.left, l.edge.y - mon.top - ha);
            let geom = size != slot.size || off != slot.off;
            let need = force || geom || l.dirty || l.pending >= 0.35 || (sheen - slot.sheen).abs() >= 0.08 || (sheen == 0.0) != (slot.sheen == 0.0);
            if !need {
                continue;
            }
            let interop: ICompositionDrawingSurfaceInterop = slot.surf.cast()?;
            if size != slot.size {
                unsafe { interop.Resize(SIZE { cx: size.0, cy: size.1 })? };
                slot.cap.SetSize(v2(size.0 as f32, size.1 as f32))?;
                slot.size = size;
            }
            if off != slot.off {
                slot.root.SetOffset(v3(off.0 as f32, off.1 as f32))?;
                slot.off = off;
            }
            unsafe {
                let mut o = POINT::default();
                let dc: ID2D1DeviceContext = interop.BeginDraw(None, &mut o)?;
                dc.SetDpi(96.0, 96.0);
                dc.SetTransform(&Matrix3x2::translation(o.x as f32, o.y as f32));
                dc.Clear(Some(&D2D1_COLOR_F::default()));
                let drawn = paint(&dc, l, ha as f32, hb as f32, s, sheen);
                interop.EndDraw()?;
                drawn?;
            }
            l.pending = 0.0;
            l.dirty = false;
            slot.sheen = sheen;
        }
        Ok(())
    }

    /// Yağmur ışıkları: her yüzeyde en uzun görünür aralık boyunca kayar.
    fn glints(&mut self, dt: f32, animate: bool) -> Result<()> {
        let (s, sheen, ha) = (self.s, self.sheen, self.above() as f32);
        for (l, slot) in self.model.list.iter().zip(self.slots.iter_mut()) {
            let span = l.edge.spans.iter().copied().max_by_key(|(a, b)| b - a);
            let mut opacity = 0.0;
            if let (true, true, Some((a, b))) = (animate, sheen > 0.02, span) {
                slot.clock += dt;
                if slot.clock >= GLINT_RUN + slot.gap {
                    slot.clock = 0.0;
                    slot.gap = self.rng.range(3.0, 7.0);
                }
                if slot.clock < GLINT_RUN {
                    let u = slot.clock / GLINT_RUN;
                    let x = (a - l.edge.x0) as f32 + (b - a) as f32 * u;
                    let (gw, gh) = (44.0 * s, 5.0 * s);
                    slot.glint.SetOffset(v3(x - gw / 2.0, ha - gh / 2.0))?;
                    opacity = sheen * 0.8 * (u * std::f32::consts::PI).sin();
                }
            }
            if opacity > 0.0 || slot.glint_on {
                slot.glint.SetOpacity(opacity)?;
                slot.glint_on = opacity > 0.0;
            }
        }
        Ok(())
    }

    fn sheen_target(&self) -> f32 {
        match self.mode {
            Some((Mode::Rain, l)) => 0.45 + 0.55 * l,
            _ => 0.0,
        }
    }

    /// Bir kare. `pill`: dock altta ve görünürken pill dikdörtgeni.
    pub fn step(&mut self, gfx: &Gfx, now: Instant, pill: Option<RECT>) -> Result<()> {
        if now < self.due {
            return Ok(());
        }
        let dt = (now - self.last).as_secs_f32().min(0.25);
        self.last = now;
        if self.gathered.is_none_or(|t| now - t >= GATHER) {
            self.gathered = Some(now);
            let edges = self.gather(pill);
            self.model.sync(edges);
            self.sync_slots(gfx)?;
        }
        let snow = match self.mode {
            Some((Mode::Snow, l)) => Some(l),
            _ => None,
        };
        let rain = matches!(self.mode, Some((Mode::Rain, _)));
        self.model.grow(dt, snow, rain);
        self.weather(gfx, now, dt, snow)?;
        let back = self.backdrop.as_ref().map(|b| b.root.clone());
        self.play.step(gfx, dt, snow.is_some(), pill, &self.model, &self.root, back.as_ref())?;
        let target = self.sheen_target();
        let k = 0.5 * dt;
        self.sheen = if self.sheen < target { (self.sheen + k).min(target) } else { (self.sheen - k).max(target) };
        self.redraw(false)?;
        self.glints(dt, true)?;
        self.due = now + if self.play.busy() {
            PLAY_FRAME
        } else if rain || self.sheen > 0.0 || self.plow.is_some() { RAIN_FRAME } else { SLOW_FRAME };
        Ok(())
    }

    /// Yağan kar ve kar küreme aracı (yalnızca kar yağarken).
    fn weather(&mut self, gfx: &Gfx, now: Instant, dt: f32, snow: Option<f32>) -> Result<()> {
        let Some(level) = snow else {
            self.snowfall = None;
            self.plow = None;
            // Toplar/adamlar erirken arka katman kalır.
            if self.play.is_empty() {
                self.backdrop = None;
            }
            return Ok(());
        };
        let (w, h) = ((self.mon.right - self.mon.left) as f32, (self.mon.bottom - self.mon.top) as f32);
        if self.snowfall.as_ref().is_none_or(|f| (f.level - level).abs() > 0.2) {
            // Önce eskisi gitsin; ikisi üst üste binmesin.
            self.snowfall = None;
            if self.backdrop.is_none() {
                let mut b = BackdropWindow::new(gfx, self.mon)?;
                b.show(self.shown);
                self.backdrop = Some(b);
                self.backdrop_z = now;
            }
            let parent = self.backdrop.as_ref().map(|b| b.root.clone()).unwrap_or_else(|| self.root.clone());
            self.snowfall = Some(Snowfall::new(gfx, &parent, w, h, self.s, level, &mut self.rng)?);
        }
        // Masaüstü öne gelirse (Win+D, masaüstüne tıklama) kar yeniden onun üstüne.
        if now - self.backdrop_z >= Duration::from_secs(2) {
            self.backdrop_z = now;
            if let Some(b) = self.backdrop.as_mut() {
                b.keep_z();
            }
        }
        if self.plow.is_none() && now >= self.next_plow {
            if self.model.fullness(DRIFT_KEY, level) > 0.6 {
                self.plow = Some(Plow::new(gfx, &self.root, w, h, self.s)?);
                self.next_plow = now + Duration::from_secs_f32(self.rng.range(90.0, 180.0));
            } else {
                self.next_plow = now + Duration::from_secs(20);
            }
        }
        if let Some(p) = self.plow.as_mut() {
            match p.advance(dt) {
                Some((a, b)) => {
                    let left = self.mon.left as f32;
                    self.model.clear(DRIFT_KEY, a + left, b + left);
                    self.play.snow.clear(DRIFT_KEY, a + left, b + left);
                }
                None => self.plow = None,
            }
        }
        Ok(())
    }

    /// Hareketi azalt: kar beklemeden oturur, ıslaklık tam; ışık yok.
    pub fn settle(&mut self, gfx: &Gfx, pill: Option<RECT>) -> Result<()> {
        let edges = self.gather(pill);
        self.model.sync(edges);
        self.sync_slots(gfx)?;
        self.snowfall = None;
        self.play.reset(&self.root, self.backdrop.as_ref().map(|b| &b.root));
        self.backdrop = None;
        self.plow = None;
        match self.mode {
            Some((Mode::Snow, l)) => self.model.settle(l),
            _ => self.model.list.iter_mut().for_each(|l| l.h.iter_mut().for_each(|h| *h = 0.0)),
        }
        self.sheen = self.sheen_target();
        self.redraw(true)?;
        self.glints(0.0, false)
    }

    /// Kar kalmadı, kurudu ve hava artık kar/yağmur değil: bırakılabilir.
    pub fn finished(&self) -> bool {
        self.mode.is_none() && self.sheen == 0.0 && self.model.bare() && self.play.is_empty()
    }

    /// Kar modunda mı (kar topu oyunu açık)?
    pub fn snowy(&self) -> bool {
        matches!(self.mode, Some((Mode::Snow, _)))
    }

    /// Fare olayı (`input::WM_APP_SNOW`). Bir şey değiştiyse kare hemen gelir.
    pub fn mouse(&mut self, kind: usize, x: i32, y: i32, dock: HWND, pill: Option<RECT>) -> bool {
        if !self.shown || (!self.snowy() && kind == super::snowplay::DOWN) {
            return false;
        }
        let changed = self.play.mouse(kind, x, y, dock, pill, &mut self.model);
        if changed {
            self.due = Instant::now();
        }
        changed
    }

    /// Bir sonraki karenin zamanı.
    pub fn due(&self) -> Instant {
        self.due
    }
}

impl Drop for LedgeView {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Gölgesiz görünen pencere çerçevesi (DWM), yoksa pencere dikdörtgeni.
unsafe fn frame_rect(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_ok() && r.right > r.left {
        return Some(r);
    }
    GetWindowRect(h, &mut r).ok().map(|_| r)
}

/// Adanın görünen kısmı: pencere bölgesinin kutusu (bölge yoksa pencerenin tamamı).
unsafe fn island_rect(h: HWND) -> Option<RECT> {
    let mut w = RECT::default();
    GetWindowRect(h, &mut w).ok()?;
    let mut b = RECT::default();
    let kind: GDI_REGION_TYPE = GetWindowRgnBox(h, &mut b);
    // 2 = SIMPLEREGION, 3 = COMPLEXREGION.
    if (kind.0 == 2 || kind.0 == 3) && b.right > b.left {
        Some(RECT { left: w.left + b.left, top: w.top + b.top, right: w.left + b.right, bottom: w.top + b.bottom })
    } else {
        Some(w)
    }
}

/// Bir yüzeyin karını ve ıslaklığını çizer. Kenar `ha` yüksekliğindedir.
unsafe fn paint(dc: &ID2D1DeviceContext, l: &Ledge, ha: f32, hb: f32, s: f32, sheen: f32) -> Result<()> {
    let x0 = l.edge.x0 as f32;
    let max = max_height(1.0, s);
    let seed = l.edge.key as u32;
    // Islaklık: kenarda ince parlak çizgi, altında sönen ıslak şerit, birkaç boncuk.
    if sheen > 0.0 {
        let line = dc.CreateSolidColorBrush(&col((235, 245, 255), 0.45 * sheen), None)?;
        let wet = linear(dc, ha + 0.8 * s, ha + hb, col((200, 220, 245), 0.18 * sheen), col((200, 220, 245), 0.0))?;
        let bead = dc.CreateSolidColorBrush(&col((225, 238, 255), 0.55 * sheen), None)?;
        let spark = dc.CreateSolidColorBrush(&col((255, 255, 255), 0.8 * sheen), None)?;
        for &(a, b) in &l.edge.spans {
            let (a, b) = (a as f32 - x0, b as f32 - x0);
            dc.FillRectangle(&D2D_RECT_F { left: a, top: ha - 0.5 * s, right: b, bottom: ha + 0.8 * s }, &line);
            dc.FillRectangle(&D2D_RECT_F { left: a, top: ha + 0.8 * s, right: b, bottom: ha + hb }, &wet);
            let step = 26.0 * s;
            let mut k = (a / step).ceil() as i32;
            while (k as f32) * step < b - 4.0 * s {
                let hsh = hash(seed, k as u32);
                if hsh % 3 == 0 {
                    let x = k as f32 * step + (hsh >> 8) as f32 % step * 0.5;
                    let y = ha + 1.6 * s + ((hsh >> 16) % 3) as f32 * 0.6 * s;
                    let r = (1.0 + ((hsh >> 20) % 3) as f32 * 0.3) * s;
                    dc.FillEllipse(&D2D1_ELLIPSE { point: pt(x, y), radiusX: r, radiusY: r * 1.15 }, &bead);
                    dc.FillEllipse(&D2D1_ELLIPSE { point: pt(x - 0.3 * r, y - 0.35 * r), radiusX: 0.35 * r, radiusY: 0.35 * r }, &spark);
                }
                k += 1;
            }
        }
    }
    if l.bare() {
        return Ok(());
    }
    // Kar: sütun tepelerinden geçen dolu şekil, beyazdan buz mavisine.
    let f = dc.GetFactory()?;
    let fill = linear(dc, ha - max, ha, col((255, 255, 255), 0.97), col((214, 224, 240), 0.95))?;
    let shade = linear(dc, ha, ha + 3.0 * s, col((20, 30, 50), 0.2), col((20, 30, 50), 0.0))?;
    let glitter = dc.CreateSolidColorBrush(&col((255, 255, 255), 1.0), None)?;
    for &(a, b) in &l.edge.spans {
        let (sa, sb) = (a as f32, b as f32);
        // Aralıkta ortalama kalınlık: altındaki gölge onunla koyulaşır.
        let mut sum = 0.0;
        let mut n = 0.0;
        let g = f.CreatePathGeometry()?;
        let sink = g.Open()?;
        sink.BeginFigure(pt(sa - x0, ha + 0.5 * s), D2D1_FIGURE_BEGIN_FILLED);
        let mut x = sa;
        loop {
            let h = l.height_at(x);
            sum += h;
            n += 1.0;
            sink.AddLine(pt(x - x0, ha + 0.5 * s - h));
            if x >= sb {
                break;
            }
            x = (x + l.col_w * 0.5).min(sb);
        }
        sink.AddLine(pt(sb - x0, ha + 0.5 * s));
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        let avg = sum / n;
        if avg < 0.05 {
            continue;
        }
        shade.SetOpacity((avg / (3.0 * s)).min(1.0));
        dc.FillRectangle(&D2D_RECT_F { left: sa - x0, top: ha + 0.5 * s, right: sb - x0, bottom: ha + 3.5 * s }, &shade);
        dc.FillGeometry(&g, &fill, None);
        // Seyrek ışıltılar.
        for i in 0..l.h.len() {
            let cx = l.col_x(i);
            if cx < sa + 2.0 * s || cx > sb - 2.0 * s || l.h[i] < 2.0 * s || hash(seed, i as u32) % 17 != 0 {
                continue;
            }
            let y = ha + 0.5 * s - l.h[i] + 1.2 * s;
            dc.FillEllipse(&D2D1_ELLIPSE { point: pt(cx - x0, y), radiusX: 0.7 * s, radiusY: 0.7 * s }, &glitter);
        }
    }
    Ok(())
}
