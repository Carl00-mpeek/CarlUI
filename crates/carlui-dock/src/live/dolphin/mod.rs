//! Yunus: camın içinde ikonların arkasında, düşük saydamlıkla yüzer; ara
//! sıra dock'un üst kenarından dışarı sıçrar. Davranış `brain.rs`'de.
//!
//! İki kopya aynı konuma çizilir: camın içerik katmanının EN ALTINDA
//! (%35 saydam, suyun altı) ve tünekte (tam opak). Tünek dock'un üst
//! kenarında kırpıldığı için tünek kopyası yalnızca su yüzeyinin üstünde
//! görünür. Su sıçramaları yalnızca tünekte, alt satırları kenara oturur.

mod brain;

use super::{gpu, load::Sheets, rng::Rng, window::PerchWindow};
use crate::dock::LiveView;
use brain::{Body, Dolphin as Brain, SURFACE_Y};
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    Win32::Foundation::{E_FAIL, RECT},
    UI::Composition::*,
};

/// Tüneğin kenar üstündeki yüksekliği (sanat pikseli): sıçramanın tepesi sığar.
/// Tepe: SURFACE_Y - LEAP_H - LEAP.1/2 = 7 - 33 - 24 = -50; 12 px pay.
const UP: f32 = 62.0;
/// Camın içindeki kopyanın saydamlığı.
const UNDER: f32 = 0.55;
// Şeritler ilk çizimin ×1,2'si (en yakın komşu + dış çizgi inceltme, bulanıklık yok).
const SWIM: (f32, f32) = (48.0, 28.0);
const LEAP: (f32, f32) = (48.0, 48.0);
const SPLASH: (f32, f32) = (40.0, 20.0);

type Brushes = Vec<CompositionSurfaceBrush>;

pub struct Dolphin {
    win: PerchWindow,
    brain: Brain,
    swim: Brushes,
    leap: Brushes,
    splash_out: Brushes,
    splash_in: Brushes,
    v_top: SpriteVisual,
    v_splash: SpriteVisual,
    v_under: SpriteVisual,
    content: ContainerVisual,
    k: f32,
    last: Option<(Body, i32, i32, bool, Option<(bool, u8, i32)>, i32)>,
}

fn strip(gfx: &Gfx, sh: &Sheets, name: &str, n: u32, size: (f32, f32)) -> Result<Brushes> {
    let s = sh.get(name).ok_or_else(|| Error::new(E_FAIL, format!("{name} yok")))?;
    if s.w != n * size.0 as u32 || s.h != size.1 as u32 {
        return Err(Error::new(E_FAIL, format!("{name}: boyut {}x{}", s.w, s.h)));
    }
    gpu::frames(gfx, s, n)
}

impl Dolphin {
    pub fn new(gfx: &Gfx, view: &LiveView, sheets: Sheets) -> Result<Self> {
        let swim = strip(gfx, &sheets, "swim.png", 8, SWIM)?;
        let leap = strip(gfx, &sheets, "leap.png", 5, LEAP)?;
        let splash_out = strip(gfx, &sheets, "splash_out.png", 6, SPLASH)?;
        let splash_in = strip(gfx, &sheets, "splash_in.png", 6, SPLASH)?;
        drop(sheets);
        let k = view.scale.round().max(1.0);
        let up = (UP * k) as i32;
        let win = PerchWindow::new(gfx, view, up, up)?;
        let v_top = gfx.comp.CreateSpriteVisual()?;
        let v_splash = gfx.comp.CreateSpriteVisual()?;
        let v_under = gfx.comp.CreateSpriteVisual()?;
        v_splash.SetSize(v2(SPLASH.0 * k, SPLASH.1 * k))?;
        v_splash.SetCenterPoint(v3(SPLASH.0 * k / 2.0, 0.0))?;
        v_under.SetOpacity(UNDER)?;
        let kids = win.root.Children()?;
        kids.InsertAtTop(&v_top)?;
        kids.InsertAtTop(&v_splash)?;
        view.content.Children()?.InsertAtBottom(&v_under)?;
        let (w, h) = (view.size.0 / k, view.size.1 / k);
        let brain = Brain::new(w / 2.0, (h / 2.0).max(SURFACE_Y), Rng::from_time());
        let mut d = Self { win, brain, swim, leap, splash_out, splash_in, v_top, v_splash, v_under, content: view.content.clone(), k, last: None };
        d.render(view.pill)?;
        Ok(d)
    }

    pub fn show(&mut self, on: bool) {
        self.win.show(on);
        let _ = self.v_under.SetIsVisible(on);
    }

    pub fn scale(&self) -> f32 {
        self.k
    }

    pub fn poke(&mut self) {
        self.brain.poke();
    }

    pub fn step(&mut self, dt: f32, pill: RECT, size: (f32, f32)) -> f32 {
        let ms = self.brain.step(dt, size.0 / self.k, size.1 / self.k);
        let _ = self.render(pill);
        ms
    }

    pub fn render(&mut self, pill: RECT) -> Result<()> {
        self.win.follow(pill);
        let k = self.k;
        let b = &self.brain;
        let body = b.body();
        let mirror = b.dir < 0.0;
        // Camın içinde, pill'in sol üstüne göre piksel.
        let (gx, gy) = ((b.x * k).round() as i32, (b.y * k).round() as i32);
        let splash = b.splash.map(|s| (s.entry, s.frame, ((s.x - SPLASH.0 / 2.0) * k).round() as i32));
        let now = (body, gx, gy, mirror, splash, pill.left);
        if self.last == Some(now) {
            return Ok(());
        }
        let (brush, size) = match body {
            Body::Swim(i) => (self.swim.get(i as usize), SWIM),
            Body::Leap(i) => (self.leap.get(i as usize), LEAP),
        };
        let (w, h) = (size.0 * k, size.1 * k);
        let sx = Vector3 { X: if mirror { -1.0 } else { 1.0 }, Y: 1.0, Z: 1.0 };
        let dx = (pill.left - self.win.left) as f32;
        let up = UP * k;
        for (v, ox, oy) in [(&self.v_top, dx, up), (&self.v_under, 0.0, 0.0)] {
            if self.last.map(|l| l.0) != Some(body) {
                if let Some(br) = brush {
                    v.SetBrush(br)?;
                }
                v.SetSize(v2(w, h))?;
                v.SetCenterPoint(v3(w / 2.0, 0.0))?;
            }
            v.SetScale(sx)?;
            v.SetOffset(v3(ox + gx as f32 - w / 2.0, oy + gy as f32 - h / 2.0))?;
        }
        match splash {
            Some((entry, frame, x)) => {
                let set = if entry { &self.splash_in } else { &self.splash_out };
                if let Some(br) = set.get(frame as usize) {
                    self.v_splash.SetBrush(br)?;
                }
                self.v_splash.SetScale(sx)?;
                // Alt satır dock'un üst kenarında.
                self.v_splash.SetOffset(v3(dx + x as f32, up - SPLASH.1 * k))?;
                self.v_splash.SetIsVisible(true)?;
            }
            None => self.v_splash.SetIsVisible(false)?,
        }
        self.last = Some(now);
        Ok(())
    }
}

impl Drop for Dolphin {
    fn drop(&mut self) {
        if let Ok(kids) = self.content.Children() {
            let _ = kids.Remove(&self.v_under);
        }
    }
}
