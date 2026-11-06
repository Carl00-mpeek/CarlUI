//! Bu modül sesli asistanın arama panelindeki görünümünü çiziyor: öneriler
//! yerine akan ışık şeritleri, altında durum yazısı ve onay düğmeleri durur.
//! Söylenen yazı yukarıdaki arama kutusuna düşer (bkz. `spot.rs`).
//! Yöntem medya dalgasıyla aynı: şeritler D2D ile bir kez, dalga boyuyla
//! periyodik olarak çizilir; kaydırma ve sesle büyüme compositor ifadeleridir.
//! Saat ve ses seviyesi kalıcı bir özellik kümesinde durur; yazı değişip panel
//! yeniden kurulunca şeritler başa sarmaz, her kare için CPU iş yapmaz.

use super::*;
use std::f32::consts::TAU;

/// Şerit alanı ve altındaki durum satırı (DIP).
const BODY: f32 = 112.0;
const BAR: f32 = 44.0;
const BTN: f32 = 32.0;
/// Dalga boyu (DIP) ve saatin bir turu.
const WAVE: f32 = 360.0;
const CLOCK_MS: i64 = 12_000;
/// Bir şeridi oluşturan ince iplik sayısı.
const STRANDS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListenStage {
    /// Tuş basılı, mikrofon açık.
    Listening,
    /// Ses bitti, son yazı çıkarılıyor.
    Thinking,
    /// Yazı onay bekliyor (✕, Düzenle, ✓).
    Review,
    /// Kullanıcı yazıyı düzeltiyor.
    Edit,
    /// Sonuç gösterilip panel kapanacak.
    Done,
}

pub struct ListenView<'a> {
    pub stage: ListenStage,
    pub status: &'a str,
}

/// Bir şerit: renkler, dalga tohumu, saat turunda geçen dalga sayısı (eksi: sağa
/// akar), sessizdeki yükseklik ve sesle eklenen yükseklik.
struct Ribbon {
    colors: [(u8, u8, u8); 3],
    seed: f32,
    speed: i32,
    rest: f32,
    gain: f32,
}

const RIBBONS: [Ribbon; 3] = [
    Ribbon { colors: [(64, 224, 255), (40, 200, 190), (90, 150, 255)], seed: 0.4, speed: 2, rest: 0.34, gain: 0.66 },
    Ribbon { colors: [(150, 90, 255), (95, 110, 255), (200, 90, 240)], seed: 2.3, speed: -3, rest: 0.26, gain: 0.72 },
    Ribbon { colors: [(255, 90, 190), (255, 120, 150), (170, 90, 255)], seed: 4.1, speed: 4, rest: 0.2, gain: 0.8 },
];

/// Adayla birlikte bir kez kurulan saat (`Phase`) ve ses seviyesi (`Level`).
pub(super) fn clock(comp: &Compositor) -> Result<CompositionPropertySet> {
    let set = comp.CreatePropertySet()?;
    set.InsertScalar(h!("Phase"), 0.0)?;
    set.InsertScalar(h!("Level"), 0.0)?;
    let tick = comp.CreateScalarKeyFrameAnimation()?;
    let lin = comp.CreateLinearEasingFunction()?;
    tick.InsertKeyFrameWithEasingFunction(0.0, 0.0, &lin)?;
    tick.InsertKeyFrameWithEasingFunction(1.0, 1.0, &lin)?;
    tick.SetDuration(ts(CLOCK_MS))?;
    tick.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
    set.StartAnimation(h!("Phase"), &tick)?;
    Ok(set)
}

fn color(c: (u8, u8, u8), a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a }
}

/// Dalga boyuyla periyodik iki eğri; şerit bu ikisinin arasında bükülür.
fn curve(t: f32, seed: f32, twist: f32) -> f32 {
    let p = seed + twist;
    0.58 * (t + p).sin() + 0.28 * (2.0 * t + seed * 1.7 + twist * 1.9).sin() + 0.14 * (3.0 * t - seed + twist * 0.6).sin()
}

