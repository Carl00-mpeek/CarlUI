//! Şerit hayvanın (kedi, köpek, kertenkele, örümcek, kullanıcı paketi) tüneği.
//! Pencere bir kare yüksekliğindedir ve alt kenarı hayvanın taban çizgisidir
//! (bkz. `window.rs`). Her kare, paket yüklenirken bir kez oluşturulan
//! yüzeylerden birine fırça değiştirmektir.

use super::{
    gpu,
    load::Decoded,
    pack::{self, Anim, Facing, Filter, ANIMS, N_ANIMS},
    pet::{self, Pet},
    rng::Rng,
    window::PerchWindow,
};
use crate::dock::LiveView;
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    Win32::Foundation::RECT,
    UI::Composition::*,
};

pub struct Perch {
    win: PerchWindow,
    sprite: SpriteVisual,
    frames: [Vec<CompositionSurfaceBrush>; N_ANIMS],
    pub pet: Pet,
    /// Ekrandaki büyütme (tam sayı, en yakın komşuda).
    k: f32,
    fw: f32,
    facing: Facing,
    last: Option<(Anim, u32, bool, i32)>,
}

impl Perch {
    /// Yalnızca altta duran ana dock'ta çağrılır.
    pub fn new(gfx: &Gfx, view: &LiveView, d: Decoded) -> Result<Self> {
        let m = &d.manifest;
        let k = pack::display_scale(m, view.scale);
        let (fw, fh) = (m.fw as f32 * k, m.fh as f32 * k);
        let nearest = m.filter == Filter::Nearest;
        let mut frames: [Vec<CompositionSurfaceBrush>; N_ANIMS] = Default::default();
        for s in &d.strips {
            let n = m.spec(s.anim).frames;
            for i in 0..n {
                // Şerit yüksekliği yükleyicide kareyle eşit denetlendi; `crop_px` yine de taşmaz.
                let surf = gpu::crop_px(gfx, &s.px, s.w, s.h, i * m.fw, 0, m.fw, m.fh)?;
                frames[s.anim as usize].push(gpu::brush(gfx, &surf, nearest)?);
            }
        }
        // Şeridi olmayan durum oturma karelerini paylaşır.
        let sit = frames[Anim::Sit as usize].clone();
        for a in ANIMS {
            if frames[a as usize].is_empty() {
                frames[a as usize] = sit.clone();
            }
        }
        let strips = ANIMS.map(|a| {
            let spec = m.spec(a);
            pet::Strip { frames: frames[a as usize].len().max(1) as u32, fps: spec.rate(), own: m.states[a as usize].is_some(), reverse_out: spec.reverse_out }
        });

        let rise = (m.baseline as f32 * k).round() as i32;
        let win = PerchWindow::new(gfx, view, rise, fh.ceil() as i32)?;
        let sprite = gfx.comp.CreateSpriteVisual()?;
        sprite.SetSize(v2(fw, fh))?;
        sprite.SetCenterPoint(v3(fw / 2.0, 0.0))?;
        win.root.Children()?.InsertAtTop(&sprite)?;

        let x = ((view.pill.left + view.pill.right) / 2 - win.left) as f32;
        let pet = Pet::new(strips, m.walk_speed * k, x, Rng::from_time());
        let mut p = Self { win, sprite, frames, pet, k, fw, facing: m.facing, last: None };
        p.render()?;
        Ok(p)
    }

    /// Yürünebilir aralık (pencere içi x): pill'in üst kenarı, köşelerden pay bırakarak.
    pub fn range(&self, pill: RECT, s: f32) -> (f32, f32) {
        let pad = self.fw / 2.0 + 12.0 * s;
        let left = self.win.left;
        let (lo, hi) = ((pill.left - left) as f32 + pad, (pill.right - left) as f32 - pad);
        if hi < lo {
            let c = (lo + hi) / 2.0;
            (c, c)
        } else {
            (lo, hi)
        }
    }

    /// İmlecin pencere içi x'i.
    pub fn local_x(&self, screen_x: i32) -> f32 {
        self.win.local_x(screen_x)
    }

    pub fn show(&mut self, on: bool) {
        self.win.show(on);
    }

    /// Dock'un üst kenarı kaydıysa tüneği taşır.
    pub fn follow(&mut self, pill: RECT) {
        self.win.follow(pill);
    }

    /// Hayvanın şimdiki karesini ve konumunu görsele aktarır; değişmediyse dokunmaz.
    pub fn render(&mut self) -> Result<()> {
        let anim = self.pet.anim();
        let list = &self.frames[anim as usize];
        let frame = self.pet.frame.min(list.len().saturating_sub(1) as u32);
        let mirror = (self.facing == Facing::Right) != self.pet.right;
        let x = (self.pet.x - self.fw / 2.0).round() as i32;
        let now = (anim, frame, mirror, x);
        if self.last == Some(now) {
            return Ok(());
        }
        if self.last.map(|l| (l.0, l.1)) != Some((anim, frame)) {
            if let Some(b) = list.get(frame as usize) {
                self.sprite.SetBrush(b)?;
            }
        }
        if self.last.map(|l| l.2) != Some(mirror) {
            self.sprite.SetScale(Vector3 { X: if mirror { -1.0 } else { 1.0 }, Y: 1.0, Z: 1.0 })?;
        }
        self.sprite.SetOffset(v3(x as f32, 0.0))?;
        self.last = Some(now);
        Ok(())
    }

    pub fn scale(&self) -> f32 {
        self.k
    }
}
