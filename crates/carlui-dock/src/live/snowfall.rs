//! Ekranın tamamına yağan ince kar ve arada alt kenardan geçen kar küreme
//! aracı. Kar tanelerinin hepsi kompozisyon animasyonudur: bir kez kurulur,
//! sonra karelerde hiç işlemci harcamaz.

use super::rng::Rng;
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use windows::{
    core::*,
    Foundation::{
        Numerics::{Matrix3x2, Vector3},
        TimeSpan,
    },
    Win32::Graphics::Direct2D::{Common::*, *},
    UI::Composition::*,
};

fn ts(ms: f32) -> TimeSpan {
    TimeSpan { Duration: (ms.max(1.0) * 10_000.0) as i64 }
}

fn col(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// Monitör genişliğine ve şiddete göre tane sayısı.
pub fn flake_count(level: f32, w: f32, s: f32) -> usize {
    let per = 70.0 + 190.0 * level.clamp(0.0, 1.0);
    ((per * w / (1920.0 * s.max(0.5))) as usize).clamp(24, 480)
}

/// Yumuşak kenarlı beyaz nokta.
unsafe fn draw_dot(dc: &ID2D1DeviceContext) -> Result<()> {
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: col(255, 255, 255, 1.0) },
        D2D1_GRADIENT_STOP { position: 0.55, color: col(255, 255, 255, 0.85) },
        D2D1_GRADIENT_STOP { position: 1.0, color: col(240, 246, 255, 0.0) },
    ];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: pt(8.0, 8.0), gradientOriginOffset: pt(0.0, 0.0), radiusX: 8.0, radiusY: 8.0 };
    let b = dc.CreateRadialGradientBrush(&props, None, &coll)?;
    dc.FillEllipse(&D2D1_ELLIPSE { point: pt(8.0, 8.0), radiusX: 8.0, radiusY: 8.0 }, &b);
    Ok(())
}

/// Yağan kar katmanı.
pub struct Snowfall {
    parent: ContainerVisual,
    root: ContainerVisual,
    pub level: f32,
}

impl Snowfall {
    /// `w`×`h` alanına `level` şiddetinde kar yağdırır; katman `parent`'ın en altına girer.
    pub fn new(gfx: &Gfx, parent: &ContainerVisual, w: f32, h: f32, s: f32, level: f32, rng: &mut Rng) -> Result<Self> {
        let comp = &gfx.comp;
        let root = comp.CreateContainerVisual()?;
        let dot = gfx.surface(16.0, 16.0, |dc| unsafe { draw_dot(dc) })?;
        let brush = comp.CreateSurfaceBrushWithSurface(&dot)?;
        brush.SetStretch(CompositionStretch::Fill)?;
        let lin = comp.CreateLinearEasingFunction()?;
        let kids = root.Children()?;
        let top = -12.0 * s;
        let len = h + 24.0 * s;
        for _ in 0..flake_count(level, w, s) {
            // Küçük taneler ağır ve soluk düşer; büyükler daha hızlı ve parlak.
            let big = rng.f();
            let d = (2.6 + 5.0 * big * big) * s;
            let speed = (28.0 + 46.0 * big + rng.range(-6.0, 6.0)) * s;
            let dur = len / speed * 1000.0;
            let dx = rng.range(-0.06, 0.16) * len;
            let x0 = rng.range(-0.12 * w, 1.06 * w);
            let p0 = rng.f().clamp(0.002, 0.998);
            let at = |p: f32| Vector3 { X: x0 + dx * p, Y: top + len * p, Z: 0.0 };

            let outer = comp.CreateContainerVisual()?;
            let fall = comp.CreateVector3KeyFrameAnimation()?;
            // Rastgele evreden başla: tabana in, tepeye sıçra, evreye dön.
            fall.InsertKeyFrameWithEasingFunction(0.0, at(p0), &lin)?;
            fall.InsertKeyFrameWithEasingFunction(1.0 - p0, at(1.0), &lin)?;
            fall.InsertKeyFrameWithEasingFunction((1.0 - p0 + 0.0004).min(0.9999), at(0.0), &lin)?;
            fall.InsertKeyFrameWithEasingFunction(1.0, at(p0), &lin)?;
            fall.SetDuration(ts(dur))?;
            fall.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
            outer.SetOffset(at(p0))?;
            outer.StartAnimation(h!("Offset"), &fall)?;

            let flake = comp.CreateSpriteVisual()?;
            flake.SetBrush(&brush)?;
            flake.SetSize(v2(d, d))?;
            flake.SetOpacity((0.6 + 0.4 * big + rng.range(0.0, 0.1)).min(1.0))?;
            // Sağa sola salınım.
            let amp = rng.range(3.0, 9.0) * s;
            let sway = comp.CreateScalarKeyFrameAnimation()?;
            sway.InsertKeyFrame(0.0, -amp)?;
            sway.InsertKeyFrame(1.0, amp)?;
            sway.SetDuration(ts(rng.range(1800.0, 4200.0)))?;
            sway.SetDelayTime(ts(rng.range(0.0, 2000.0)))?;
            sway.SetDirection(AnimationDirection::Alternate)?;
            sway.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
            flake.StartAnimation(h!("Offset.X"), &sway)?;
            outer.Children()?.InsertAtTop(&flake)?;
            kids.InsertAtTop(&outer)?;
        }
        // Yavaşça belirsin.
        let fade = comp.CreateScalarKeyFrameAnimation()?;
        fade.InsertKeyFrame(0.0, 0.0)?;
        fade.InsertKeyFrame(1.0, 1.0)?;
        fade.SetDuration(ts(2500.0))?;
        root.SetOpacity(1.0)?;
        root.StartAnimation(h!("Opacity"), &fade)?;
        parent.Children()?.InsertAtBottom(&root)?;
        Ok(Self { parent: parent.clone(), root, level })
    }
}

