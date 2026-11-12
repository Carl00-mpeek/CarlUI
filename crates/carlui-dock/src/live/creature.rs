//! Dock'taki hayvan: şerit paketi (kedi, köpek, kertenkele, örümcek, kullanıcı
//! paketi), yılan ya da yunus. `Live` hepsini aynı arayüzle sürer: tek
//! zamanlayıcı, her tikte `step` ve bir sonraki tike kadar beklenecek süre.

use super::{dolphin::Dolphin, load::Loaded, perch::Perch, pet, snake::Snake};
use crate::dock::LiveView;
use carlui_ui::gfx::Gfx;
use windows::{
    core::Result,
    Win32::{Foundation::{POINT, RECT}, UI::WindowsAndMessaging::GetCursorPos},
};

pub enum Creature {
    Strip(Perch),
    Snake(Snake),
    Dolphin(Dolphin),
}

fn cursor_pos() -> Option<POINT> {
    let mut pt = POINT::default();
    unsafe { GetCursorPos(&mut pt) }.ok().map(|_| pt)
}

impl Creature {
    pub fn new(gfx: &Gfx, view: &LiveView, l: Loaded) -> Result<Self> {
        Ok(match l {
            Loaded::Strip(d) => Self::Strip(Perch::new(gfx, view, d)?),
            Loaded::Snake(s) => Self::Snake(Snake::new(gfx, view, s)?),
            Loaded::Dolphin(s) => Self::Dolphin(Dolphin::new(gfx, view, s)?),
        })
    }

    pub fn show(&mut self, on: bool) {
        match self {
            Self::Strip(p) => p.show(on),
            Self::Snake(s) => s.show(on),
            Self::Dolphin(d) => d.show(on),
        }
    }

    pub fn scale(&self) -> f32 {
        match self {
            Self::Strip(p) => p.scale(),
            Self::Snake(s) => s.scale(),
            Self::Dolphin(d) => d.scale(),
        }
    }

    /// `dt` saniye ilerletir ve çizer. Dönüş: sonraki tike kadar ms.
    pub fn step(&mut self, dt: f32, pill: RECT, size: (f32, f32), s: f32) -> f32 {
        match self {
            Self::Strip(p) => {
                p.follow(pill);
                let (lo, hi) = p.range(pill, s);
                p.pet.update(dt * 1000.0, lo, hi);
                if p.pet.take_dirty() {
                    let _ = p.render();
                }
                p.pet.next_ms()
            }
            // İmleç yalnızca çalışırken, tikte okunur (ayrı kanca yok).
            Self::Snake(sn) => sn.step(dt, pill, cursor_pos()),
            Self::Dolphin(d) => d.step(dt, pill, size),
        }
    }

    /// Durağan kare (hareketi azalt).
    pub fn render(&mut self, pill: RECT) {
        let _ = match self {
            Self::Strip(p) => {
                p.follow(pill);
                p.render()
            }
            Self::Snake(s) => s.render(pill),
            Self::Dolphin(d) => d.render(pill),
        };
    }

    /// Yeni pencere açıldı: hayvan irkilir/uyanır.
    pub fn poke(&mut self) {
        match self {
            Self::Strip(p) => p.pet.poke(),
            Self::Snake(s) => s.poke(),
            Self::Dolphin(d) => d.poke(),
        }
    }

    /// Fare dock'un üstünde gezindi ya da çıktı. Bekleme süresi değiştiyse
    /// yeni süreyi (ms) döndürür.
    pub fn cursor(&mut self, inside: bool) -> Option<f32> {
        match self {
            Self::Strip(p) => {
                let x = inside.then(cursor_pos).flatten().map(|pt| p.local_x(pt.x));
                let k = p.scale();
                p.pet.cursor(x, pet::LOOK_RADIUS * k);
                p.pet.take_dirty().then(|| {
                    let _ = p.render();
                    p.pet.next_ms()
                })
            }
            // Yılan imleci kendi tikinde okur; kıvrıkken hemen tepki versin.
            Self::Snake(_) => Some(100.0),
            Self::Dolphin(_) => None,
        }
    }
}
