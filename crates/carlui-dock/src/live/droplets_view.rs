//! Cam damlalarının çizimi: `droplets.rs` simülasyonunu SpriteVisual'lara
//! aktarır. Damla ve iz bitmap'i birer kez çizilir, fırça paylaşılır; karede
//! yalnızca değişen konum/boyut/saydamlık gönderilir (bkz. `scene.rs`).

use super::{
    droplets::{Droplets, MAX_TRAILS},
    particles::Sprite,
    rng::Rng,
    scene_art::{draw, sprite_px, visual_size, Palette},
};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use windows::{core::*, UI::Composition::*};

/// İz çizgisinin yarıçapı (ölçek 1'de piksel).
const TRAIL_R: f32 = 0.6;

pub struct DropletsView {
    sim: Droplets,
    s: f32,
    beads: Vec<SpriteVisual>,
    trails: Vec<SpriteVisual>,
    last_b: Vec<(f32, f32, f32)>,
    last_t: Vec<(f32, f32, f32)>,
}

fn brush(gfx: &Gfx, sp: Sprite, pal: &Palette) -> Result<CompositionSurfaceBrush> {
    let (w, h) = sprite_px(sp);
    let surf = gfx.surface(w, h, |dc| unsafe { draw(dc, sp, pal) })?;
    let b = gfx.comp.CreateSurfaceBrushWithSurface(&surf)?;
    b.SetStretch(CompositionStretch::Fill)?;
    Ok(b)
}

impl DropletsView {
    /// `level`: hava şiddeti yüzdesi (0..=100).
    pub fn new(gfx: &Gfx, root: &ContainerVisual, size: (f32, f32), s: f32, level: u8, pal: &Palette) -> Result<Self> {
        let sim = Droplets::new(size.0, size.1, s, level as f32 / 100.0, Rng::from_time());
        let (bead_brush, trail_brush) = (brush(gfx, Sprite::Drop, pal)?, brush(gfx, Sprite::Trail, pal)?);
        let make = |b: &CompositionSurfaceBrush| -> Result<SpriteVisual> {
            let v = gfx.comp.CreateSpriteVisual()?;
            v.SetBrush(b)?;
            v.SetOpacity(0.0)?;
            root.Children()?.InsertAtTop(&v)?;
            Ok(v)
        };
        // Önce izler (altta), sonra damlalar.
        let trails = (0..MAX_TRAILS).map(|_| make(&trail_brush)).collect::<Result<Vec<_>>>()?;
        let beads = (0..sim.beads().len()).map(|_| make(&bead_brush)).collect::<Result<Vec<_>>>()?;
        let nan = (f32::NAN, f32::NAN, f32::NAN);
        let mut v = Self { s: s.max(0.5), last_b: vec![nan; beads.len()], last_t: vec![nan; trails.len()], sim, beads, trails };
        v.render()?;
        Ok(v)
    }

    pub fn resize(&mut self, size: (f32, f32)) {
        self.sim.resize(size.0, size.1);
    }

    pub fn step(&mut self, dt: f32) {
        self.sim.step(dt);
    }

    pub fn render(&mut self) -> Result<()> {
        for (i, (b, v)) in self.sim.beads().iter().zip(&self.beads).enumerate() {
            let (w, h) = visual_size(Sprite::Drop, b.r, b.len);
            let (lw, lh, la) = &mut self.last_b[i];
            if (*lw, *lh) != (w, h) {
                v.SetSize(v2(w, h))?;
                (*lw, *lh) = (w, h);
            }
            if *la != b.a {
                v.SetOpacity(b.a)?;
                *la = b.a;
            }
            v.SetOffset(v3(b.x - w / 2.0, b.y - h / 2.0))?;
        }
        let r = TRAIL_R * self.s;
        for (i, (t, v)) in self.sim.trails().iter().zip(&self.trails).enumerate() {
            let span = t.y1 - t.y0;
            let a = if span > 0.5 { t.a } else { 0.0 };
            let (lw, lh, la) = &mut self.last_t[i];
            if *la != a {
                v.SetOpacity(a)?;
                *la = a;
            }
            if a <= 0.0 {
                continue;
            }
            let (w, h) = visual_size(Sprite::Trail, r, (span / (2.0 * r)).max(1.0));
            if (*lw, *lh) != (w, h) {
                v.SetSize(v2(w, h))?;
                (*lw, *lh) = (w, h);
            }
            v.SetOffset(v3(t.x - w / 2.0, (t.y0 + t.y1) / 2.0 - h / 2.0))?;
        }
        Ok(())
    }
}
