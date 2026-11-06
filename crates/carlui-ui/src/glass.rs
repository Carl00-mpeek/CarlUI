//! Bu modül "OneUI + Liquid Glass" yüzeyini tek yerde tanımlar: sistem
//! bulanıklığı, koyu füme tint, üstten sönen parlaklık ve ince kenar yansıması.
//! Yüzeyin boyutu tek bir `Size` özelliğinden okunur; clip ve kenar ona
//! expression ile bağlı olduğu için yüzey spring ile büyürken her şey birlikte akar.

use carlui_core::theme::{Rgba, Theme};
use windows::{
    core::*,
    Foundation::Numerics::{Vector2, Vector3},
    UI::{Color, Composition::*},
};

/// Koyu cama göre yazılmış renk: açık temada rolüne göre çevrilir.
pub fn color(c: Rgba) -> Color {
    let hex = (c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32;
    let (v, a) = crate::tone::fill(hex, c.3 as f32 / 255.0);
    Color { A: (a * 255.0).round().min(255.0) as u8, R: (v >> 16) as u8, G: (v >> 8) as u8, B: v as u8 }
}

/// Gerçek renk: `Theme` değerleri (zaten temaya göre), gölge ve kapak rengi.
pub fn exact(c: Rgba) -> Color {
    Color { A: c.3, R: c.0, G: c.1, B: c.2 }
}

pub fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

pub fn v3(x: f32, y: f32) -> Vector3 {
    Vector3 { X: x, Y: y, Z: 0.0 }
}

pub struct Glass {
    /// Yüzeyin kendisi; Size/Offset/AnchorPoint buradan ayarlanır.
    pub root: ContainerVisual,
    /// İçerik (ikonlar vb.) buraya eklenir; cam katmanlarının üstündedir.
    pub content: ContainerVisual,
    tint_brush: CompositionColorBrush,
    /// Ruh hali katmanı (ör. çalan şarkının kapak rengi); normalde saydam.
    mood_brush: CompositionLinearGradientBrush,
    clip: CompositionRoundedRectangleGeometry,
    rim: CompositionRoundedRectangleGeometry,
}

fn gradient(comp: &Compositor, top: Rgba, bottom: Rgba, end: f32) -> Result<CompositionLinearGradientBrush> {
    let b = comp.CreateLinearGradientBrush()?;
    b.SetStartPoint(v2(0.0, 0.0))?;
    b.SetEndPoint(v2(0.0, 1.0))?;
    let stops = b.ColorStops()?;
    stops.Append(&comp.CreateColorGradientStopWithOffsetAndColor(0.0, exact(top))?)?;
    stops.Append(&comp.CreateColorGradientStopWithOffsetAndColor(end, exact(bottom))?)?;
    Ok(b)
}

impl Glass {
    pub fn new(comp: &Compositor, theme: &Theme, radius: f32) -> Result<Self> {
        let root = comp.CreateContainerVisual()?;

        let clip_geom = comp.CreateRoundedRectangleGeometry()?;
        clip_geom.SetCornerRadius(v2(radius, radius))?;
        let follow = comp.CreateExpressionAnimationWithExpression(h!("r.Size"))?;
        follow.SetReferenceParameter(h!("r"), &root)?;
        clip_geom.StartAnimation(h!("Size"), &follow)?;
        root.SetClip(&comp.CreateGeometricClipWithGeometry(&clip_geom)?)?;

        let kids = root.Children()?;
        let layer = |brush: &CompositionBrush| -> Result<()> {
            let v = comp.CreateSpriteVisual()?;
            v.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            v.SetBrush(brush)?;
            kids.InsertAtTop(&v)
        };

        // 1) Sistem arka plan bulanıklığı
        layer(&comp.CreateHostBackdropBrush()?.cast()?)?;
        // 2) Füme (açık temada süt beyazı) tint
        let tint_brush = comp.CreateColorBrushWithColor(exact(theme.glass_tint))?;
        layer(&tint_brush.cast()?)?;
        // 2b) Ruh hali: soldan sağa sönen renk (başta saydam)
        let mood_brush = comp.CreateLinearGradientBrush()?;
        mood_brush.SetStartPoint(v2(0.0, 0.0))?;
        mood_brush.SetEndPoint(v2(1.0, 1.0))?;
        let ms = mood_brush.ColorStops()?;
        ms.Append(&comp.CreateColorGradientStopWithOffsetAndColor(0.0, exact(Rgba(0, 0, 0, 0)))?)?;
        ms.Append(&comp.CreateColorGradientStopWithOffsetAndColor(1.0, exact(Rgba(0, 0, 0, 0)))?)?;
        layer(&mood_brush.cast()?)?;
        // 3) Üstten sönen hafif parlaklık
        let clear = Rgba(theme.gloss.0, theme.gloss.1, theme.gloss.2, 0);
        layer(&gradient(comp, theme.gloss, clear, 0.55)?.cast()?)?;

        // 4) İnce kenar yansıması
        let rim_geom = comp.CreateRoundedRectangleGeometry()?;
        rim_geom.SetCornerRadius(v2(radius - 0.5, radius - 0.5))?;
        rim_geom.SetOffset(v2(0.5, 0.5))?;
        let rim_follow = comp.CreateExpressionAnimationWithExpression(h!("r.Size - Vector2(1, 1)"))?;
        rim_follow.SetReferenceParameter(h!("r"), &root)?;
        rim_geom.StartAnimation(h!("Size"), &rim_follow)?;
        let rim_shape = comp.CreateSpriteShapeWithGeometry(&rim_geom)?;
        rim_shape.SetStrokeBrush(&gradient(comp, theme.rim_top, theme.rim_bottom, 1.0)?)?;
        rim_shape.SetStrokeThickness(1.0)?;
        let rim = comp.CreateShapeVisual()?;
        rim.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        rim.Shapes()?.Append(&rim_shape)?;
        kids.InsertAtTop(&rim)?;

        let content = comp.CreateContainerVisual()?;
        content.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        kids.InsertAtTop(&content)?;

        Ok(Self { root, content, tint_brush, mood_brush, clip: clip_geom, rim: rim_geom })
    }

    /// Ruh hali rengini yumuşakça değiştirir; `None` saydama döner.
    /// Renk sol üstte daha yoğun, sağ altta daha hafif durur.
    pub fn set_mood(&self, comp: &Compositor, c: Option<Rgba>, ms: i64) -> Result<()> {
        let (a, b) = match c {
            Some(Rgba(r, g, bl, al)) => (Rgba(r, g, bl, al), Rgba(r, g, bl, (al as f32 * 0.45) as u8)),
            None => (Rgba(0, 0, 0, 0), Rgba(0, 0, 0, 0)),
        };
        let stops = self.mood_brush.ColorStops()?;
        for (i, to) in [(0, a), (1, b)] {
            let stop = stops.GetAt(i)?;
            let anim = comp.CreateColorKeyFrameAnimation()?;
            anim.InsertKeyFrame(1.0, exact(to))?;
            anim.SetDuration(crate::motion::ts(ms))?;
            stop.StartAnimation(h!("Color"), &anim)?;
        }
        Ok(())
    }

    /// Köşe yarıçapını canlandırır (ör. daire balondan panele dönüşüm).
    pub fn animate_radius(&self, comp: &Compositor, to: f32, ms: i64, delay_ms: i64) -> Result<()> {
        for (geom, r) in [(&self.clip, to), (&self.rim, (to - 0.5).max(0.0))] {
            let a = comp.CreateVector2KeyFrameAnimation()?;
            a.InsertKeyFrame(1.0, v2(r, r))?;
            a.SetDuration(crate::motion::ts(ms))?;
            if delay_ms > 0 {
                a.SetDelayTime(crate::motion::ts(delay_ms))?;
            }
            geom.StartAnimation(h!("CornerRadius"), &a)?;
        }
        Ok(())
    }

    pub fn set_radius(&self, r: f32) -> Result<()> {
        self.clip.SetCornerRadius(v2(r, r))?;
        self.rim.SetCornerRadius(v2((r - 0.5).max(0.0), (r - 0.5).max(0.0)))
    }

    /// Tema değişince (ör. cam yoğunluğu) yeniden oluşturmadan güncelle.
    pub fn apply_theme(&self, theme: &Theme) -> Result<()> {
        self.tint_brush.SetColor(exact(theme.glass_tint))
    }
}
