//! Şerit hayvanın tepki parçaları bir arada: karar (`react.rs`), parçacıklar
//! (`pet_fx*.rs`), tıklama alanı (`pet_hit.rs`) ve boşta süresi. Ayar
//! kapalıyken hiç kurulmaz (pencere, görsel, bellek yok).

use super::{
    load::Decoded,
    pack::{Anim, ANIMS, N_ANIMS},
    pet::Pet,
    pet_fx::{self, Fx, Puff},
    pet_fx_view::FxView,
    pet_hit::{self, BoxPx, HitWindow},
    react::{self, React},
    rng::Rng,
};
use carlui_core::warn;
use carlui_ui::gfx::Gfx;
use windows::{
    core::Result,
    Win32::{
        Foundation::{HWND, RECT},
        System::SystemInformation::GetTickCount,
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    },
    UI::Composition::ContainerVisual,
};

/// Parçacıklar için tünek penceresinin hayvanın üstünde bıraktığı pay (sanat pikseli).
pub const HEAD_PX: f32 = 26.0;

/// Son girdiden beri geçen süre (ms).
fn idle_ms() -> Option<u32> {
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe { GetLastInputInfo(&mut li).as_bool().then(|| GetTickCount().wrapping_sub(li.dwTime)) }
}

/// Dans/tepki kutularını paketten çıkarır (şeridi olmayan durum oturuşunkini kullanır).
pub fn boxes(d: &Decoded) -> [Option<BoxPx>; N_ANIMS] {
    let m = &d.manifest;
    let mut out = [None; N_ANIMS];
    for s in &d.strips {
        out[s.anim as usize] = pet_hit::opaque_box(&s.px, s.w, s.h, m.spec(s.anim).frames, m.fw, m.fh);
    }
    let sit = out[Anim::Sit as usize];
    for a in ANIMS {
        if out[a as usize].is_none() {
            out[a as usize] = sit;
        }
    }
    out
}

pub struct Reactions {
    react: React,
    fx: Fx,
    view: FxView,
    hit: Option<HitWindow>,
    boxes: [Option<BoxPx>; N_ANIMS],
    /// Görünür ve çalışıyor: tıklama alanı yalnızca o zaman durur.
    live: bool,
    /// Kum havuzu denemesi: kullanıcı uzaktaymış gibi davran.
    fake_idle: bool,
}

impl Reactions {
    pub fn new(gfx: &Gfx, root: &ContainerVisual, dock: HWND, k: f32, boxes: [Option<BoxPx>; N_ANIMS]) -> Result<Self> {
        let hit = HitWindow::new(dock).map_err(|e| warn!("canlı dock: okşama alanı kurulamadı: {e}")).ok();
        Ok(Self { react: React::new(Rng::from_time()), fx: Fx::new(Rng::from_time()), view: FxView::new(gfx, root, k)?, hit, boxes, live: false, fake_idle: false })
    }

    /// Kum havuzu denemesi: boşta süresini eşiğin üstünde say (ya da bırak).
    pub fn fake_idle(&mut self, on: bool) {
        self.fake_idle = on;
    }

    pub fn music(&mut self, on: bool) {
        self.react.music(on);
    }

    pub fn note(&mut self, pet: &mut Pet) {
        self.react.note(pet);
    }

    pub fn click(&mut self, pet: &mut Pet) {
        let n = self.react.click(pet);
        self.fx.spawn(Puff::Heart, n);
    }

    /// `dt` saniye ilerletir. Dönüş: tike kadar en çok kaç ms beklenmeli
    /// ve parçacıklar yeniden çizilmeli mi.
    pub fn step(&mut self, pet: &mut Pet, dt: f32) -> (Option<f32>, bool) {
        let fake = self.fake_idle;
        let out = self.react.step(pet, dt * 1000.0, || if fake { Some(react::IDLE_SLEEP_MS) } else { idle_ms() });
        if out.notes > 0 {
            self.fx.spawn(Puff::Note, out.notes);
        }
        let had = self.fx.alive();
        self.fx.step(dt);
        let cap = if self.fx.alive() { Some(out.cap_ms.map_or(pet_fx::STEP_MS, |c| c.min(pet_fx::STEP_MS))) } else { out.cap_ms };
        (cap, had || self.fx.alive())
    }

    /// Opak kutunun üst kenarı (sanat pikseli) — parçacıklar buradan çıkar.
    pub fn head_top(&self, anim: Anim) -> f32 {
        self.boxes[anim as usize].map_or(0.0, |b| b.t as f32)
    }

    pub fn draw_fx(&mut self, origin: (f32, f32)) {
        let _ = self.view.render(&self.fx, origin);
    }

    /// Görünürlük/çalışma durumu değişti.
    pub fn set_live(&mut self, on: bool) {
        self.live = on;
        if !on {
            if let Some(h) = self.hit.as_mut() {
                h.place(None);
            }
        }
    }

    /// Tıklama alanını hayvanın üstüne taşır (yalnızca çizilen karelerde çağrılır).
    #[allow(clippy::too_many_arguments)]
    pub fn place_hit(&mut self, anim: Anim, win: (i32, i32), sprite: (i32, i32), k: f32, fw: u32, mirror: bool, pill_top: i32) {
        let r: Option<RECT> = if self.live { self.boxes[anim as usize].and_then(|b| pet_hit::screen_rect(win, sprite.0, sprite.1, k, fw, b, mirror, pill_top)) } else { None };
        if let Some(h) = self.hit.as_mut() {
            h.place(r);
        }
    }
}
