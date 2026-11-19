//! Kar yağarken dock simgelerinin üstünde biriken küçük kar kapakları.
//! Her kapak simgenin çocuk görselidir (simgeyle birlikte büyür ve kayar);
//! büyümesi tek seferlik kompozisyon animasyonudur, karelerde işlemci harcamaz.

use super::rng::Rng;
use crate::dock::Dock;
use carlui_ui::gfx::Gfx;
use std::time::{Duration, Instant};
use windows::{
    core::*,
    Foundation::{
        Numerics::{Vector2, Vector3},
        TimeSpan,
    },
    Win32::Graphics::Direct2D::{Common::*, *},
    UI::Composition::*,
};

const SYNC: Duration = Duration::from_millis(700);
/// Kapağın dolması (ms).
const GROW_MS: f32 = 150_000.0;

fn ts(ms: f32) -> TimeSpan {
    TimeSpan { Duration: (ms.max(1.0) * 10_000.0) as i64 }
}

fn col(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// 64×20 kar kapağı: yumuşak tümsek, altta gölge, üstünde tanecikler.
unsafe fn draw_cap(dc: &ID2D1DeviceContext) -> Result<()> {
    let f = dc.GetFactory()?;
    let g = f.CreatePathGeometry()?;
    let sink = g.Open()?;
    sink.BeginFigure(pt(1.0, 19.0), D2D1_FIGURE_BEGIN_FILLED);
    // Pütürlü tepe: birkaç tümseğin birleşimi.
    let mut x = 1.0f32;
    let mut k = 0u32;
    while x <= 63.0 {
        let u = (x - 32.0) / 31.0;
        let base = 13.0 * (1.0 - u * u).max(0.0).powf(0.6);
        let bump = ((k.wrapping_mul(2654435761) >> 13) % 100) as f32 / 100.0 * 2.2;
        sink.AddLine(pt(x, 19.0 - base - bump * (base / 13.0)));
        x += 3.0;
        k += 1;
    }
    sink.AddLine(pt(63.0, 19.0));
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close()?;
    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: col(250, 252, 255, 0.98) }, D2D1_GRADIENT_STOP { position: 1.0, color: col(214, 225, 242, 0.96) }];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let fill = dc.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(0.0, 4.0), endPoint: pt(0.0, 19.0) }, None, &coll)?;
    dc.FillGeometry(&g, &fill, None);
    let dark = dc.CreateSolidColorBrush(&col(185, 200, 225, 0.6), None)?;
    let lite = dc.CreateSolidColorBrush(&col(255, 255, 255, 1.0), None)?;
    for i in 0..40u32 {
        let h = i.wrapping_mul(2246822519).rotate_left(13).wrapping_mul(3266489917);
        let x = 6.0 + (h % 52) as f32;
        let u = (x - 32.0) / 31.0;
        let top = 19.0 - 12.0 * (1.0 - u * u).max(0.0).powf(0.6);
        let y = top + 1.2 + ((h >> 8) % 100) as f32 / 100.0 * (17.5 - top).max(0.0);
        let r = 0.4 + ((h >> 16) % 50) as f32 / 100.0;
        dc.FillEllipse(&D2D1_ELLIPSE { point: pt(x, y), radiusX: r, radiusY: r }, if h >> 28 < 5 { &dark } else { &lite });
    }
    Ok(())
}

struct Cap {
    icon: SpriteVisual,
    vis: SpriteVisual,
}

impl Drop for Cap {
    fn drop(&mut self) {
        if let Ok(kids) = self.icon.Children() {
            let _ = kids.Remove(&self.vis);
        }
    }
}

pub struct IconSnow {
    brush: Option<CompositionSurfaceBrush>,
    caps: Vec<Cap>,
    checked: Option<Instant>,
    rng: Rng,
}

impl IconSnow {
    pub fn new() -> Self {
        Self { brush: None, caps: Vec::new(), checked: None, rng: Rng::from_time() }
    }

    pub fn sync(&mut self, gfx: &Gfx, dock: &Dock, now: Instant) {
        if self.checked.is_some_and(|t| now - t < SYNC) {
            return;
        }
        self.checked = Some(now);
        if self.brush.is_none() {
            let made = (|| -> Result<CompositionSurfaceBrush> {
                let surf = gfx.surface(64.0, 20.0, |dc| unsafe { draw_cap(dc) })?;
                let b = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
                b.SetStretch(CompositionStretch::Fill)?;
                Ok(b)
            })();
            match made {
                Ok(b) => self.brush = Some(b),
                Err(_) => return,
            }
        }
        let icons = dock.icon_visuals();
        self.caps.retain(|c| icons.contains(&c.icon));
        for icon in icons {
            if self.caps.iter().any(|c| c.icon == icon) {
                continue;
            }
            let delay = self.rng.range(0.0, 20_000.0);
            let wide = self.rng.range(0.78, 0.92);
            let Some(brush) = self.brush.as_ref() else { return };
            if let Ok(vis) = make(gfx, &icon, brush, delay, wide) {
                self.caps.push(Cap { icon, vis });
            }
        }
    }
}

/// Kapağı simgeye ekler: ince başlar, `GROW_MS` içinde kalınlaşır. Boyutu
/// simgeye göreli olduğundan simge büyüyünce (hover) kapak da büyür.
fn make(gfx: &Gfx, icon: &SpriteVisual, brush: &CompositionSurfaceBrush, delay: f32, wide: f32) -> Result<SpriteVisual> {
    let comp = &gfx.comp;
    let vis = comp.CreateSpriteVisual()?;
    vis.SetBrush(brush)?;
    let left = (1.0 - wide) / 2.0;
    // Alt kenar simgenin üstünün biraz altında sabit kalır, kapak yukarı doğru kalınlaşır.
    let base = 0.07;
    let size = |h: f32| Vector2 { X: wide, Y: h };
    let off = |h: f32| Vector3 { X: left, Y: base - h, Z: 0.0 };
    vis.SetRelativeSizeAdjustment(size(0.02))?;
    vis.SetRelativeOffsetAdjustment(off(0.02))?;
    let ease = comp.CreateCubicBezierEasingFunction(Vector2 { X: 0.2, Y: 0.0 }, Vector2 { X: 0.4, Y: 1.0 })?;
    let a = comp.CreateVector2KeyFrameAnimation()?;
    a.InsertKeyFrame(0.0, size(0.02))?;
    a.InsertKeyFrameWithEasingFunction(1.0, size(0.27), &ease)?;
    a.SetDuration(ts(GROW_MS))?;
    a.SetDelayTime(ts(delay))?;
    a.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
    vis.StartAnimation(h!("RelativeSizeAdjustment"), &a)?;
    let b = comp.CreateVector3KeyFrameAnimation()?;
    b.InsertKeyFrame(0.0, off(0.02))?;
    b.InsertKeyFrameWithEasingFunction(1.0, off(0.27), &ease)?;
    b.SetDuration(ts(GROW_MS))?;
    b.SetDelayTime(ts(delay))?;
    b.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
    vis.StartAnimation(h!("RelativeOffsetAdjustment"), &b)?;
    icon.Children()?.InsertAtTop(&vis)?;
    Ok(vis)
}
