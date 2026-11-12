//! Akvaryum sahnesi: camın içinde balıklar ve kabarcıklar. Sahnenin
//! (`scene.rs`) kökü altında yaşar; böylece saydamlık tavanı, cam tonu ve
//! kaldırma sahneyle ortaktır. Kareler yüklemede bir kez yüzeye kesilir;
//! her karede yalnızca fırça, konum ve ayna değişir.

mod sim;

use super::{
    gpu,
    load::{self, embed, Files},
    rng::Rng,
};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use sim::{Sim, BUBBLE, FISH};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    Win32::Foundation::E_FAIL,
    UI::Composition::*,
};

/// Kabarcık büyütmesi (tam sayı): 8 px sanat 16 px görünür.
const BUBBLE_ZOOM: f32 = 2.0;

const FILES: Files = embed!("scenes/aquarium": "fish_a.png", "fish_b.png", "fish_c.png", "bubble.png");

pub struct Aquarium {
    sim: Sim,
    fish: [Vec<CompositionSurfaceBrush>; 3],
    bubble: Vec<CompositionSurfaceBrush>,
    v_fish: Vec<SpriteVisual>,
    v_bubble: Vec<SpriteVisual>,
    k: f32,
    /// Son çizilen (kare, x, y, ayna) — değişmeyen görsele dokunulmaz.
    last: Vec<(u8, i32, i32, bool)>,
}

impl Aquarium {
    /// `root` sahnenin kökü; görseller onun en üstüne eklenir.
    pub fn new(gfx: &Gfx, root: &ContainerVisual, size: (f32, f32), s: f32, dense: bool) -> Result<Self> {
        let sheets = load::decode_sheets(&gfx.wic, FILES).map_err(|e| Error::new(E_FAIL, e))?;
        let get = |n: &str, fw: f32, fh: f32| -> Result<Vec<CompositionSurfaceBrush>> {
            let sh = sheets.get(n).ok_or_else(|| Error::new(E_FAIL, format!("{n} yok")))?;
            if sh.h != fh as u32 || sh.w % fw as u32 != 0 {
                return Err(Error::new(E_FAIL, format!("{n}: boyut {}x{}", sh.w, sh.h)));
            }
            gpu::frames(gfx, sh, sh.w / fw as u32)
        };
        let fish = [get("fish_a.png", FISH.0, FISH.1)?, get("fish_b.png", FISH.0, FISH.1)?, get("fish_c.png", FISH.0, FISH.1)?];
        let bubble = get("bubble.png", BUBBLE, BUBBLE)?;
        drop(sheets);
        let k = s.round().max(1.0);
        // Balık camın ~%60'ı boyunda olsun; büyütme tam sayı (keskin piksel).
        let fz = sim::zoom_for(size.1 / k);
        let sim = Sim::new(size.0 / k, size.1 / k, dense, fz, Rng::from_time());
        let (fw, fh) = sim.fish_size();
        let bz = BUBBLE * BUBBLE_ZOOM;
        let kids = root.Children()?;
        let mk =|w: f32, h: f32| -> Result<SpriteVisual> {
            let v = gfx.comp.CreateSpriteVisual()?;
            v.SetSize(v2(w * k, h * k))?;
            v.SetCenterPoint(v3(w * k / 2.0, 0.0))?;
            kids.InsertAtTop(&v)?;
            Ok(v)
        };
        // Kabarcıklar balıkların arkasında.
        let v_bubble = (0..sim.bubbles.len()).map(|_| mk(bz, bz)).collect::<Result<Vec<_>>>()?;
        let v_fish = (0..sim.fish.len()).map(|_| mk(fw, fh)).collect::<Result<Vec<_>>>()?;
        let mut a = Self { sim, fish, bubble, v_fish, v_bubble, k, last: Vec::new() };
        a.render()?;
        Ok(a)
    }

    pub fn resize(&mut self, size: (f32, f32)) {
        self.sim.resize(size.0 / self.k, size.1 / self.k);
    }

    pub fn step(&mut self, dt: f32) -> Result<()> {
        self.sim.step(dt);
        self.render()
    }

    pub fn render(&mut self) -> Result<()> {
        let k = self.k;
        let h = self.sim.h;
        let fs = self.sim.fish_size();
        let bz = BUBBLE * BUBBLE_ZOOM;
        let fish = self.sim.fish.iter().map(|f| (f.kind, f.frame, f.x, f.y, f.dir < 0.0, fs));
        let bubbles = self.sim.bubbles.iter().map(|b| (3, b.frame(h), b.x, b.y, false, (bz, bz)));
        let visuals = self.v_fish.iter().chain(&self.v_bubble);
        self.last.resize(self.v_fish.len() + self.v_bubble.len(), (u8::MAX, 0, 0, false));
        for (i, ((set, frame, x, y, mirror, size), v)) in fish.chain(bubbles).zip(visuals).enumerate() {
            let now = (frame, ((x - size.0 / 2.0) * k).round() as i32, ((y - size.1 / 2.0) * k).round() as i32, mirror);
            let last = self.last[i];
            if last == now {
                continue;
            }
            if last.0 != now.0 {
                let list = if set < 3 { &self.fish[set as usize] } else { &self.bubble };
                if let Some(b) = list.get(frame as usize) {
                    v.SetBrush(b)?;
                }
            }
            if last.3 != now.3 || last.0 == u8::MAX {
                v.SetScale(Vector3 { X: if mirror { -1.0 } else { 1.0 }, Y: 1.0, Z: 1.0 })?;
            }
            v.SetOffset(v3(now.1 as f32, now.2 as f32))?;
            self.last[i] = now;
        }
        Ok(())
    }
}
