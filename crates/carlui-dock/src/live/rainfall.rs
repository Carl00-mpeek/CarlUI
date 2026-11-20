//! Ekranın tamamına yağan yağmur ve yıldırım. Yağmur çizgilerinin hepsi
//! kompozisyon animasyonudur: bir kez kurulur, karelerde işlemci harcamaz.
//! Yıldırım seyrek gelir: masaüstü katmanında bir şimşek çizgisi ve parlama,
//! pencerelerin üstünde de çok hafif bir aydınlanma.

use super::rng::Rng;
use carlui_core::live::RainStrength;
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use std::time::{Duration, Instant};
use windows::{
    core::*,
    Foundation::{Numerics::Vector3, TimeSpan},
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

/// Monitör genişliğine, şiddete ve seçilen yoğunluğa göre damla sayısı.
pub fn drop_count(strength: RainStrength, level: f32, w: f32, s: f32) -> usize {
    let l = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.6 };
    let (base, extra, cap) = match strength {
        RainStrength::Light => (60.0, 80.0, 260),
        RainStrength::Normal => (130.0, 170.0, 520),
        RainStrength::Heavy => (260.0, 320.0, 900),
    };
    (((base + extra * l) * w / (1920.0 * s.max(0.5))) as usize).clamp(30, cap)
}

/// Bir sonraki yıldırıma kadar beklenecek süre (sn); hafif yağmurda yıldırım yok.
pub fn bolt_gap(strength: RainStrength, rng: &mut Rng) -> Option<f32> {
    match strength {
        RainStrength::Light => None,
        RainStrength::Normal => Some(rng.range(28.0, 65.0)),
        RainStrength::Heavy => Some(rng.range(7.0, 18.0)),
    }
}

/// Yağmur çizgisi (4×64): tepede saydam, dibe doğru belirginleşen ince iz.
unsafe fn draw_streak(dc: &ID2D1DeviceContext) -> Result<()> {
    let stops = [
        D2D1_GRADIENT_STOP { position: 0.0, color: col(200, 215, 235, 0.0) },
        D2D1_GRADIENT_STOP { position: 0.75, color: col(215, 228, 245, 0.55) },
        D2D1_GRADIENT_STOP { position: 1.0, color: col(240, 247, 255, 0.95) },
    ];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(2.0, 0.0), endPoint: pt(2.0, 64.0) };
    let b = dc.CreateLinearGradientBrush(&props, None, &coll)?;
    dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: 0.5, top: 0.0, right: 3.5, bottom: 64.0 }, radiusX: 1.5, radiusY: 1.5 }, &b);
    Ok(())
}

/// Yağan yağmur katmanı.
pub struct Rainfall {
    parent: ContainerVisual,
    root: ContainerVisual,
    pub level: f32,
}

