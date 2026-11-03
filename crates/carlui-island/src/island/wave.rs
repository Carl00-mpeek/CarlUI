//! Bu modül "medya ilerleme çubuğu düz ve cansız duruyor" sorununu One UI'deki
//! gibi çözüyor: çalınan kısmın üstünde, sola doğru sönen yumuşak bir ses
//! dalgası akar; çubuğun ucunda parlayan bir tutamaç durur.
//! Yöntem: dalga D2D ile bir kez, dalga boyunun katı genişliğinde bir şerit
//! olarak çizilir. Kompozitörde fırçanın kayması sonsuz döngüyle canlandırılır,
//! görselin genişliği ifade animasyonuyla dolu kısma bağlanır ve bir maske
//! dalgayı sola doğru söndürür. Böylece her kare için CPU hiç iş yapmaz.
//! Duraklatınca dalga yatışır.

use super::*;
use std::f32::consts::TAU;

/// Dalga boyu ve yüksekliği (DIP).
const WAVE_LEN: f32 = 34.0;
const WAVE_H: f32 = 13.0;
/// Bir dalga boyunun akma süresi.
const WAVE_MS: i64 = 1500;

/// Adayla birlikte bir kez kurulan, sonsuza dek 0→1 dönen saat.
pub(super) fn clock(comp: &Compositor) -> Result<CompositionPropertySet> {
    let set = comp.CreatePropertySet()?;
    set.InsertScalar(h!("Phase"), 0.0)?;
    let tick = comp.CreateScalarKeyFrameAnimation()?;
    let lin = comp.CreateLinearEasingFunction()?;
    tick.InsertKeyFrameWithEasingFunction(0.0, 0.0, &lin)?;
    tick.InsertKeyFrameWithEasingFunction(1.0, 1.0, &lin)?;
    tick.SetDuration(ts(WAVE_MS))?;
    tick.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
    set.StartAnimation(h!("Phase"), &tick)?;
    Ok(set)
}

impl Island {
    /// İlerleme çubuğunun üstüne dalgayı ve parlayan tutamacı ekler.
    pub(super) fn wave(&self, gfx: &Gfx, kids: &VisualCollection, b: &Bar, y: f32, playing: bool) -> Result<()> {
        let s = self.s;
        let wl = (WAVE_LEN * s).round();
        let wh = (WAVE_H * s).round();
        let (c0, c1) = self.accent_colors();
        let sw = (b.w + wl * 2.0).ceil();
        let surface = gfx.surface(sw, wh, |dc| unsafe {
            let f = dc.GetFactory()?;
            // (genlik, faz, saydamlık, renk): üst üste binen üç yumuşak tepe.
            let layers = [(0.95, 0.0, 0.42, c1), (0.7, 2.1, 0.5, c0), (0.45, 4.2, 0.75, c0)];
            for (amp, ph, a, c) in layers {
                let g = f.CreatePathGeometry()?;
                let sink = g.Open()?;
                sink.BeginFigure(D2D_POINT_2F { x: 0.0, y: wh }, D2D1_FIGURE_BEGIN_FILLED);
                let mut x = 0.0;
                while x <= sw {
                    // Dalga boyuyla periyodik: şerit kaydıkça ek yeri görünmez.
                    let t = x / wl * TAU + ph;
                    let v = 0.5 + 0.35 * t.sin() + 0.15 * (2.0 * t + ph).sin();
                    sink.AddLine(D2D_POINT_2F { x, y: wh - v.clamp(0.0, 1.0) * amp * (wh - 1.0) });
                    x += 1.0;
                }
                sink.AddLine(D2D_POINT_2F { x: sw, y: wh });
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                let col = D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a };
                dc.FillGeometry(&g, &dc.CreateSolidColorBrush(&col, None)?, None);
            }
            Ok(())
        })?;

