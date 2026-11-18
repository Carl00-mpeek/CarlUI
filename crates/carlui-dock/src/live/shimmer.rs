//! Yağmurda dock simgelerinin üstünden arada çapraz kayan ıslak parlaklık.
//!
//! Her simgenin içine, simgenin kendi fırçasıyla maskelenmiş bir parıltı
//! görseli eklenir; parıltı yalnızca simgenin dolu piksellerinde görünür.
//! Kayma kompozisyon animasyonudur (işlemci harcamaz); simgeler sırayla,
//! araya gecikme girerek parlar. Simge listesi seyrek (`SYNC`) eşlenir.

use super::rng::Rng;
use crate::dock::Dock;
use carlui_ui::{gfx::Gfx, glass::v2};
use std::time::{Duration, Instant};
use windows::{
    core::*,
    Foundation::{Numerics::Vector2, TimeSpan},
    UI::{Color, Composition::*},
};

/// Simge listesinin yeniden okunma aralığı.
const SYNC: Duration = Duration::from_millis(500);
/// Bir parıltı döngüsü (ms): başta kayar, kalanında bekler.
const PERIOD: f32 = 5200.0;
/// Döngünün kayma payı.
const SWEEP: f32 = 0.24;

fn ts(ms: f32) -> TimeSpan {
    TimeSpan { Duration: (ms.max(1.0) * 10_000.0) as i64 }
}

struct Gloss {
    icon: SpriteVisual,
    vis: SpriteVisual,
    mask: CompositionMaskBrush,
}

pub struct Shimmer {
    items: Vec<Gloss>,
    checked: Option<Instant>,
    level: f32,
    rng: Rng,
}

impl Shimmer {
    pub fn new(level: f32) -> Self {
        Self { items: Vec::new(), checked: None, level, rng: Rng::from_time() }
    }

    /// Simgelerle eşler: yeni simgeye parıltı ekler, simge fırçası değiştiyse
    /// maskeyi günceller, giden simgelerin parıltısını kaldırır.
    pub fn sync(&mut self, gfx: &Gfx, dock: &Dock, now: Instant, level: f32) {
        if self.checked.is_some_and(|t| now - t < SYNC) && (level - self.level).abs() < 0.2 {
            return;
        }
        self.checked = Some(now);
        if (level - self.level).abs() >= 0.2 {
            // Şiddet değişti: parlaklık yeniden kurulsun.
            self.items.clear();
            self.level = level;
        }
        let icons = dock.icon_visuals();
        self.items.retain(|g| icons.contains(&g.icon));
        for (i, icon) in icons.into_iter().enumerate() {
            match self.items.iter().position(|g| g.icon == icon) {
                Some(k) => {
                    let g = &self.items[k];
                    let _ = (|| -> Result<()> {
                        let b = icon.Brush()?;
                        if g.mask.Mask().ok().as_ref() != Some(&b) {
                            g.mask.SetMask(&b)?;
                        }
                        let size = icon.Size()?;
                        if g.vis.Size()? != size {
                            g.vis.SetSize(size)?;
                        }
                        Ok(())
                    })();
                }
                None => {
                    let delay = i as f32 * 420.0 + self.rng.range(0.0, 1500.0);
                    match make(gfx, &icon, self.level, delay) {
                        Ok((vis, mask)) => self.items.push(Gloss { icon, vis, mask }),
                        Err(_) => continue,
                    }
                }
            }
        }
    }
}

impl Drop for Shimmer {
    fn drop(&mut self) {
        self.items.clear();
    }
}

impl Drop for Gloss {
    fn drop(&mut self) {
        if let Ok(kids) = self.icon.Children() {
            let _ = kids.Remove(&self.vis);
        }
    }
}

/// Bir simgeye parıltı görseli ekler.
fn make(gfx: &Gfx, icon: &SpriteVisual, level: f32, delay: f32) -> Result<(SpriteVisual, CompositionMaskBrush)> {
    let comp = &gfx.comp;
    let band = comp.CreateLinearGradientBrush()?;
    let stops = band.ColorStops()?;
    let white = |a: f32| Color { A: (a * 255.0) as u8, R: 255, G: 255, B: 255 };
    for (p, a) in [(0.0, 0.0), (0.38, 0.0), (0.5, 0.62), (0.62, 0.0), (1.0, 0.0)] {
        stops.Append(&comp.CreateColorGradientStopWithOffsetAndColor(p, white(a))?)?;
    }
    band.SetExtendMode(CompositionGradientExtendMode::Clamp)?;
    // Bant (u, u) noktasından geçer; u simgenin dışından dışına kayar.
    let at = |u: f32| (Vector2 { X: u - 0.5, Y: u - 0.5 }, Vector2 { X: u + 0.5, Y: u + 0.5 });
    let (s0, e0) = at(-0.7);
    let (s1, e1) = at(1.7);
    band.SetStartPoint(s0)?;
    band.SetEndPoint(e0)?;
    let ease = comp.CreateCubicBezierEasingFunction(Vector2 { X: 0.4, Y: 0.0 }, Vector2 { X: 0.6, Y: 1.0 })?;
    for (prop, a, b) in [(h!("StartPoint"), s0, s1), (h!("EndPoint"), e0, e1)] {
        let anim = comp.CreateVector2KeyFrameAnimation()?;
        anim.InsertKeyFrame(0.0, a)?;
        anim.InsertKeyFrameWithEasingFunction(SWEEP, b, &ease)?;
        anim.InsertKeyFrame(1.0, b)?;
        anim.SetDuration(ts(PERIOD))?;
        anim.SetDelayTime(ts(delay))?;
        anim.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        band.StartAnimation(prop, &anim)?;
    }
    let mask = comp.CreateMaskBrush()?;
    mask.SetMask(&icon.Brush()?)?;
    mask.SetSource(&band)?;
    let vis = comp.CreateSpriteVisual()?;
    vis.SetBrush(&mask)?;
    let size = icon.Size()?;
    vis.SetSize(if size.X > 0.0 { size } else { v2(1.0, 1.0) })?;
    vis.SetOpacity(0.55 + 0.45 * level.clamp(0.0, 1.0))?;
    icon.Children()?.InsertAtTop(&vis)?;
    Ok((vis, mask))
}