impl Rainfall {
    /// `w`×`h` alanına yağmur yağdırır; katman `parent`'ın en altına girer.
    /// Uzaktaki damlalar kısa, ince ve soluk; yakındakiler uzun, parlak ve hızlı.
    #[allow(clippy::too_many_arguments)]
    pub fn new(gfx: &Gfx, parent: &ContainerVisual, w: f32, h: f32, s: f32, level: f32, strength: RainStrength, rng: &mut Rng) -> Result<Self> {
        let comp = &gfx.comp;
        let root = comp.CreateContainerVisual()?;
        let surf = gfx.surface(4.0, 64.0, |dc| unsafe { draw_streak(dc) })?;
        let brush = comp.CreateSurfaceBrushWithSurface(&surf)?;
        brush.SetStretch(CompositionStretch::Fill)?;
        let lin = comp.CreateLinearEasingFunction()?;
        let far = comp.CreateContainerVisual()?;
        let near = comp.CreateContainerVisual()?;
        root.Children()?.InsertAtTop(&far)?;
        root.Children()?.InsertAtTop(&near)?;
        let (far_kids, near_kids) = (far.Children()?, near.Children()?);
        // Bütün damlalar aynı rüzgârla eğik düşer; yoğun yağmurda rüzgâr sertleşir.
        let tilt = match strength {
            RainStrength::Light => rng.range(0.04, 0.1),
            RainStrength::Normal => rng.range(0.08, 0.16),
            RainStrength::Heavy => rng.range(0.14, 0.24),
        };
        let angle = -tilt.atan().to_degrees();
        let bright = match strength {
            RainStrength::Light => 0.8,
            RainStrength::Normal => 0.9,
            RainStrength::Heavy => 1.0,
        };
        for _ in 0..drop_count(strength, level, w, s) {
            let depth = rng.f();
            let len = (14.0 + 34.0 * depth * depth + rng.range(-3.0, 3.0)) * s;
            let thick = (1.0 + 1.2 * depth) * s;
            let speed = (900.0 + 1000.0 * depth + rng.range(-80.0, 80.0)) * s;
            let top = -len - 4.0 * s;
            let travel = h + len + 8.0 * s;
            let dx = tilt * travel;
            let x0 = rng.range(-dx - 0.02 * w, 1.02 * w);
            let p0 = rng.f().clamp(0.002, 0.998);
            let at = |p: f32| Vector3 { X: x0 + dx * p, Y: top + travel * p, Z: 0.0 };

            let outer = comp.CreateContainerVisual()?;
            let fall = comp.CreateVector3KeyFrameAnimation()?;
            // Rastgele evreden başla: dibe in, tepeye sıçra, evreye dön.
            fall.InsertKeyFrameWithEasingFunction(0.0, at(p0), &lin)?;
            fall.InsertKeyFrameWithEasingFunction(1.0 - p0, at(1.0), &lin)?;
            fall.InsertKeyFrameWithEasingFunction((1.0 - p0 + 0.0004).min(0.9999), at(0.0), &lin)?;
            fall.InsertKeyFrameWithEasingFunction(1.0, at(p0), &lin)?;
            fall.SetDuration(ts(travel / speed * 1000.0))?;
            fall.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
            outer.SetOffset(at(p0))?;
            outer.StartAnimation(h!("Offset"), &fall)?;

            let drop = comp.CreateSpriteVisual()?;
            drop.SetBrush(&brush)?;
            drop.SetSize(v2(thick, len))?;
            drop.SetCenterPoint(Vector3 { X: thick / 2.0, Y: len / 2.0, Z: 0.0 })?;
            drop.SetRotationAngleInDegrees(angle)?;
            drop.SetOpacity(((0.14 + 0.4 * depth + rng.range(0.0, 0.08)) * bright).min(0.75))?;
            outer.Children()?.InsertAtTop(&drop)?;
            if depth < 0.5 {
                far_kids.InsertAtTop(&outer)?;
            } else {
                near_kids.InsertAtTop(&outer)?;
            }
        }
        // Yavaşça başlasın.
        let fade = comp.CreateScalarKeyFrameAnimation()?;
        fade.InsertKeyFrame(0.0, 0.0)?;
        fade.InsertKeyFrame(1.0, 1.0)?;
        fade.SetDuration(ts(2000.0))?;
        root.SetOpacity(1.0)?;
        root.StartAnimation(h!("Opacity"), &fade)?;
        parent.Children()?.InsertAtBottom(&root)?;
        Ok(Self { parent: parent.clone(), root, level })
    }
}

impl Drop for Rainfall {
    fn drop(&mut self) {
        if let Ok(kids) = self.parent.Children() {
            let _ = kids.Remove(&self.root);
        }
    }
}

/// Bir yıldırımın ömrü (ms).
const BOLT_MS: f32 = 950.0;