        let comp = &self.comp;
        let src = comp.CreateSurfaceBrushWithSurface(&surface)?;
        src.SetStretch(CompositionStretch::None)?;
        src.SetHorizontalAlignmentRatio(0.0)?;
        src.SetVerticalAlignmentRatio(1.0)?;
        // Sola doğru sönme: maskenin saydamlığı tutamaca yaklaştıkça artar.
        let mask = comp.CreateLinearGradientBrush()?;
        mask.SetStartPoint(v2(0.0, 0.0))?;
        mask.SetEndPoint(v2(1.0, 0.0))?;
        let stops = mask.ColorStops()?;
        for (o, a) in [(0.0, 0u8), (0.45, 70), (1.0, 255)] {
            stops.Append(&comp.CreateColorGradientStopWithOffsetAndColor(o, color(Rgba(255, 255, 255, a)))?)?;
        }
        let brush = comp.CreateMaskBrush()?;
        brush.SetSource(&src)?;
        brush.SetMask(&mask)?;

        let base = y + b.th / 2.0;
        let sprite = comp.CreateSpriteVisual()?;
        sprite.SetBrush(&brush)?;
        sprite.SetOffset(v3(b.x0, base - wh))?;
        sprite.SetCenterPoint(Vector3 { X: 0.0, Y: wh, Z: 0.0 })?;
        // Genişlik dolu kısımla birlikte akar.
        let size = comp.CreateExpressionAnimationWithExpression(&HSTRING::from(format!("Vector2(g.Size.X, {wh})")))?;
        size.SetReferenceParameter(h!("g"), &b.geom)?;
        sprite.StartAnimation(h!("Size"), &size)?;
        if playing {
            // Ortak saate bağlı: panel her yeniden kurulduğunda dalga kaldığı
            // yerden akar, başa sarıp "kesilmiş" görünmez.
            let flow = comp.CreateExpressionAnimationWithExpression(&HSTRING::from(format!("-c.Phase * {wl}")))?;
            flow.SetReferenceParameter(h!("c"), &self.wave_clock)?;
            src.StartAnimation(h!("Offset.X"), &flow)?;
        } else {
            sprite.SetScale(Vector3 { X: 1.0, Y: 0.3, Z: 1.0 })?;
        }
        // Dalga çubuğun altında kalsın: dolu çizgi onun üstünde parlak dursun.
        kids.InsertAtBottom(&sprite)?;

        // Parlayan tutamaç: yumuşak hale + beyaz çekirdek, dolu kısmın ucunda.
        let (r, gr) = ((5.5 * s).round(), (11.0 * s).round());
        let knob = comp.CreateContainerVisual()?;
        let (glow, _) = self.rrect(gr * 2.0, gr * 2.0, gr, &self.solid(Rgba(c0.0, c0.1, c0.2, 0x55))?)?;
        knob.Children()?.InsertAtTop(&glow)?;
        let (ring, _) = self.rrect(r * 2.0 + 4.0 * s, r * 2.0 + 4.0 * s, r + 2.0 * s, &self.solid(Rgba(c1.0, c1.1, c1.2, 0xFF))?)?;
        ring.SetOffset(v3(gr - r - 2.0 * s, gr - r - 2.0 * s))?;
        knob.Children()?.InsertAtTop(&ring)?;
        let (core, _) = self.rrect(r * 2.0, r * 2.0, r, &self.solid(Rgba(255, 255, 255, 0xF5))?)?;
        core.SetOffset(v3(gr - r, gr - r))?;
        knob.Children()?.InsertAtTop(&core)?;
        let at = comp.CreateExpressionAnimationWithExpression(&HSTRING::from(format!("Vector3({} + g.Size.X - {gr}, {}, 0)", b.x0, base - gr)))?;
        at.SetReferenceParameter(h!("g"), &b.geom)?;
        knob.StartAnimation(h!("Offset"), &at)?;
        if playing {
            // Hale yavaşça nefes alır (aynı saatle: yeniden kurulumda sıçramaz).
            let pulse = comp.CreateExpressionAnimationWithExpression(h!("0.775 - 0.225 * Cos(c.Phase * 6.2831853)"))?;
            pulse.SetReferenceParameter(h!("c"), &self.wave_clock)?;
            glow.StartAnimation(h!("Opacity"), &pulse)?;
        } else {
            glow.SetOpacity(0.5)?;
        }
        kids.InsertAtTop(&knob)?;
        Ok(())
    }
}
