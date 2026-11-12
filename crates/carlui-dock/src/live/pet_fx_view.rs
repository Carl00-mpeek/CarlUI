//! Kalp ve nota parçacıklarının çizimi: tünek penceresinde sabit, küçük bir
//! görsel havuzu (`pet_fx::MAX`). Fırçalar bir kez, kodla üretilen piksel
//! sanatından kurulur; her karede yalnızca konum ve saydamlık değişir.

use super::{
    gpu,
    pet_fx::{self, Fx, Puff},
};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use windows::{core::*, UI::Composition::*};

struct Art {
    brush: CompositionSurfaceBrush,
    w: f32,
    h: f32,
}

fn art(gfx: &Gfx, kind: Puff) -> Result<Art> {
    let (w, h, px) = pet_fx::art(kind);
    let surf = gpu::crop_px(gfx, &px, w, h, 0, 0, w, h)?;
    Ok(Art { brush: gpu::brush(gfx, &surf, true)?, w: w as f32, h: h as f32 })
}

pub struct FxView {
    pool: Vec<(SpriteVisual, Option<Puff>)>,
    heart: Art,
    note: Art,
    /// Hayvanın büyütmesi (parçacıklar aynı piksel boyunda görünsün).
    k: f32,
}

impl FxView {
    pub fn new(gfx: &Gfx, root: &ContainerVisual, k: f32) -> Result<Self> {
        let mut pool = Vec::with_capacity(pet_fx::MAX);
        for _ in 0..pet_fx::MAX {
            let v = gfx.comp.CreateSpriteVisual()?;
            v.SetOpacity(0.0)?;
            root.Children()?.InsertAtTop(&v)?;
            pool.push((v, None));
        }
        Ok(Self { pool, heart: art(gfx, Puff::Heart)?, note: art(gfx, Puff::Note)?, k })
    }

    /// Parçacıkları çizer. `origin`: hayvanın başının pencere içi konumu
    /// (yatay orta, opak kısmın üst kenarı).
    pub fn render(&mut self, fx: &Fx, origin: (f32, f32)) -> Result<()> {
        let k = self.k;
        for (i, (v, shown)) in self.pool.iter_mut().enumerate() {
            let Some(p) = fx.list.get(i).filter(|p| p.age > 0.0) else {
                if shown.take().is_some() {
                    v.SetOpacity(0.0)?;
                }
                continue;
            };
            let a = if p.kind == Puff::Heart { &self.heart } else { &self.note };
            if *shown != Some(p.kind) {
                v.SetBrush(&a.brush)?;
                v.SetSize(v2(a.w * k, a.h * k))?;
                *shown = Some(p.kind);
            }
            // Sanat pikseli ızgarasına oturt (piksel sanatı bulanmasın).
            let x = origin.0 + (p.x.round() - (a.w / 2.0).floor()) * k;
            let y = origin.1 - (p.y.round() + a.h + 1.0) * k;
            v.SetOffset(v3(x.round(), y.round()))?;
            v.SetOpacity(p.alpha())?;
        }
        Ok(())
    }
}