/// Kırık çizgi: tepeden aşağı inen gövde ve bir-iki dal (yüzey koordinatlarında).
fn bolt_paths(bw: f32, bh: f32, s: f32, rng: &mut Rng) -> Vec<Vec<(f32, f32)>> {
    let mut main = Vec::new();
    let mut x = bw / 2.0 + rng.range(-0.1, 0.1) * bw;
    let steps = 12 + rng.int(0, 6) as usize;
    let dy = bh / steps as f32;
    let drift = rng.range(-0.25, 0.25) * dy;
    for i in 0..=steps {
        main.push((x, i as f32 * dy));
        x = (x + drift + rng.range(-0.55, 0.55) * dy).clamp(8.0 * s, bw - 8.0 * s);
    }
    let mut out = vec![main.clone()];
    for _ in 0..rng.int(1, 3) {
        let from = rng.int(2, (steps / 2) as u32) as usize;
        let (mut bx, mut by) = main[from];
        let dir = if rng.f() < 0.5 { -1.0 } else { 1.0 };
        let mut br = vec![(bx, by)];
        for _ in 0..rng.int(3, 6) {
            bx = (bx + dir * rng.range(0.3, 0.9) * dy).clamp(4.0 * s, bw - 4.0 * s);
            by = (by + rng.range(0.5, 1.0) * dy).min(bh);
            br.push((bx, by));
        }
        out.push(br);
    }
    out
}

unsafe fn draw_bolt(dc: &ID2D1DeviceContext, paths: &[Vec<(f32, f32)>], s: f32) -> Result<()> {
    let f = dc.GetFactory()?;
    let props = D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() };
    let style = f.CreateStrokeStyle(&props, None)?;
    let glow = dc.CreateSolidColorBrush(&col(170, 190, 255, 0.22), None)?;
    let mid = dc.CreateSolidColorBrush(&col(205, 220, 255, 0.6), None)?;
    let core = dc.CreateSolidColorBrush(&col(255, 255, 255, 1.0), None)?;
    for (i, p) in paths.iter().enumerate() {
        if p.len() < 2 {
            continue;
        }
        let g = f.CreatePathGeometry()?;
        let sink = g.Open()?;
        sink.BeginFigure(pt(p[0].0, p[0].1), D2D1_FIGURE_BEGIN_HOLLOW);
        for &(x, y) in &p[1..] {
            sink.AddLine(pt(x, y));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        // Dallar gövdeden ince.
        let k = if i == 0 { 1.0 } else { 0.55 };
        dc.DrawGeometry(&g, &glow, 9.0 * s * k, &style);
        dc.DrawGeometry(&g, &mid, 3.6 * s * k, &style);
        dc.DrawGeometry(&g, &core, 1.6 * s * k, &style);
    }
    Ok(())
}

/// Çakma, sönme ve ikinci çakma: yıldırımın titreşen ışığı.
fn flicker(comp: &Compositor, peak: f32) -> Result<ScalarKeyFrameAnimation> {
    let a = comp.CreateScalarKeyFrameAnimation()?;
    for (t, v) in [(0.0, 0.0), (0.04, 1.0), (0.12, 0.15), (0.2, 0.9), (0.34, 0.45), (0.6, 0.05), (1.0, 0.0)] {
        a.InsertKeyFrame(t, v * peak)?;
    }
    a.SetDuration(ts(BOLT_MS))?;
    Ok(a)
}

/// Seyrek yıldırımlar.
pub struct Storm {
    strength: RainStrength,
    next: Instant,
    /// Söndükten sonra kaldırılacak görseller.
    live: Vec<(ContainerVisual, SpriteVisual, Instant)>,
}

impl Storm {
    /// Hafif yağmurda yıldırım yok: `None`.
    pub fn new(strength: RainStrength, now: Instant, rng: &mut Rng) -> Option<Self> {
        // İlki biraz daha erken gelsin.
        let first = bolt_gap(strength, rng)? * 0.4;
        Some(Self { strength, next: now + Duration::from_secs_f32(first), live: Vec::new() })
    }