impl Drop for Snowfall {
    fn drop(&mut self) {
        if let Ok(kids) = self.parent.Children() {
            let _ = kids.Remove(&self.root);
        }
    }
}

/// Küreme aracının tasarım boyutu (ölçek 1'de).
pub const PLOW_W: f32 = 100.0;
pub const PLOW_H: f32 = 46.0;
/// Bıçağın ön ucu (araç bitmap'inde x).
pub const PLOW_BLADE: f32 = 24.0;
/// Hız (ölçek 1'de piksel/sn).
const PLOW_SPEED: f32 = 115.0;

/// Sola giden sarı kar küreme aracı; önünde kar yığını.
unsafe fn draw_plow(dc: &ID2D1DeviceContext, s: f32) -> Result<()> {
    let mut m = Matrix3x2::default();
    dc.GetTransform(&mut m);
    dc.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: m.M31, M32: m.M32 });
    let solid = |c: D2D1_COLOR_F| dc.CreateSolidColorBrush(&c, None);
    let rr = |l: f32, t: f32, r: f32, b: f32, k: f32| D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b }, radiusX: k, radiusY: k };
    let f = dc.GetFactory()?;

    // Bıçağın önünde itilen kar.
    let pile = f.CreatePathGeometry()?;
    let sink = pile.Open()?;
    sink.BeginFigure(pt(0.0, 46.0), D2D1_FIGURE_BEGIN_FILLED);
    sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: pt(6.0, 40.0), point2: pt(10.0, 24.0), point3: pt(17.0, 14.0) });
    sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: pt(20.0, 10.0), point2: pt(24.0, 10.0), point3: pt(26.0, 13.0) });
    sink.AddLine(pt(26.0, 46.0));
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close()?;
    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: col(255, 255, 255, 1.0) }, D2D1_GRADIENT_STOP { position: 1.0, color: col(206, 218, 238, 1.0) }];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let snow = dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(8.0, 12.0), endPoint: pt(20.0, 46.0) }, None, &coll)?;

    // Paletler ve tekerler.
    dc.FillRoundedRectangle(&rr(36.0, 33.0, 97.0, 46.0, 6.5), &solid(col(44, 44, 54, 1.0))?);
    let wheel = solid(col(178, 182, 192, 1.0))?;
    let hub = solid(col(60, 62, 72, 1.0))?;
    for x in [45.0, 59.0, 73.0, 87.0] {
        dc.FillEllipse(&D2D1_ELLIPSE { point: pt(x, 39.5), radiusX: 4.6, radiusY: 4.6 }, &wheel);
        dc.FillEllipse(&D2D1_ELLIPSE { point: pt(x, 39.5), radiusX: 1.6, radiusY: 1.6 }, &hub);
    }
    // Gövde, kabin, cam, tepe lambası.
    let yellow = solid(col(250, 196, 38, 1.0))?;
    let dark_yellow = solid(col(214, 150, 20, 1.0))?;
    dc.FillRoundedRectangle(&rr(36.0, 21.0, 97.0, 34.0, 3.0), &yellow);
    dc.FillRectangle(&D2D_RECT_F { left: 36.0, top: 31.0, right: 97.0, bottom: 34.0 }, &dark_yellow);
    dc.FillRoundedRectangle(&rr(50.0, 4.0, 85.0, 23.0, 3.0), &yellow);
    dc.FillRoundedRectangle(&rr(54.0, 8.0, 81.0, 19.0, 1.5), &solid(col(150, 206, 242, 1.0))?);
    dc.FillRectangle(&D2D_RECT_F { left: 56.0, top: 9.0, right: 62.0, bottom: 11.0 }, &solid(col(230, 246, 255, 0.9))?);
    dc.FillRoundedRectangle(&rr(64.0, 0.5, 71.0, 4.5, 1.5), &solid(col(255, 120, 40, 1.0))?);
    // Kol ve bıçak.
    dc.FillRectangle(&D2D_RECT_F { left: 29.0, top: 27.0, right: 37.0, bottom: 31.0 }, &solid(col(70, 72, 82, 1.0))?);
    dc.FillRoundedRectangle(&rr(24.0, 13.0, 30.0, 44.0, 1.5), &solid(col(150, 156, 168, 1.0))?);
    dc.FillRectangle(&D2D_RECT_F { left: 28.0, top: 14.0, right: 30.0, bottom: 43.0 }, &solid(col(108, 114, 126, 1.0))?);
    dc.FillGeometry(&pile, &snow, None);
    Ok(())
}

