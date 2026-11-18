//! Karla oyun: birikmiş kardan top avuçlama, sürükleyip atma, imleçle
//! itme, üç topu üst üste koyunca kardan adam.
//!
//! Fizik `snowball` modelinde; burada yalnızca fare olaylarının yorumu ve
//! görseller var. Alt birikintideki toplar/adamlar pencerelerin arkasındaki
//! katmanda (uygulamalar onları örter), tutulan/uçan/dock üstündeki toplar
//! dock'un hemen altındaki katmanda çizilir; top katman değiştirince görseli
//! taşınır. Tıklama hiçbir zaman yutulmaz: yalnızca dock'un ya da
//! masaüstünün üstündeki basışlar kar sayılır.

use super::{
    ledge::Ledges,
    snowball::{Ball, Floor, Man, Press, Snow, State},
};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Direct2D::{Common::*, *},
        UI::WindowsAndMessaging::*,
    },
    UI::Composition::*,
};

/// Dock'un üstündeki kar zemini.
pub const DOCK_KEY: isize = -1;

/// Fare olayı türü (`input::WM_APP_SNOW` wparam'ı; 2 hareket).
pub const DOWN: usize = 0;
pub const UP: usize = 1;

struct BallVis {
    id: u32,
    vis: SpriteVisual,
    back: bool,
}

struct ManVis {
    id: u32,
    vis: SpriteVisual,
    back: bool,
    /// Çizildiği yarıçaplar (eridikçe yeniden çizilir).
    rs: [f32; 3],
    size: (f32, f32),
}

pub struct SnowPlay {
    pub snow: Snow,
    s: f32,
    mon: RECT,
    drift: isize,
    ball_brush: Option<CompositionSurfaceBrush>,
    balls: Vec<BallVis>,
    men: Vec<ManVis>,
    /// Son fare konumu (itme yönü için).
    last: Option<(i32, i32)>,
}

fn col(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// Karın `f` zemininde `x`'teki yüzeyi (ekran y'si).
pub fn surface(model: &Ledges, s: f32, f: &Floor, x: f32) -> f32 {
    match model.list.iter().find(|l| l.edge.key == f.key) {
        Some(l) => l.edge.y as f32 + 0.5 * s - l.height_at(x),
        None => f.y,
    }
}

/// Basılan yer dock ya da masaüstü mü (uygulama penceresi değil)?
unsafe fn open_spot(x: i32, y: i32, dock: HWND) -> bool {
    let h = WindowFromPoint(POINT { x, y });
    if h.0.is_null() {
        return false;
    }
    let root = GetAncestor(h, GA_ROOT);
    if root == dock {
        return true;
    }
    let c = carlui_core::win::class_name(root);
    c == "Progman" || c == "WorkerW"
}

/// Gölgeli kar küresi.
unsafe fn sphere(dc: &ID2D1DeviceContext, c: D2D_POINT_2F, r: f32, s: f32) -> Result<()> {
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: col(255, 255, 255, 1.0) },
        D2D1_GRADIENT_STOP { position: 0.6, color: col(238, 243, 251, 1.0) },
        D2D1_GRADIENT_STOP { position: 1.0, color: col(186, 202, 228, 1.0) },
    ];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: c, gradientOriginOffset: pt(-0.35 * r, -0.4 * r), radiusX: r * 1.05, radiusY: r * 1.05 };
    let b = dc.CreateRadialGradientBrush(&props, None, &coll)?;
    let e = D2D1_ELLIPSE { point: c, radiusX: r, radiusY: r };
    dc.FillEllipse(&e, &b);
    let rim = dc.CreateSolidColorBrush(&col(150, 170, 205, 0.55), None)?;
    dc.DrawEllipse(&e, &rim, (0.6 * s).max(0.8), None);
    Ok(())
}

/// Top bitmap'i (fırçayla boyutlandırılır).
unsafe fn draw_ball(dc: &ID2D1DeviceContext) -> Result<()> {
    sphere(dc, pt(32.0, 32.0), 30.5, 2.0)
}

