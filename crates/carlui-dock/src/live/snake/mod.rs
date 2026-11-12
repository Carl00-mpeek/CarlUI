//! Yılan: yordamsal, boncuklu gövde. Geometri `path`, davranış `brain`,
//! çizim parçaları `art`. Gövde CPU'da iki küçük tuvale (ön ve arka) çizilir;
//! çizim listesi değişmedikçe tuval yeniden yüklenmez.
//!
//! Katmanlar: arka tuval yalnızca tünekte (dock'un üst kenarında kırpılır:
//! helezonun arka yarısı dock'un arkasına girer). Ön tuval hem tünekte hem
//! camın içerik katmanının en üstünde: helezonun ön yarısı camın içinde,
//! ikonların önünde görünür.

mod art;
mod brain;
mod debug;
mod path;

use super::{gpu::Canvas, load::Sheets, rng::Rng, window::PerchWindow};
use crate::dock::LiveView;
use art::{Art, Op};
use brain::{Brain, CH, CW, UP};
use debug::Force;
use carlui_ui::{
    gfx::Gfx,
    glass::{v2, v3},
};
use windows::{
    core::*,
    Foundation::Numerics::Vector3,
    Win32::Foundation::{POINT, RECT},
    UI::Composition::*,
};

pub struct Snake {
    win: PerchWindow,
    art: Art,
    brain: Brain,
    front: Canvas,
    back: Canvas,
    v_front: SpriteVisual,
    v_back: SpriteVisual,
    /// Ön tuvalin camın içindeki kopyası.
    v_glass: SpriteVisual,
    content: ContainerVisual,
    k: f32,
    last_front: Vec<Op>,
    last_back: Vec<Op>,
    last_pos: Option<(i32, bool, i32)>,
    /// Yalnız geliştirme: zorlanan durum ve kendi saati (bkz. `debug.rs`).
    force: Option<Force>,
    force_t: f32,
}

impl Snake {
    pub fn new(gfx: &Gfx, view: &LiveView, sheets: Sheets) -> Result<Self> {
        let art = Art::new(&sheets).map_err(|e| Error::new(windows::Win32::Foundation::E_FAIL, e))?;
        drop(sheets); // sayfalar parçalara kesildi; çözülmüş PNG'ler bırakılır
        let k = view.scale.round().max(1.0);
        let up = (UP as f32 * k) as i32;
        let win = PerchWindow::new(gfx, view, up, up)?;
        let front = Canvas::new(gfx, CW as u32, CH as u32)?;
        let back = Canvas::new(gfx, CW as u32, CH as u32)?;
        let size = v2(CW as f32 * k, CH as f32 * k);
        let mk = |c: &Canvas| -> Result<SpriteVisual> {
            let v = gfx.comp.CreateSpriteVisual()?;
            v.SetSize(size)?;
            v.SetCenterPoint(v3(size.X / 2.0, 0.0))?;
            v.SetBrush(&c.brush)?;
            Ok(v)
        };
        let (v_front, v_back, v_glass) = (mk(&front)?, mk(&back)?, mk(&front)?);
        let kids = win.root.Children()?;
        kids.InsertAtTop(&v_back)?;
        kids.InsertAtTop(&v_front)?;
        view.content.Children()?.InsertAtTop(&v_glass)?;
        let mid = ((view.pill.left + view.pill.right) / 2 - win.left) as f32 / k;
        let brain = Brain::new(mid, Rng::from_time());
        let mut s = Self {
            win,
            art,
            brain,
            front,
            back,
            v_front,
            v_back,
            v_glass,
            content: view.content.clone(),
            k,
            last_front: Vec::new(),
            last_back: Vec::new(),
            last_pos: None,
            force: Force::read(),
            force_t: 0.0,
        };
        s.render(view.pill)?;
        Ok(s)
    }

    pub fn show(&mut self, on: bool) {
        self.win.show(on);
        let _ = self.v_glass.SetIsVisible(on);
    }

    pub fn scale(&self) -> f32 {
        self.k
    }

    pub fn poke(&mut self) {
        self.brain.poke();
    }

    /// Başın gezebileceği dünya aralığı (sanat pikseli).
    fn range(&self, pill: RECT) -> (f32, f32) {
        let pad = 6.0;
        let l = (pill.left - self.win.left) as f32 / self.k + pad;
        let r = (pill.right - self.win.left) as f32 / self.k - pad;
        (l, r.max(l))
    }

    /// Bir adım: davranış, sonra çizim. Dönüş: sonraki tike kadar ms.
    pub fn step(&mut self, dt: f32, pill: RECT, cursor: Option<POINT>) -> f32 {
        let (lo, hi) = self.range(pill);
        let mut cur = cursor.map(|p| ((p.x - self.win.left) as f32 / self.k, (p.y - pill.top) as f32 / self.k));
        // Yalnız geliştirme: zorlanan durum (yoksa hiçbir şey yapmaz).
        if let Some(f) = self.force {
            self.force_t += dt;
            let fake = f.apply(&mut self.brain, self.force_t, lo, hi);
            if f == Force::Track {
                cur = fake;
            }
        }
        let ms = self.brain.step(dt, lo, hi, cur);
        let _ = self.render(pill);
        ms
    }

    /// Pozu tuvallere ve görsellere aktarır; değişmeyen kısma dokunmaz.
    pub fn render(&mut self, pill: RECT) -> Result<()> {
        self.win.follow(pill);
        let f = self.brain.frame();
        if f.front != self.last_front {
            self.front.px.fill(0);
            art::paint(&self.art, &f.front, &mut self.front.px, CW, CH);
            self.front.upload()?;
            self.last_front = f.front;
        }
        if f.back != self.last_back {
            self.back.px.fill(0);
            art::paint(&self.art, &f.back, &mut self.back.px, CW, CH);
            self.back.upload()?;
            self.last_back = f.back;
        }
        let mirror = f.dir < 0.0;
        // Tuvalin dünya sol kenarı (aynalıyken yerel sağ kenar dünyada solda).
        let left = if mirror { f.ox - (f.x0 + CW) as f32 } else { f.ox + f.x0 as f32 };
        let px = (left * self.k).round() as i32;
        let glass_dx = px + self.win.left - pill.left;
        let now = (px, mirror, glass_dx);
        if self.last_pos == Some(now) {
            return Ok(());
        }
        let sx = Vector3 { X: if mirror { -1.0 } else { 1.0 }, Y: 1.0, Z: 1.0 };
        for v in [&self.v_front, &self.v_back] {
            v.SetScale(sx)?;
            v.SetOffset(v3(px as f32, 0.0))?;
        }
        self.v_glass.SetScale(sx)?;
        // Camın içeriğinin (0, 0)'ı pill'in sol üst köşesi; tuvalin üstü kenarın UP üstünde.
        self.v_glass.SetOffset(v3(glass_dx as f32, -(UP as f32) * self.k))?;
        self.last_pos = Some(now);
        Ok(())
    }
}

impl Drop for Snake {
    fn drop(&mut self) {
        if let Ok(kids) = self.content.Children() {
            let _ = kids.Remove(&self.v_glass);
        }
    }
}