/// Alt kenardan geçen küreme aracı.
pub struct Plow {
    parent: ContainerVisual,
    vis: SpriteVisual,
    x_from: f32,
    x_to: f32,
    run: f32,
    elapsed: f32,
    s: f32,
}

impl Plow {
    /// Sağdan girip sola çıkar; `w`, `h` katmanın boyutu.
    pub fn new(gfx: &Gfx, parent: &ContainerVisual, w: f32, h: f32, s: f32) -> Result<Self> {
        let (pw, ph) = ((PLOW_W * s).ceil(), (PLOW_H * s).ceil());
        let surf = gfx.surface(pw, ph, |dc| unsafe { draw_plow(dc, s) })?;
        let brush = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
        let vis = gfx.comp.CreateSpriteVisual()?;
        vis.SetBrush(&brush)?;
        vis.SetSize(v2(pw, ph))?;
        let (x_from, x_to) = (w + 8.0 * s, -pw - 8.0 * s);
        let y = h - ph;
        let run = (x_from - x_to) / (PLOW_SPEED * s);
        let lin = gfx.comp.CreateLinearEasingFunction()?;
        let a = gfx.comp.CreateVector3KeyFrameAnimation()?;
        a.InsertKeyFrameWithEasingFunction(0.0, Vector3 { X: x_from, Y: y, Z: 0.0 }, &lin)?;
        a.InsertKeyFrameWithEasingFunction(1.0, Vector3 { X: x_to, Y: y, Z: 0.0 }, &lin)?;
        a.SetDuration(ts(run * 1000.0))?;
        vis.SetOffset(v3(x_from, y))?;
        vis.StartAnimation(h!("Offset"), &a)?;
        parent.Children()?.InsertAtTop(&vis)?;
        Ok(Self { parent: parent.clone(), vis, x_from, x_to, run, elapsed: 0.0, s })
    }

    /// Zamanı ilerletir; bıçağın arkasında temizlenmesi gereken katman-x
    /// aralığını verir. Bitti ise `None`.
    pub fn advance(&mut self, dt: f32) -> Option<(f32, f32)> {
        self.elapsed += dt.max(0.0);
        if self.elapsed >= self.run {
            return None;
        }
        let x = self.x_from + (self.x_to - self.x_from) * (self.elapsed / self.run);
        let blade = x + PLOW_BLADE * self.s;
        Some((blade, blade + (PLOW_W - PLOW_BLADE + 30.0) * self.s))
    }
}