    /// Vakti geldiyse yıldırım çakar; sönenleri kaldırır. `back`: masaüstü
    /// katmanı (şimşek ve parlama), `top`: pencerelerin üstü (hafif aydınlanma).
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, gfx: &Gfx, now: Instant, back: &ContainerVisual, top: &ContainerVisual, w: f32, h: f32, s: f32, rng: &mut Rng) -> Result<()> {
        self.live.retain(|(p, v, end)| {
            let alive = now < *end;
            if !alive {
                if let Ok(k) = p.Children() {
                    let _ = k.Remove(v);
                }
            }
            alive
        });
        if now < self.next {
            return Ok(());
        }
        self.next = now + Duration::from_secs_f32(bolt_gap(self.strength, rng).unwrap_or(60.0));
        let heavy = self.strength == RainStrength::Heavy;
        let end = now + Duration::from_millis(BOLT_MS as u64 + 100);
        let comp = &gfx.comp;
        let tint = windows::UI::Color { A: 255, R: 232, G: 238, B: 255 };

        // Gökyüzü aydınlanır (masaüstü), pencerelerin üstü de çok az.
        for (parent, peak) in [(back, if heavy { 0.34 } else { 0.26 }), (top, if heavy { 0.09 } else { 0.06 })] {
            let v = comp.CreateSpriteVisual()?;
            v.SetBrush(&comp.CreateColorBrushWithColor(tint)?)?;
            v.SetSize(v2(w, h))?;
            v.SetOpacity(0.0)?;
            v.StartAnimation(h!("Opacity"), &flicker(comp, peak)?)?;
            parent.Children()?.InsertAtTop(&v)?;
            self.live.push((parent.clone(), v, end));
        }

        // Şimşek çizgisi.
        let bh = (h * rng.range(0.45, 0.85)).ceil();
        let bw = (bh * 0.35).ceil().max(40.0 * s);
        let paths = bolt_paths(bw, bh, s, rng);
        let surf = gfx.surface(bw, bh, |dc| unsafe { draw_bolt(dc, &paths, s) })?;
        let brush = comp.CreateSurfaceBrushWithSurface(&surf)?;
        let v = comp.CreateSpriteVisual()?;
        v.SetBrush(&brush)?;
        v.SetSize(v2(bw, bh))?;
        v.SetOffset(v3(rng.range(0.08, 0.92) * w - bw / 2.0, 0.0))?;
        v.SetOpacity(0.0)?;
        v.StartAnimation(h!("Opacity"), &flicker(comp, 1.0)?)?;
        back.Children()?.InsertAtTop(&v)?;
        self.live.push((back.clone(), v, end));
        Ok(())
    }
}

impl Drop for Storm {
    fn drop(&mut self) {
        for (p, v, _) in &self.live {
            if let Ok(k) = p.Children() {
                let _ = k.Remove(v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damla_sayisi_yogunlukla_artar() {
        for l in [0.15, 0.6, 1.0] {
            let a = drop_count(RainStrength::Light, l, 1920.0, 1.0);
            let b = drop_count(RainStrength::Normal, l, 1920.0, 1.0);
            let c = drop_count(RainStrength::Heavy, l, 1920.0, 1.0);
            assert!(a < b && b < c, "{l}: {a} {b} {c}");
        }
        assert_eq!(drop_count(RainStrength::Heavy, 1.0, 100_000.0, 1.0), 900);
        assert_eq!(drop_count(RainStrength::Light, 0.0, 10.0, 2.0), 30);
        assert!(drop_count(RainStrength::Normal, f32::NAN, 1920.0, 1.0) >= 30);
    }

    #[test]
    fn yildirim_sikligi() {
        let mut r = Rng::new(5);
        assert!(bolt_gap(RainStrength::Light, &mut r).is_none());
        for _ in 0..50 {
            let n = bolt_gap(RainStrength::Normal, &mut r).unwrap();
            let h = bolt_gap(RainStrength::Heavy, &mut r).unwrap();
            assert!(h < n, "yoğunda daha sık: {h} {n}");
        }
        assert!(Storm::new(RainStrength::Light, Instant::now(), &mut r).is_none());
    }

    #[test]
    fn simsek_yuzeyde_kalir() {
        let mut r = Rng::new(9);
        for _ in 0..30 {
            let (bw, bh) = (300.0, 800.0);
            for p in bolt_paths(bw, bh, 1.0, &mut r) {
                assert!(p.len() >= 2);
                for (x, y) in p {
                    assert!((0.0..=bw).contains(&x) && (0.0..=bh + 0.01).contains(&y), "{x},{y}");
                }
            }
        }
    }
}