impl Island {
    /// Şeridin yüzeyi: dolgu ve üstünde ipliksi çizgiler, renk dalga boyunca döner.
    fn ribbon_strip(&self, gfx: &Gfx, r: &Ribbon, w: f32, h: f32, wl: f32, light: bool) -> Result<CompositionDrawingSurface> {
        let s = self.s;
        gfx.surface(w, h, |dc| unsafe {
            let f = dc.GetFactory()?;
            let a = if light { 0.95 } else { 0.8 };
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: color(r.colors[0], a) },
                D2D1_GRADIENT_STOP { position: 0.33, color: color(r.colors[1], a) },
                D2D1_GRADIENT_STOP { position: 0.66, color: color(r.colors[2], a) },
                D2D1_GRADIENT_STOP { position: 1.0, color: color(r.colors[0], a) },
            ];
            let rt: &ID2D1RenderTarget = dc;
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_WRAP)?;
            let brush = dc.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: D2D_POINT_2F { x: 0.0, y: 0.0 }, endPoint: D2D_POINT_2F { x: wl, y: 0.0 } },
                None,
                &coll,
            )?;
            let mid = h / 2.0;
            let amp = h * 0.44;
            let at = |x: f32, u: f32| {
                let t = x / wl * TAU;
                mid + amp * (curve(t, r.seed, 0.0) * (1.0 - u) + curve(t, r.seed, 1.1) * u)
            };
            // Gövde: iki kenar arasında yumuşak bir dolgu.
            let g = f.CreatePathGeometry()?;
            let sink = g.Open()?;
            sink.BeginFigure(D2D_POINT_2F { x: 0.0, y: at(0.0, 0.0) }, D2D1_FIGURE_BEGIN_FILLED);
            let mut x = 0.0;
            while x <= w {
                sink.AddLine(D2D_POINT_2F { x, y: at(x, 0.0) });
                x += 2.0;
            }
            while x >= 0.0 {
                sink.AddLine(D2D_POINT_2F { x, y: at(x, 1.0) });
                x -= 2.0;
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close()?;
            brush.SetOpacity(if light { 0.2 } else { 0.13 });
            dc.FillGeometry(&g, &brush, None);
            // İplikler: kenarlarda parlak, ortada soluk.
            for i in 0..STRANDS {
                let u = i as f32 / (STRANDS - 1) as f32;
                let g = f.CreatePathGeometry()?;
                let sink = g.Open()?;
                sink.BeginFigure(D2D_POINT_2F { x: 0.0, y: at(0.0, u) }, D2D1_FIGURE_BEGIN_HOLLOW);
                let mut x = 2.0;
                while x <= w {
                    sink.AddLine(D2D_POINT_2F { x, y: at(x, u) });
                    x += 2.0;
                }
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                let edge = (u - 0.5).abs() * 2.0;
                brush.SetOpacity(0.16 + 0.6 * edge * edge);
                dc.DrawGeometry(&g, &brush, (1.0 + 0.5 * edge) * s, None);
            }
            Ok(())
        })
    }

    /// Şeritlerin arasında süzülen ışık tozu; aynı konumlar her dalga boyunda yinelenir.
    fn spark_strip(&self, gfx: &Gfx, w: f32, h: f32, wl: f32, light: bool) -> Result<CompositionDrawingSurface> {
        let s = self.s;
        gfx.surface(w, h, |dc| unsafe {
            let tint = if light { (120, 110, 255) } else { (255, 255, 255) };
            let brush = dc.CreateSolidColorBrush(&color(tint, 1.0), None)?;
            let mut seed = 0x2F6Eu32;
            let mut next = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32
            };
            for _ in 0..26 {
                let (x, y) = (next() * wl, h * (0.14 + 0.72 * next()));
                let rad = (0.7 + 1.5 * next() * next()) * s;
                brush.SetOpacity(0.3 + 0.65 * next());
                let mut cx = x;
                while cx < w + wl {
                    dc.FillEllipse(&D2D1_ELLIPSE { point: D2D_POINT_2F { x: cx, y }, radiusX: rad, radiusY: rad }, &brush);
                    cx += wl;
                }
            }
            Ok(())
        })
    }

    /// Şeritleri (panel genişliği ve tema değişmedikçe aynı yüzeylerle) kurar.
    fn ribbons(&mut self, gfx: &Gfx, kids: &VisualCollection, x: f32, y: f32, w: f32, h: f32) -> Result<()> {
        let wl = (WAVE * self.s).round();
        let sw = (w + wl).ceil();
        let light = carlui_ui::tone::light();
        let key = (sw as u32, h as u32, light);
        if self.listen_strips.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut all = Vec::with_capacity(RIBBONS.len() + 1);
            for r in &RIBBONS {
                all.push(self.ribbon_strip(gfx, r, sw, h, wl, light)?);
            }
            all.push(self.spark_strip(gfx, sw, h, wl, light)?);
            self.listen_strips = Some((key, all));
        }
        let Some((_, strips)) = &self.listen_strips else { return Ok(()) };
        let comp = &self.comp;
        // Kenarlara doğru sönsün: şerit panelin ortasından doğmuş gibi dursun.
        let mask = comp.CreateLinearGradientBrush()?;
        mask.SetStartPoint(v2(0.0, 0.0))?;
        mask.SetEndPoint(v2(1.0, 0.0))?;
        let stops = mask.ColorStops()?;
        for (o, a) in [(0.0, 0u8), (0.16, 255), (0.84, 255), (1.0, 0)] {
            stops.Append(&comp.CreateColorGradientStopWithOffsetAndColor(o, carlui_ui::glass::exact(Rgba(255, 255, 255, a)))?)?;
        }
        for (i, surface) in strips.iter().enumerate() {
            let (speed, rest, gain) = RIBBONS.get(i).map_or((1, 0.75, 0.25), |r| (r.speed, r.rest, r.gain));
            let src = comp.CreateSurfaceBrushWithSurface(surface)?;
            src.SetStretch(CompositionStretch::None)?;
            src.SetHorizontalAlignmentRatio(0.0)?;
            src.SetVerticalAlignmentRatio(0.5)?;
            let brush = comp.CreateMaskBrush()?;
            brush.SetSource(&src)?;
            brush.SetMask(&mask)?;
            let sprite = comp.CreateSpriteVisual()?;
            sprite.SetBrush(&brush)?;
            sprite.SetSize(v2(w, h))?;
            sprite.SetOffset(v3(x, y))?;
            sprite.SetCenterPoint(Vector3 { X: 0.0, Y: h / 2.0, Z: 0.0 })?;
            // Şerit bir dalga boyu kayınca başa döner; ek yeri görünmez.
            let n = speed.abs();
            let text = if speed > 0 { format!("-Mod(c.Phase * {n}, 1) * {wl}") } else { format!("(Mod(c.Phase * {n}, 1) - 1) * {wl}") };
            let flow = comp.CreateExpressionAnimationWithExpression(&HSTRING::from(text))?;
            flow.SetReferenceParameter(h!("c"), &self.listen_fx)?;
            src.StartAnimation(h!("Offset.X"), &flow)?;
            // Sesle büyür, sessizken yavaşça nefes alır.
            let turn = TAU * (2 + i) as f32;
            let text = format!("Vector3(1, Min(1, {rest} + {gain} * c.Level + 0.05 * Sin(c.Phase * {turn} + {i})), 1)");
            let swell = comp.CreateExpressionAnimationWithExpression(&HSTRING::from(text))?;
            swell.SetReferenceParameter(h!("c"), &self.listen_fx)?;
            sprite.StartAnimation(h!("Scale"), &swell)?;
            kids.InsertAtTop(&sprite)?;
        }
        Ok(())
    }

    /// Yuvarlak ya da yazılı düğme.
    #[allow(clippy::too_many_arguments)]
    fn listen_btn(&mut self, gfx: &Gfx, kids: &VisualCollection, x: f32, y: f32, w: f32, glyph: Option<char>, text: &str, main: bool, hit: u8) -> Result<()> {
        let s = self.s;
        let h = (BTN * s).round();
        let brush = if main { self.accent_brush()? } else { self.solid(Rgba(255, 255, 255, 0x1C))? };
        let (bg, _) = self.rrect(w, h, h / 2.0, &brush)?;
        Self::place(&bg, x, y)?;
        kids.InsertAtTop(&bg)?;
        if let Some(c) = glyph {
            let g = carlui_ui::tone::literal_if(main, || self.text.glyph(gfx, c, (14.0 * s).round(), 0.95))?;
            kids.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, x + (w - g.width) / 2.0, y + (h - g.height) / 2.0)?)?;
        } else {
            let (t, tw, th) = self.label(gfx, text, 12.5, false, w - 12.0 * s, 0.88)?;
            Self::place(&t, x + (w - tw) / 2.0, y + (h - th) / 2.0)?;
            kids.InsertAtTop(&t)?;
        }
        self.hits.push((Hit::ListenBtn(hit), RECT { left: x as i32, top: y as i32, right: (x + w) as i32, bottom: (y + h) as i32 }));
        Ok(())
    }

    /// Arama kutusunun altındaki asistan gövdesi; bittiği y konumunu döndürür.
    pub(super) fn build_listen(&mut self, gfx: &Gfx, kids: &VisualCollection, l: &ListenView, pad: f32, fw: f32, y: f32) -> Result<f32> {
        let s = self.s;
        let bh = (BODY * s).round();
        self.ribbons(gfx, kids, pad, y, fw, bh)?;
        let y = y + bh;
        let row = (BAR * s).round();
        let ask = matches!(l.stage, ListenStage::Review | ListenStage::Edit);
        let (st, sw, sh) = self.label(gfx, l.status, 12.5, false, if ask { fw - 210.0 * s } else { fw - 16.0 * s }, 0.66)?;
        Self::place(&st, if ask { pad + 8.0 * s } else { pad + (fw - sw) / 2.0 }, y + (row - sh) / 2.0)?;
        kids.InsertAtTop(&st)?;
        if ask {
            let d = (BTN * s).round();
            let by = y + (row - d) / 2.0;
            let gap = 8.0 * s;
            let mut x = pad + fw - 4.0 * s - d;
            self.listen_btn(gfx, kids, x, by, d, Some('\u{E73E}'), "", true, 2)?;
            if l.stage == ListenStage::Review {
                let ew = (84.0 * s).round();
                x -= gap + ew;
                self.listen_btn(gfx, kids, x, by, ew, None, t!("Düzenle", "Edit"), false, 1)?;
            }
            x -= gap + d;
            self.listen_btn(gfx, kids, x, by, d, Some('\u{E711}'), "", false, 0)?;
        }
        Ok(y + row)
    }

    /// Mikrofon seviyesi (0..1): şeritler bu değere yumuşakça yaklaşır.
    pub fn listen_level(&self, v: f32) {
        let go = || -> Result<()> {
            let a = self.comp.CreateScalarKeyFrameAnimation()?;
            a.InsertKeyFrame(1.0, v.clamp(0.0, 1.0))?;
            a.SetDuration(ts(120))?;
            self.listen_fx.StartAnimation(h!("Level"), &a)
        };
        let _ = go();
    }
}