/// Kardan adam: kömür gözler ve düğmeler, havuç burun, kırmızı atkı, dal kollar.
/// `(bx, by)`: bitmap'te adamın taban ortası.
unsafe fn draw_man(dc: &ID2D1DeviceContext, m: &Man, bx: f32, by: f32, s: f32) -> Result<()> {
    let cs = m.centers().map(|(x, y)| pt(x - m.x + bx, y - m.y + by));
    let [a, b, c] = m.rs;
    let (mid, head) = (cs[1], cs[2]);
    let wood = dc.CreateSolidColorBrush(&col(96, 66, 40, 1.0), None)?;
    let arm_w = (0.13 * b).clamp(1.2 * s, 2.6 * s);
    for dir in [-1.0f32, 1.0] {
        let p0 = pt(mid.x + dir * 0.6 * b, mid.y - 0.1 * b);
        let p1 = pt(mid.x + dir * 1.95 * b, mid.y - 0.85 * b);
        dc.DrawLine(p0, p1, &wood, arm_w, None);
        // Dal ucunda iki parmak.
        let k = pt(p0.x + (p1.x - p0.x) * 0.72, p0.y + (p1.y - p0.y) * 0.72);
        dc.DrawLine(k, pt(k.x + dir * 0.15 * b, k.y - 0.45 * b), &wood, arm_w * 0.75, None);
        dc.DrawLine(p1, pt(p1.x + dir * 0.35 * b, p1.y + 0.05 * b), &wood, arm_w * 0.7, None);
    }
    sphere(dc, cs[0], a, s)?;
    sphere(dc, mid, b, s)?;
    sphere(dc, head, c, s)?;
    let coal = dc.CreateSolidColorBrush(&col(34, 34, 40, 1.0), None)?;
    let dot = |p: D2D_POINT_2F, r: f32| D2D1_ELLIPSE { point: p, radiusX: r, radiusY: r };
    // Düğmeler.
    let br = (0.11 * b).max(1.2 * s);
    for k in [-0.3f32, 0.12, 0.52] {
        dc.FillEllipse(&dot(pt(mid.x, mid.y + k * b), br), &coal);
    }
    dc.FillEllipse(&dot(pt(cs[0].x, cs[0].y - 0.3 * a), br * 1.05), &coal);
    // Gözler ve ağız.
    let er = (0.12 * c).max(1.1 * s);
    for dir in [-1.0f32, 1.0] {
        dc.FillEllipse(&dot(pt(head.x + dir * 0.34 * c, head.y - 0.22 * c), er), &coal);
    }
    let mr = (0.065 * c).max(0.7 * s);
    for (dx, dy) in [(-0.34f32, 0.3f32), (-0.12, 0.42), (0.12, 0.42), (0.34, 0.3)] {
        dc.FillEllipse(&dot(pt(head.x + dx * c, head.y + dy * c), mr), &coal);
    }
    // Havuç burun.
    let f = dc.GetFactory()?;
    let g = f.CreatePathGeometry()?;
    let sink = g.Open()?;
    sink.BeginFigure(pt(head.x, head.y - 0.12 * c), D2D1_FIGURE_BEGIN_FILLED);
    sink.AddLine(pt(head.x + 1.05 * c, head.y + 0.02 * c));
    sink.AddLine(pt(head.x, head.y + 0.14 * c));
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close()?;
    dc.FillGeometry(&g, &dc.CreateSolidColorBrush(&col(240, 128, 36, 1.0), None)?, None);
    // Atkı: boyunda bant ve sarkan uç.
    let red = dc.CreateSolidColorBrush(&col(206, 38, 46, 1.0), None)?;
    let dark = dc.CreateSolidColorBrush(&col(150, 22, 32, 1.0), None)?;
    let ny = head.y + 0.82 * c;
    let band = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: head.x - 1.0 * c, top: ny - 0.24 * c, right: head.x + 1.0 * c, bottom: ny + 0.24 * c }, radiusX: 0.2 * c, radiusY: 0.2 * c };
    dc.FillRoundedRectangle(&band, &red);
    let tail = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: head.x + 0.3 * c, top: ny, right: head.x + 0.72 * c, bottom: ny + 1.05 * c }, radiusX: 0.12 * c, radiusY: 0.12 * c };
    dc.FillRoundedRectangle(&tail, &red);
    for k in [0.45f32, 0.75] {
        dc.FillRectangle(&D2D_RECT_F { left: head.x + 0.3 * c, top: ny + k * c, right: head.x + 0.72 * c, bottom: ny + (k + 0.08) * c }, &dark);
    }
    dc.FillRectangle(&D2D_RECT_F { left: head.x - 0.6 * c, top: ny - 0.04 * c, right: head.x + 0.6 * c, bottom: ny + 0.04 * c }, &dark);
    Ok(())
}

impl SnowPlay {
    pub fn new(s: f32, mon: RECT, drift: isize) -> Self {
        Self { snow: Snow::new(s, mon.bottom as f32 + 60.0 * s), s, mon, drift, ball_brush: None, balls: Vec::new(), men: Vec::new(), last: None }
    }

    pub fn is_empty(&self) -> bool {
        self.snow.is_empty()
    }