impl Drop for Plow {
    fn drop(&mut self) {
        if let Ok(kids) = self.parent.Children() {
            let _ = kids.Remove(&self.vis);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tane_sayisi_sinirli() {
        assert!(flake_count(0.15, 1920.0, 1.0) < flake_count(1.0, 1920.0, 1.0));
        assert_eq!(flake_count(1.0, 100_000.0, 1.0), 480);
        assert_eq!(flake_count(0.0, 10.0, 2.0), 24);
        assert!(flake_count(f32::NAN, 1920.0, 1.0) >= 24);
        // 1920 genişlikte, 0.8 şiddette ekran dolusu kar.
        assert!(flake_count(0.8, 1920.0, 1.0) >= 200);
    }
}

/// Taşınan pencereden dökülen kar: kısa ömürlü, kompozisyonla düşen parçacıklar.
pub struct Spill {
    parent: ContainerVisual,
    brush: Option<CompositionSurfaceBrush>,
    bursts: Vec<(ContainerVisual, std::time::Instant)>,
}

/// Parçacığın en uzun ömrü (ms).
const SPILL_MS: f32 = 1100.0;

impl Spill {
    pub fn new(parent: &ContainerVisual) -> Self {
        Self { parent: parent.clone(), brush: None, bursts: Vec::new() }
    }

    /// `[x0, x1]` aralığında, `y` kenarından (`h`: kar kalınlığı) `frac` oranında
    /// kar döker; `dx`: pencerenin gittiği yön (parçalar geride kalır).
    #[allow(clippy::too_many_arguments)]
    pub fn burst(&mut self, gfx: &Gfx, x0: f32, x1: f32, y: f32, h: f32, frac: f32, dx: f32, s: f32, rng: &mut Rng) -> Result<()> {
        let w = (x1 - x0).max(0.0);
        let n = ((w / (9.0 * s)) * frac.clamp(0.0, 1.0) * 2.2).clamp(3.0, 70.0) as usize;
        if w < 4.0 || h < 0.5 {
            return Ok(());
        }
        let comp = &gfx.comp;
        if self.brush.is_none() {
            let dot = gfx.surface(16.0, 16.0, |dc| unsafe { draw_dot(dc) })?;
            let b = comp.CreateSurfaceBrushWithSurface(&dot)?;
            b.SetStretch(CompositionStretch::Fill)?;
            self.brush = Some(b);
        }
        let brush = self.brush.clone().unwrap();
        let root = comp.CreateContainerVisual()?;
        let fall = comp.CreateCubicBezierEasingFunction(windows::Foundation::Numerics::Vector2 { X: 0.45, Y: 0.0 }, windows::Foundation::Numerics::Vector2 { X: 0.9, Y: 0.6 })?;
        let lin = comp.CreateLinearEasingFunction()?;
        // Pencere hızla giderse parçalar geride kalır (ters yöne savrulur).
        let back = -(dx.clamp(-80.0, 80.0)) * 0.6;
        let kids = root.Children()?;
        for _ in 0..n {
            let x = rng.range(x0, x1);
            let y0 = y - rng.range(0.0, h.max(1.0));
            let d = rng.range(2.2, 5.5) * s;
            let ms = rng.range(650.0, SPILL_MS);
            let drop = rng.range(110.0, 260.0) * s;
            let x_end = x + back * s + rng.range(-14.0, 14.0) * s;
            let v = comp.CreateSpriteVisual()?;
            v.SetBrush(&brush)?;
            v.SetSize(v2(d, d))?;
            v.SetOffset(v3(x - d / 2.0, y0 - d / 2.0))?;
            let mv = comp.CreateVector3KeyFrameAnimation()?;
            mv.InsertKeyFrameWithEasingFunction(1.0, Vector3 { X: x_end - d / 2.0, Y: y0 + drop, Z: 0.0 }, &fall)?;
            mv.SetDuration(ts(ms))?;
            v.StartAnimation(h!("Offset"), &mv)?;
            let op = comp.CreateScalarKeyFrameAnimation()?;
            op.InsertKeyFrameWithEasingFunction(0.0, 0.95, &lin)?;
            op.InsertKeyFrameWithEasingFunction(0.6, 0.85, &lin)?;
            op.InsertKeyFrameWithEasingFunction(1.0, 0.0, &lin)?;
            op.SetDuration(ts(ms))?;
            v.StartAnimation(h!("Opacity"), &op)?;
            kids.InsertAtTop(&v)?;
        }
        self.parent.Children()?.InsertAtTop(&root)?;
        self.bursts.push((root, std::time::Instant::now()));
        Ok(())
    }

    /// Ömrü dolan parçacıkları kaldırır.
    pub fn sweep(&mut self) {
        let parent = &self.parent;
        self.bursts.retain(|(r, t)| {
            let alive = t.elapsed().as_secs_f32() * 1000.0 < SPILL_MS + 100.0;
            if !alive {
                if let Ok(k) = parent.Children() {
                    let _ = k.Remove(r);
                }
            }
            alive
        });
    }

    pub fn busy(&self) -> bool {
        !self.bursts.is_empty()
    }
}

impl Drop for Spill {
    fn drop(&mut self) {
        if let Ok(k) = self.parent.Children() {
            for (r, _) in &self.bursts {
                let _ = k.Remove(r);
            }
        }
    }
}