    /// Sık kare gerekiyor (hareket eden ya da tutulan top var).
    pub fn busy(&self) -> bool {
        self.snow.busy()
    }

    /// Mevcut zeminler: dock (altta ise) ve ekranın alt kenarı.
    pub fn floors(&self, pill: Option<RECT>) -> Vec<Floor> {
        let mut v = Vec::with_capacity(2);
        if let Some(p) = pill {
            if p.right - p.left > 20 {
                v.push(Floor { key: DOCK_KEY, x0: p.left as f32, x1: p.right as f32, y: p.top as f32, wall: false });
            }
        }
        v.push(Floor { key: self.drift, x0: self.mon.left as f32, x1: self.mon.right as f32, y: self.mon.bottom as f32, wall: true });
        v
    }

    /// Bir fare olayı. Bir şey değiştiyse (kare gerekiyorsa) `true`.
    pub fn mouse(&mut self, kind: usize, x: i32, y: i32, dock: HWND, pill: Option<RECT>, model: &mut Ledges) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        let prev = self.last.replace((x, y));
        match kind {
            DOWN => {
                if !unsafe { open_spot(x, y, dock) } {
                    return false;
                }
                match self.snow.press(fx, fy) {
                    Press::Ball(_) | Press::Man => return true,
                    Press::Nothing => {}
                }
                let s = self.s;
                for f in self.floors(pill) {
                    if fx < f.x0 || fx > f.x1 {
                        continue;
                    }
                    let Some(l) = model.list.iter().find(|l| l.edge.key == f.key) else { continue };
                    let h = l.height_at(fx);
                    let top = f.y - h - 6.0 * s;
                    // Dock: pill'in üst kenarına dek; alt birikinti: ekranın dibine dek.
                    let bottom = if f.key == DOCK_KEY { f.y + 2.0 } else { f.y };
                    if h < 2.0 * s || fy < top || fy > bottom {
                        continue;
                    }
                    let r = (4.0 * s + 1.2 * h).clamp(7.0 * s, 12.0 * s);
                    model.clear(f.key, fx - r, fx + r);
                    self.snow.scoop(fx, fy, r);
                    return true;
                }
                false
            }
            UP => {
                if self.snow.holding() {
                    self.snow.release();
                    return true;
                }
                false
            }
            _ => {
                if self.snow.holding() {
                    self.snow.drag(fx, fy);
                    return true;
                }
                let s = self.s;
                let near = self.snow.balls.iter().any(|b| {
                    let (dx, dy) = (fx - b.x, fy - b.y);
                    dx * dx + dy * dy <= (b.r + 2.0 * s).powi(2)
                });
                if !near || !unsafe { open_spot(x, y, dock) } {
                    return false;
                }
                let dx = prev.map_or(0.0, |(px, _)| (x - px) as f32);
                self.snow.push(fx, fy, dx);
                true
            }
        }
    }

    /// Fizik ve görseller. `snowing`: kar yağmıyorsa toplar erir.
    /// `front`: dock'un altındaki katman; `back`: pencerelerin arkasındaki katman.
    pub fn step(&mut self, gfx: &Gfx, dt: f32, snowing: bool, pill: Option<RECT>, model: &Ledges, front: &ContainerVisual, back: Option<&ContainerVisual>) -> Result<()> {
        if self.snow.is_empty() && self.balls.is_empty() && self.men.is_empty() {
            return Ok(());
        }
        let floors = self.floors(pill);
        let s = self.s;
        if !snowing {
            self.snow.melt(dt);
        }
        self.snow.step(dt, &floors, |f, x| surface(model, s, f, x));
        self.snow.follow_floors(&floors, |f, x| surface(model, s, f, x));
        self.draw(gfx, front, back)
    }

    /// Topun en alttaki zemini (yığındaysa altındakinin).
    fn base(&self, b: &Ball) -> Option<isize> {
        let mut cur = *b;
        for _ in 0..16 {
            match cur.state {
                State::Stacked(id) => cur = *self.snow.balls.iter().find(|o| o.id == id)?,
                _ => return cur.floor(),
            }
        }
        None
    }

    fn draw(&mut self, gfx: &Gfx, front: &ContainerVisual, back: Option<&ContainerVisual>) -> Result<()> {
        let (ml, mt) = (self.mon.left as f32, self.mon.top as f32);
        let parent = |b: bool| if b { back.unwrap_or(front) } else { front };
        // Toplar.
        let ids: Vec<u32> = self.snow.balls.iter().map(|b| b.id).collect();
        for v in self.balls.iter().filter(|v| !ids.contains(&v.id)) {
            let _ = parent(v.back).Children()?.Remove(&v.vis);
        }
        self.balls.retain(|v| ids.contains(&v.id));
        if !self.snow.balls.is_empty() && self.ball_brush.is_none() {
            let surf = gfx.surface(64.0, 64.0, |dc| unsafe { draw_ball(dc) })?;
            let b = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
            b.SetStretch(CompositionStretch::Fill)?;
            self.ball_brush = Some(b);
        }
        for i in 0..self.snow.balls.len() {
            let b = self.snow.balls[i];
            let want_back = back.is_some() && self.base(&b) == Some(self.drift);
            let k = match self.balls.iter().position(|v| v.id == b.id) {
                Some(k) => k,
                None => {
                    let vis = gfx.comp.CreateSpriteVisual()?;
                    if let Some(br) = self.ball_brush.as_ref() {
                        vis.SetBrush(br)?;
                    }
                    parent(want_back).Children()?.InsertAtTop(&vis)?;
                    self.balls.push(BallVis { id: b.id, vis, back: want_back });
                    self.balls.len() - 1
                }
            };
            let v = &mut self.balls[k];
            if v.back != want_back {
                let _ = parent(v.back).Children()?.Remove(&v.vis);
                parent(want_back).Children()?.InsertAtTop(&v.vis)?;
                v.back = want_back;
            }
            let d = 2.0 * b.r;
            v.vis.SetSize(v2(d, d))?;
            v.vis.SetOffset(v3(b.x - b.r - ml, b.y - b.r - mt))?;
        }
        // Kardan adamlar.
        let ids: Vec<u32> = self.snow.men.iter().map(|m| m.id).collect();
        for v in self.men.iter().filter(|v| !ids.contains(&v.id)) {
            let _ = parent(v.back).Children()?.Remove(&v.vis);
        }
        self.men.retain(|v| ids.contains(&v.id));
        let s = self.s;
        for i in 0..self.snow.men.len() {
            let m = self.snow.men[i];
            let want_back = back.is_some() && m.key == self.drift;
            let k = self.men.iter().position(|v| v.id == m.id);
            let stale = k.is_none_or(|k| self.men[k].rs.iter().zip(m.rs).any(|(a, b)| (a - b).abs() > 1.0));
            if stale {
                let w = (2.0 * m.rs[0]).max(2.0 * 2.4 * m.rs[1]) + 4.0 * s;
                let h = m.height() + 3.0 * s;
                let (bx, by) = (w / 2.0, h - 1.0 * s);
                let surf = gfx.surface(w.ceil(), h.ceil(), |dc| unsafe { draw_man(dc, &m, bx, by, s) })?;
                let brush = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
                brush.SetStretch(CompositionStretch::None)?;
                brush.SetHorizontalAlignmentRatio(0.0)?;
                brush.SetVerticalAlignmentRatio(0.0)?;
                match k {
                    Some(k) => {
                        let v = &mut self.men[k];
                        v.vis.SetBrush(&brush)?;
                        v.rs = m.rs;
                        v.size = (w.ceil(), h.ceil());
                    }
                    None => {
                        let vis = gfx.comp.CreateSpriteVisual()?;
                        vis.SetBrush(&brush)?;
                        parent(want_back).Children()?.InsertAtTop(&vis)?;
                        self.men.push(ManVis { id: m.id, vis, back: want_back, rs: m.rs, size: (w.ceil(), h.ceil()) });
                    }
                }
            }
            let Some(v) = self.men.iter_mut().find(|v| v.id == m.id) else { continue };
            if v.back != want_back {
                let _ = parent(v.back).Children()?.Remove(&v.vis);
                parent(want_back).Children()?.InsertAtTop(&v.vis)?;
                v.back = want_back;
            }
            v.vis.SetSize(v2(v.size.0, v.size.1))?;
            v.vis.SetOffset(v3(m.x - v.size.0 / 2.0 - ml, m.y - (v.size.1 - 1.0 * s) - mt))?;
        }
        Ok(())
    }

    /// Her şeyi siler (görseller dahil).
    pub fn reset(&mut self, front: &ContainerVisual, back: Option<&ContainerVisual>) {
        self.snow.reset();
        for v in self.balls.drain(..) {
            for p in [Some(front), back].into_iter().flatten() {
                if let Ok(k) = p.Children() {
                    let _ = k.Remove(&v.vis);
                }
            }
        }
        for v in self.men.drain(..) {
            for p in [Some(front), back].into_iter().flatten() {
                if let Ok(k) = p.Children() {
                    let _ = k.Remove(&v.vis);
                }
            }
        }
    }
}
